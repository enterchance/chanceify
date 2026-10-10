//! Playing the song files in the local songs folder.
//!
//! The deck has its own thread. It decodes a file with symphonia, turns the
//! sound into the stereo 44.1 kHz that Spotify's own audio path carries, and
//! hands it to the same output the Spotify player uses (the equalizer, the
//! limiter, the speed change and the visualizer tap all sit in front of it),
//! so a song from a file sounds and looks like any other song.
//!
//! The window never waits for it: it sends commands down a channel and reads
//! a snapshot ([`DeckState`]) that the deck updates when something changes.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use librespot_playback::audio_backend::Sink;
use librespot_playback::config::VolumeCtrl;
use librespot_playback::convert::Converter;
use librespot_playback::decoder::AudioPacket;
use librespot_playback::mixer::{self, Mixer, MixerConfig};
use librespot_playback::{NUM_CHANNELS, SAMPLE_RATE};
use sha1::{Digest, Sha1};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, Decoder, DecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, MetadataRevision, StandardVisualKey};
use symphonia::core::probe::Hint;
use symphonia::core::units::{Time, TimeBase};

use crate::player::RepeatMode;
use crate::resample::Resampler;
use crate::sink::{AudioControl, ErrorHook, RodioSink};
use crate::vis::{AudioTap, Tapped};

/// What a song from a file is called to the rest of the program. Spotify's
/// are `spotify:track:...`; these are never sent to Spotify.
pub const URI_PREFIX: &str = "local:file:";

/// Whether a song address is a file played by the deck.
pub fn is_file_uri(uri: &str) -> bool {
    uri.starts_with(URI_PREFIX)
}

/// The address a file's song goes by: a short fingerprint of its path.
pub fn uri_for(path: &Path) -> String {
    let digest = Sha1::digest(path.to_string_lossy().as_bytes());
    let mut id = String::with_capacity(16);
    for byte in digest.iter().take(8) {
        use std::fmt::Write;
        let _ = write!(id, "{byte:02x}");
    }
    format!("{URI_PREFIX}{id}")
}

/// Frames in each piece handed to the output. Small pieces keep the queue
/// short, so a pause or a skip is heard quickly and the shown position
/// stays close to what is audible.
const PIECE_FRAMES: usize = 1024;

/// How far behind the written position the sound being heard is, taken off
/// the position shown while playing.
const OUTPUT_LAG_MS: u32 = 200;

/// How often a playing deck refreshes the position it publishes.
const PUBLISH_EVERY: Duration = Duration::from_millis(750);

/// Previous restarts the song when it is further in than this.
const RESTART_AFTER_MS: u32 = 3000;

/// What the deck is told about the output, each time something plays.
#[derive(Clone)]
pub struct DeckConfig {
    /// The output device from Settings; `None` is the default one.
    pub device: Option<String>,
    pub buffer_ms: u32,
    pub tap: Arc<AudioTap>,
    pub eq: crate::eq::SharedEq,
    pub speed: crate::speed::SharedSpeed,
    /// Where the volume starts, on Spotify's 0 to 65535 scale.
    pub volume: u16,
    /// How long one song fades into the next, in milliseconds; 0 for none.
    /// This is for these files only: Spotify's own songs are not touched.
    pub crossfade_ms: u32,
}

/// A file to play and what is known about it from the scan.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeckEntry {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: u32,
}

/// The song on now.
#[derive(Clone, Debug, Default)]
pub struct DeckTrack {
    pub uri: String,
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: u32,
    /// The picture stored inside the file, if it has one.
    pub cover: Option<Arc<[u8]>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DeckPlayback {
    #[default]
    Stopped,
    Playing,
    Paused,
}

/// A snapshot of the deck for the window to read.
#[derive(Clone, Debug, Default)]
pub struct DeckState {
    pub playback: DeckPlayback,
    pub track: Option<DeckTrack>,
    pub position_ms: u32,
    /// When `position_ms` was taken; `None` while not advancing.
    pub position_at: Option<Instant>,
    pub volume: u16,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    /// Counts up each time a song starts, even the same one again.
    pub track_sequence: u64,
    /// A message for the person, taken once by [`FileDeck::take_error`].
    pub error: Option<String>,
}

impl DeckState {
    pub fn is_active(&self) -> bool {
        self.playback != DeckPlayback::Stopped && self.track.is_some()
    }

    /// The position now, carried on from the last report while playing.
    pub fn position_now(&self) -> u32 {
        match (self.playback, self.position_at) {
            (DeckPlayback::Playing, Some(at)) => {
                let limit = self
                    .track
                    .as_ref()
                    .map_or(u32::MAX, |track| track.duration_ms.max(self.position_ms));
                self.position_ms
                    .saturating_add(at.elapsed().as_millis() as u32)
                    .min(limit)
            }
            _ => self.position_ms,
        }
    }
}

enum Cmd {
    Configure(DeckConfig),
    Play { entries: Vec<DeckEntry>, index: usize },
    Toggle,
    Next,
    Previous,
    Seek(u32),
    Volume(u16),
    Shuffle(bool),
    Repeat(RepeatMode),
    Stop,
    Quit,
}

/// The handle the window keeps.
pub struct FileDeck {
    tx: Sender<Cmd>,
    state: Arc<Mutex<DeckState>>,
}

impl FileDeck {
    /// Starts the deck's thread. `notify` is called whenever the snapshot
    /// changes, so the window can redraw.
    pub fn new(config: DeckConfig, notify: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (tx, rx) = mpsc::channel();
        let state = Arc::new(Mutex::new(DeckState {
            volume: config.volume,
            ..DeckState::default()
        }));
        let shared = Arc::clone(&state);
        let started = std::thread::Builder::new()
            .name("file-deck".into())
            .spawn(move || Engine::new(rx, shared, notify, config).run());
        if let Err(error) = started {
            log::error!("could not start the file player: {error}");
            state.lock().unwrap_or_else(PoisonError::into_inner).error =
                Some("Could not start the file player".into());
        }
        Self { tx, state }
    }

    pub fn state(&self) -> DeckState {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn take_error(&self) -> Option<String> {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .error
            .take()
    }

    fn send(&self, command: Cmd) {
        // A deck whose thread has gone has nothing left to tell.
        let _ = self.tx.send(command);
    }

    /// Plays `entries` from `index`, with the output as `config` says.
    pub fn play(&self, config: DeckConfig, entries: Vec<DeckEntry>, index: usize) {
        self.send(Cmd::Configure(config));
        self.send(Cmd::Play { entries, index });
    }

    pub fn toggle(&self) {
        self.send(Cmd::Toggle);
    }

    pub fn next(&self) {
        self.send(Cmd::Next);
    }

    pub fn previous(&self) {
        self.send(Cmd::Previous);
    }

    pub fn seek(&self, position_ms: u32) {
        self.send(Cmd::Seek(position_ms));
    }

    pub fn set_volume(&self, volume: u16) {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .volume = volume;
        self.send(Cmd::Volume(volume));
    }

    pub fn set_shuffle(&self, shuffle: bool) {
        self.send(Cmd::Shuffle(shuffle));
    }

    pub fn set_repeat(&self, repeat: RepeatMode) {
        self.send(Cmd::Repeat(repeat));
    }

    pub fn stop(&self) {
        self.send(Cmd::Stop);
    }
}

impl Drop for FileDeck {
    fn drop(&mut self) {
        self.send(Cmd::Quit);
    }
}

// ---- reading a file ------------------------------------------------------

/// One file open for playing.
struct Reader {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
    rate: u32,
    time_base: Option<TimeBase>,
    resampler: Option<Resampler>,
    buffer: Option<SampleBuffer<f32>>,
    cover: Option<Arc<[u8]>>,
    duration_ms: u32,
}

fn cover_of(revision: &MetadataRevision) -> Option<Arc<[u8]>> {
    let visuals = revision.visuals();
    let chosen = visuals
        .iter()
        .find(|visual| visual.usage == Some(StandardVisualKey::FrontCover))
        .or_else(|| visuals.first())?;
    if chosen.data.is_empty() {
        return None;
    }
    Some(Arc::from(&chosen.data[..]))
}

impl Reader {
    fn open(path: &Path) -> Result<Self, String> {
        let file = File::open(path).map_err(|error| error.to_string())?;
        let stream = MediaSourceStream::new(Box::new(file), Default::default());
        let mut hint = Hint::new();
        if let Some(extension) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(extension);
        }
        let mut probed = symphonia::default::get_probe()
            .format(
                &hint,
                stream,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )
            .map_err(|error| format!("not a song chanceify™ can read ({error})"))?;
        let mut cover = None;
        if let Some(metadata) = probed.metadata.get() {
            if let Some(revision) = metadata.current() {
                cover = cover_of(revision);
            }
        }
        let mut format = probed.format;
        if cover.is_none() {
            let metadata = format.metadata();
            if let Some(revision) = metadata.current() {
                cover = cover_of(revision);
            }
        }
        let (track_id, params) = {
            let track = format
                .tracks()
                .iter()
                .find(|track| track.codec_params.codec != CODEC_TYPE_NULL)
                .ok_or_else(|| "no sound in this file".to_string())?;
            (track.id, track.codec_params.clone())
        };
        let rate = params
            .sample_rate
            .ok_or_else(|| "unknown sample rate".to_string())?;
        if rate == 0 {
            return Err("unknown sample rate".into());
        }
        let decoder = symphonia::default::get_codecs()
            .make(&params, &DecoderOptions::default())
            .map_err(|error| format!("cannot play this kind of file ({error})"))?;
        let duration_ms = params
            .n_frames
            .map(|frames| (frames * 1000 / u64::from(rate)) as u32)
            .unwrap_or(0);
        Ok(Self {
            format,
            decoder,
            track_id,
            rate,
            time_base: params.time_base,
            resampler: Resampler::new(rate, SAMPLE_RATE, 2),
            buffer: None,
            cover,
            duration_ms,
        })
    }

    /// The next stretch of sound as interleaved stereo at 44.1 kHz, or
    /// `None` at the end of the file.
    fn next(&mut self) -> Result<Option<Vec<f32>>, String> {
        let mut bad_packets = 0u32;
        loop {
            let packet = match self.format.next_packet() {
                Ok(packet) => packet,
                Err(SymphoniaError::IoError(error))
                    if error.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    return Ok(None);
                }
                Err(SymphoniaError::ResetRequired) => return Ok(None),
                Err(error) => return Err(error.to_string()),
            };
            if packet.track_id() != self.track_id {
                continue;
            }
            match self.decoder.decode(&packet) {
                Ok(decoded) => {
                    let spec = *decoded.spec();
                    let channels = spec.channels.count().max(1);
                    let needed = decoded.frames() * channels;
                    if self
                        .buffer
                        .as_ref()
                        .is_none_or(|buffer| buffer.capacity() < needed)
                    {
                        self.buffer =
                            Some(SampleBuffer::<f32>::new(decoded.capacity() as u64, spec));
                    }
                    let Some(buffer) = self.buffer.as_mut() else {
                        continue;
                    };
                    buffer.copy_interleaved_ref(decoded);
                    let stereo = to_stereo(buffer.samples(), channels);
                    let out = match &mut self.resampler {
                        Some(resampler) => resampler.process(&stereo),
                        None => stereo,
                    };
                    if out.is_empty() {
                        continue;
                    }
                    return Ok(Some(out));
                }
                // One bad packet is skipped; the rest of the song is there.
                Err(SymphoniaError::DecodeError(_)) => {
                    bad_packets += 1;
                    if bad_packets > 200 {
                        return Err("this file is damaged".into());
                    }
                }
                Err(error) => return Err(error.to_string()),
            }
        }
    }

    /// Moves to `position_ms`; returns where it actually landed.
    fn seek(&mut self, position_ms: u32) -> Result<u32, String> {
        let seconds = u64::from(position_ms / 1000);
        let fraction = f64::from(position_ms % 1000) / 1000.0;
        let landed = self
            .format
            .seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time: Time::new(seconds, fraction),
                    track_id: Some(self.track_id),
                },
            )
            .map_err(|error| error.to_string())?;
        self.decoder.reset();
        self.resampler = Resampler::new(self.rate, SAMPLE_RATE, 2);
        let actual = self
            .time_base
            .map(|base| {
                let time = base.calc_time(landed.actual_ts);
                (time.seconds * 1000) as u32 + (time.frac * 1000.0) as u32
            })
            .unwrap_or(position_ms);
        Ok(actual)
    }
}

/// Interleaved stereo from any number of channels. Mono is doubled; a
/// surround mix is folded down; anything else keeps its first two.
fn to_stereo(samples: &[f32], channels: usize) -> Vec<f32> {
    let channels = channels.max(1);
    let frames = samples.len() / channels;
    let mut out = Vec::with_capacity(frames * 2);
    for frame in samples.chunks_exact(channels) {
        let (left, right) = match channels {
            1 => (frame[0], frame[0]),
            2..=5 => (frame[0], frame[1]),
            _ => {
                // Front pair, centre, then the rear pair.
                let centre = frame[2] * 0.707;
                (
                    (frame[0] + centre + frame[4] * 0.5) * 0.6,
                    (frame[1] + centre + frame[5] * 0.5) * 0.6,
                )
            }
        };
        out.push(left);
        out.push(right);
    }
    out
}

/// The order songs are played in, and where `keep` sits in it. A shuffle
/// puts `keep` first, so turning it on does not skip the song playing.
fn build_order(len: usize, keep: usize, shuffle: bool) -> (Vec<usize>, usize) {
    let mut order: Vec<usize> = (0..len).collect();
    if !shuffle || len < 2 {
        return (order, keep.min(len.saturating_sub(1)));
    }
    for i in (1..len).rev() {
        order.swap(i, rand::random_range(0..=i));
    }
    let keep = keep.min(len - 1);
    if let Some(at) = order.iter().position(|&i| i == keep) {
        order.swap(0, at);
    }
    (order, 0)
}

// ---- the deck's thread ---------------------------------------------------

struct Engine {
    rx: Receiver<Cmd>,
    shared: Arc<Mutex<DeckState>>,
    notify: Arc<dyn Fn() + Send + Sync>,
    config: DeckConfig,
    mixer: Option<Arc<dyn Mixer>>,
    audio: Arc<AudioControl>,
    sink: Option<Box<dyn Sink>>,
    /// The sink has been started and not stopped since.
    sink_on: bool,
    converter: Converter,
    list: Vec<DeckEntry>,
    order: Vec<usize>,
    at: usize,
    shuffle: bool,
    repeat: RepeatMode,
    reader: Option<Reader>,
    playing: bool,
    /// Where in the song the sound written so far began, and how much has
    /// been written since, in 44.1 kHz frames.
    base_ms: u32,
    frames_written: u64,
    last_publish: Instant,
    /// Songs in a row that would not open or play.
    failures: usize,
    track_sequence: u64,
    current: Option<DeckTrack>,
    /// The next song, already playing under the end of this one.
    fade: Option<Fade>,
    /// The song (by `track_sequence`) whose next file would not open for a
    /// fade, so it is not tried again every step.
    fade_failed_for: u64,
}

/// A fade from the song playing into the one after it.
struct Fade {
    reader: Reader,
    entry: DeckEntry,
    at: usize,
    /// How many frames the fade lasts, and how many have been mixed.
    total: u64,
    done: u64,
}

/// Mixes the end of one song with the start of the next, as interleaved
/// stereo. The gains keep the loudness steady (equal power): the old song
/// follows a quarter of a cosine down, the new one a quarter of a sine up.
/// `done` frames of a fade of `total` have been mixed already.
fn mix_fade(outgoing: &[f32], incoming: &[f32], done: u64, total: u64) -> Vec<f32> {
    let length = outgoing.len().max(incoming.len());
    let mut mixed = Vec::with_capacity(length);
    for (index, pair) in (0..length).step_by(2).enumerate() {
        let progress = ((done + index as u64) as f32 / total.max(1) as f32).min(1.0);
        let angle = progress * std::f32::consts::FRAC_PI_2;
        let (down, up) = (angle.cos(), angle.sin());
        for channel in 0..2 {
            let at = pair + channel;
            if at >= length {
                break;
            }
            let old = outgoing.get(at).copied().unwrap_or(0.0);
            let new = incoming.get(at).copied().unwrap_or(0.0);
            mixed.push(old * down + new * up);
        }
    }
    mixed
}

impl Engine {
    fn new(
        rx: Receiver<Cmd>,
        shared: Arc<Mutex<DeckState>>,
        notify: Arc<dyn Fn() + Send + Sync>,
        config: DeckConfig,
    ) -> Self {
        let audio = AudioControl::new(config.buffer_ms);
        Self {
            rx,
            shared,
            notify,
            config,
            mixer: None,
            audio,
            sink: None,
            sink_on: false,
            converter: Converter::new(None),
            list: Vec::new(),
            order: Vec::new(),
            at: 0,
            shuffle: false,
            repeat: RepeatMode::Off,
            reader: None,
            playing: false,
            base_ms: 0,
            frames_written: 0,
            last_publish: Instant::now(),
            failures: 0,
            track_sequence: 0,
            current: None,
            fade: None,
            fade_failed_for: 0,
        }
    }

    fn run(mut self) {
        loop {
            let streaming = self.playing && self.reader.is_some();
            if !streaming {
                match self.rx.recv() {
                    Ok(command) => {
                        if !self.handle(command) {
                            return;
                        }
                    }
                    Err(_) => return,
                }
            }
            loop {
                match self.rx.try_recv() {
                    Ok(command) => {
                        if !self.handle(command) {
                            return;
                        }
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return,
                }
            }
            if self.playing && self.reader.is_some() {
                self.step();
            }
        }
    }

    /// Returns false when the deck is to shut down.
    fn handle(&mut self, command: Cmd) -> bool {
        match command {
            Cmd::Quit => return false,
            Cmd::Configure(config) => {
                let moved = config.device != self.config.device
                    || config.buffer_ms != self.config.buffer_ms;
                self.config = config;
                if moved {
                    // The output opens again, on the new device, with the
                    // song that follows.
                    self.sink = None;
                    self.sink_on = false;
                    self.audio = AudioControl::new(self.config.buffer_ms);
                }
                let volume = self.config.volume;
                if let Some(mixer) = self.mixer() {
                    mixer.set_volume(volume);
                }
            }
            Cmd::Play { entries, index } => {
                if entries.is_empty() {
                    return true;
                }
                let index = index.min(entries.len() - 1);
                self.list = entries;
                let (order, at) = build_order(self.list.len(), index, self.shuffle);
                self.order = order;
                self.at = at;
                self.failures = 0;
                self.go_to(self.at, true);
            }
            Cmd::Toggle => {
                if self.reader.is_none() {
                    return true;
                }
                if self.playing {
                    self.playing = false;
                    self.publish();
                    if self.sink_on {
                        if let Some(sink) = self.sink.as_mut() {
                            let _ = sink.stop();
                        }
                        self.sink_on = false;
                    }
                } else {
                    self.playing = true;
                    self.start_sink();
                    self.publish();
                }
            }
            Cmd::Next => {
                if self.reader.is_some() && !self.order.is_empty() {
                    let next = self.after(false);
                    match next {
                        Some(at) => self.go_to(at, true),
                        None => self.go_to(0, true),
                    }
                }
            }
            Cmd::Previous => {
                if self.reader.is_none() || self.order.is_empty() {
                    return true;
                }
                if self.position_ms() > RESTART_AFTER_MS || self.at == 0 {
                    self.seek_to(0);
                } else {
                    self.go_to(self.at - 1, true);
                }
            }
            Cmd::Seek(position_ms) => self.seek_to(position_ms),
            Cmd::Volume(volume) => {
                if let Some(mixer) = self.mixer() {
                    mixer.set_volume(volume);
                }
            }
            Cmd::Shuffle(shuffle) => {
                if shuffle != self.shuffle {
                    self.shuffle = shuffle;
                    if !self.list.is_empty() {
                        let current = self.order.get(self.at).copied().unwrap_or(0);
                        let (order, at) = build_order(self.list.len(), current, shuffle);
                        self.order = order;
                        self.at = at;
                    }
                }
                self.publish();
            }
            Cmd::Repeat(repeat) => {
                self.repeat = repeat;
                self.publish();
            }
            Cmd::Stop => self.finish(),
        }
        true
    }

    fn mixer(&mut self) -> Option<&Arc<dyn Mixer>> {
        if self.mixer.is_none() {
            let built = mixer::find(Some("softvol")).map(|builder| {
                builder(MixerConfig {
                    volume_ctrl: VolumeCtrl::Cubic(VolumeCtrl::DEFAULT_DB_RANGE),
                    ..MixerConfig::default()
                })
            });
            match built {
                Some(Ok(mixer)) => {
                    mixer.set_volume(self.config.volume);
                    self.mixer = Some(mixer);
                }
                _ => log::error!("the file player has no volume control"),
            }
        }
        self.mixer.as_ref()
    }

    /// Opens the output the first time it is wanted.
    fn make_sink(&mut self) -> bool {
        if self.sink.is_some() {
            return true;
        }
        let (output_volume, ceiling) = match self.mixer() {
            Some(mixer) => (mixer.get_soft_volume(), mixer.get_soft_volume()),
            None => return false,
        };
        let shared = Arc::clone(&self.shared);
        let notify = Arc::clone(&self.notify);
        let report: ErrorHook = Arc::new(move |message: String| {
            shared
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .error = Some(message);
            notify();
        });
        let output = RodioSink::new(
            self.config.device.clone(),
            report,
            output_volume,
            self.config.buffer_ms,
            Arc::clone(&self.audio),
            Arc::clone(&self.config.speed),
        );
        let tapped = Tapped::new(
            Box::new(output),
            Arc::clone(&self.config.tap),
            ceiling,
            false,
            Arc::clone(&self.config.eq),
            Arc::clone(&self.config.speed),
            Arc::new(AtomicU64::new(1.0f64.to_bits())),
        );
        self.sink = Some(Box::new(tapped));
        true
    }

    fn start_sink(&mut self) {
        if self.sink_on || !self.make_sink() {
            return;
        }
        if let Some(sink) = self.sink.as_mut() {
            let _ = sink.start();
            self.sink_on = true;
        }
    }

    /// The position in the song, in milliseconds.
    fn position_ms(&self) -> u32 {
        let written = u64::from(self.base_ms) + self.frames_written * 1000 / u64::from(SAMPLE_RATE);
        let lag = if self.playing { u64::from(OUTPUT_LAG_MS) } else { 0 };
        written.saturating_sub(lag).min(u64::from(u32::MAX)) as u32
    }

    /// The place in the order that follows the current one, if there is one.
    /// `auto` is a song running out on its own, which repeat-one holds.
    fn after(&self, auto: bool) -> Option<usize> {
        if auto && self.repeat == RepeatMode::Track {
            return Some(self.at);
        }
        if self.at + 1 < self.order.len() {
            Some(self.at + 1)
        } else if self.repeat == RepeatMode::Context {
            Some(0)
        } else {
            None
        }
    }

    fn publish(&mut self) {
        self.last_publish = Instant::now();
        let position = self.position_ms();
        let playback = if self.reader.is_none() {
            DeckPlayback::Stopped
        } else if self.playing {
            DeckPlayback::Playing
        } else {
            DeckPlayback::Paused
        };
        {
            let mut state = self.shared.lock().unwrap_or_else(PoisonError::into_inner);
            state.playback = playback;
            state.track = if playback == DeckPlayback::Stopped {
                None
            } else {
                self.current.clone()
            };
            state.position_ms = position;
            state.position_at = (playback == DeckPlayback::Playing).then(Instant::now);
            state.shuffle = self.shuffle;
            state.repeat = self.repeat;
            state.track_sequence = self.track_sequence;
        }
        (self.notify)();
    }

    fn fail(&mut self, message: String) {
        log::warn!("file player: {message}");
        self.shared
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .error = Some(message);
    }

    /// Ends playback: nothing is on.
    fn finish(&mut self) {
        self.fade = None;
        self.playing = false;
        self.reader = None;
        self.current = None;
        if self.sink_on {
            if let Some(sink) = self.sink.as_mut() {
                let _ = sink.stop();
            }
            self.sink_on = false;
        }
        self.publish();
    }

    /// Starts the song at `at` in the order, from its beginning. `user` is a
    /// person's choice, which cuts the old sound off at once; a song that
    /// ran out on its own flows into the next without a gap.
    fn go_to(&mut self, at: usize, user: bool) {
        self.fade = None;
        let cut = user && self.sink_on && self.playing;
        if cut {
            self.audio.interrupt();
        }
        let mut at = at;
        let mut tried = 0;
        let opened = loop {
            if self.order.is_empty() || tried >= self.order.len() {
                break false;
            }
            let Some(entry) = self.order.get(at).and_then(|&i| self.list.get(i)).cloned() else {
                break false;
            };
            self.at = at;
            match Reader::open(&entry.path) {
                Ok(reader) => {
                    self.install(entry, reader);
                    break true;
                }
                Err(error) => {
                    self.fail(format!("Could not play {}: {error}", entry.title));
                    tried += 1;
                    match self.after(false) {
                        Some(next) if next != at => at = next,
                        _ => break false,
                    }
                }
            }
        };
        if cut {
            self.audio.track_changed();
        }
        if opened {
            self.playing = true;
            self.start_sink();
            self.publish();
        } else {
            self.finish();
        }
    }

    fn install(&mut self, entry: DeckEntry, reader: Reader) {
        let duration_ms = if entry.duration_ms > 0 {
            entry.duration_ms
        } else {
            reader.duration_ms
        };
        let title = if entry.title.is_empty() {
            entry
                .path
                .file_stem()
                .map(|stem| stem.to_string_lossy().to_string())
                .unwrap_or_default()
        } else {
            entry.title.clone()
        };
        self.current = Some(DeckTrack {
            uri: uri_for(&entry.path),
            path: entry.path,
            title,
            artist: entry.artist,
            album: entry.album,
            duration_ms,
            cover: reader.cover.clone(),
        });
        self.reader = Some(reader);
        self.base_ms = 0;
        self.frames_written = 0;
        self.failures = 0;
        self.track_sequence += 1;
    }

    fn seek_to(&mut self, position_ms: u32) {
        self.fade = None;
        let Some(reader) = self.reader.as_mut() else {
            return;
        };
        let duration = self.current.as_ref().map_or(0, |track| track.duration_ms);
        let target = if duration > 0 {
            position_ms.min(duration.saturating_sub(1))
        } else {
            position_ms
        };
        let cut = self.sink_on && self.playing;
        if cut {
            self.audio.interrupt();
        }
        let moved = reader.seek(target);
        if cut {
            self.audio.track_changed();
        }
        match moved {
            Ok(actual) => {
                self.base_ms = actual;
                self.frames_written = 0;
            }
            Err(error) => self.fail(format!("Could not jump there: {error}")),
        }
        self.publish();
    }

    /// Opens the next file for a fade once the song is near enough its end.
    fn maybe_start_fade(&mut self) {
        let wanted = self.config.crossfade_ms;
        if wanted == 0 || self.repeat == RepeatMode::Track || self.fade_failed_for == self.track_sequence {
            return;
        }
        let Some(duration) = self.current.as_ref().map(|track| track.duration_ms).filter(|d| *d > 0) else {
            return;
        };
        // A short song gives up at most a third of itself.
        let ms = wanted.min(duration / 3);
        let played = u64::from(self.base_ms) + self.frames_written * 1000 / u64::from(SAMPLE_RATE);
        if ms == 0 || played + u64::from(ms) < u64::from(duration) {
            return;
        }
        let Some(next_at) = self.after(true).filter(|next| *next != self.at) else {
            return;
        };
        let Some(entry) = self.order.get(next_at).and_then(|&i| self.list.get(i)).cloned() else {
            return;
        };
        match Reader::open(&entry.path) {
            Ok(reader) => {
                self.fade = Some(Fade {
                    reader,
                    entry,
                    at: next_at,
                    total: u64::from(ms) * u64::from(SAMPLE_RATE) / 1000,
                    done: 0,
                });
            }
            Err(error) => {
                log::debug!("crossfade: could not open the next file: {error}");
                self.fade_failed_for = self.track_sequence;
            }
        }
    }

    /// Decodes a little of both songs and plays the mix. When the fade is
    /// over the next song is simply the one playing.
    fn step_fade(&mut self) {
        let Some(reader) = self.reader.as_mut() else {
            self.fade = None;
            return;
        };
        let outgoing = match reader.next() {
            Ok(Some(samples)) => samples,
            _ => Vec::new(),
        };
        let Some(fade) = self.fade.as_mut() else {
            return;
        };
        let incoming = match fade.reader.next() {
            Ok(Some(samples)) => samples,
            _ => Vec::new(),
        };
        let mixed = mix_fade(&outgoing, &incoming, fade.done, fade.total);
        fade.done += (mixed.len() / NUM_CHANNELS as usize) as u64;
        let finished = outgoing.is_empty() || incoming.is_empty() || fade.done >= fade.total;
        if !mixed.is_empty() {
            self.write(&mixed);
        }
        if finished && let Some(fade) = self.fade.take() {
            let elapsed_ms = (fade.done * 1000 / u64::from(SAMPLE_RATE)) as u32;
            self.install(fade.entry, fade.reader);
            self.at = fade.at;
            self.base_ms = elapsed_ms;
            self.publish();
        }
    }

    /// Decodes a little and plays it.
    fn step(&mut self) {
        if self.fade.is_none() {
            self.maybe_start_fade();
        }
        if self.fade.is_some() {
            self.step_fade();
            return;
        }
        let Some(reader) = self.reader.as_mut() else {
            return;
        };
        match reader.next() {
            Ok(Some(samples)) => {
                self.failures = 0;
                self.write(&samples);
            }
            Ok(None) => match self.after(true) {
                Some(next) => self.go_to(next, false),
                None => self.finish(),
            },
            Err(error) => {
                self.failures += 1;
                let title = self
                    .current
                    .as_ref()
                    .map(|track| track.title.clone())
                    .unwrap_or_default();
                self.fail(format!("Could not play {title}: {error}"));
                match self.after(false) {
                    Some(next) if self.failures < self.list.len().max(1) => self.go_to(next, false),
                    _ => self.finish(),
                }
            }
        }
    }

    fn write(&mut self, samples: &[f32]) {
        for piece in samples.chunks(PIECE_FRAMES * NUM_CHANNELS as usize) {
            let wide: Vec<f64> = piece.iter().map(|&sample| f64::from(sample)).collect();
            let Some(sink) = self.sink.as_mut() else {
                return;
            };
            if let Err(error) = sink.write(AudioPacket::Samples(wide), &mut self.converter) {
                // The output stopped working. Pause where it was, say why.
                self.fail(error.to_string());
                self.playing = false;
                if let Some(sink) = self.sink.as_mut() {
                    let _ = sink.stop();
                }
                self.sink_on = false;
                self.publish();
                return;
            }
            self.frames_written += (piece.len() / NUM_CHANNELS as usize) as u64;
        }
        if self.last_publish.elapsed() >= PUBLISH_EVERY {
            self.publish();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(path: &Path, rate: u32, channels: u16, seconds: f32) {
        let frames = (rate as f32 * seconds) as u32;
        let data_len = frames * u32::from(channels) * 2;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&channels.to_le_bytes());
        bytes.extend_from_slice(&rate.to_le_bytes());
        bytes.extend_from_slice(&(rate * u32::from(channels) * 2).to_le_bytes());
        bytes.extend_from_slice(&(channels * 2).to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        for i in 0..frames {
            let value = ((i as f32 * 0.05).sin() * 8000.0) as i16;
            for _ in 0..channels {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        std::fs::write(path, bytes).expect("write the test file");
    }

    #[test]
    fn a_song_from_a_file_has_its_own_address() {
        let one = uri_for(Path::new("/music/a.mp3"));
        let two = uri_for(Path::new("/music/b.mp3"));
        assert!(is_file_uri(&one));
        assert_ne!(one, two);
        assert_eq!(one, uri_for(Path::new("/music/a.mp3")));
        assert!(!is_file_uri("spotify:track:abc"));
    }

    #[test]
    fn a_fade_keeps_the_loudness_steady() {
        let loud = vec![1.0_f32; 8];
        let start = mix_fade(&loud, &loud, 0, 1000);
        assert!((start[0] - 1.0).abs() < 0.01, "all old at the start");
        let middle = mix_fade(&loud, &loud, 500, 1000);
        assert!(middle[0] > 1.3 && middle[0] < 1.5, "equal power in the middle");
        let end = mix_fade(&loud, &[0.0; 8], 1000, 1000);
        assert!(end[0].abs() < 0.01, "none of the old at the end");
    }

    #[test]
    fn a_fade_survives_one_song_being_shorter() {
        let mixed = mix_fade(&[1.0; 4], &[1.0; 8], 0, 100);
        assert_eq!(mixed.len(), 8);
    }

    #[test]
    fn mono_is_doubled_and_surround_is_folded() {
        assert_eq!(to_stereo(&[0.5, -0.5], 1), vec![0.5, 0.5, -0.5, -0.5]);
        assert_eq!(to_stereo(&[0.1, 0.2, 0.3, 0.4], 2), vec![0.1, 0.2, 0.3, 0.4]);
        let folded = to_stereo(&[1.0, 1.0, 0.0, 0.0, 0.0, 0.0], 6);
        assert_eq!(folded.len(), 2);
        assert!(folded[0] > 0.0 && folded[0] <= 1.0);
    }

    #[test]
    fn a_shuffle_keeps_every_song_and_puts_the_playing_one_first() {
        for len in [1usize, 2, 5, 40] {
            for keep in [0, len / 2, len - 1] {
                let (order, at) = build_order(len, keep, true);
                let mut sorted = order.clone();
                sorted.sort_unstable();
                assert_eq!(sorted, (0..len).collect::<Vec<_>>());
                assert_eq!(order[at], keep);
            }
        }
        let (order, at) = build_order(4, 2, false);
        assert_eq!(order, vec![0, 1, 2, 3]);
        assert_eq!(at, 2);
    }

    #[test]
    fn the_position_carries_on_only_while_playing() {
        let mut state = DeckState {
            playback: DeckPlayback::Playing,
            track: Some(DeckTrack {
                duration_ms: 60_000,
                ..DeckTrack::default()
            }),
            position_ms: 1000,
            position_at: Some(Instant::now() - Duration::from_millis(500)),
            ..DeckState::default()
        };
        assert!(state.position_now() >= 1500);
        state.playback = DeckPlayback::Paused;
        assert_eq!(state.position_now(), 1000);
        state.playback = DeckPlayback::Stopped;
        assert!(!state.is_active());
    }

    #[test]
    fn a_wav_file_opens_plays_and_seeks() {
        let dir = std::env::temp_dir().join(format!("chanceify-deck-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a test folder");
        let path = dir.join("tone.wav");
        wav(&path, 22_050, 1, 1.0);

        let mut reader = Reader::open(&path).expect("the file opens");
        assert!((reader.duration_ms as i32 - 1000).abs() < 50);
        let mut frames = 0usize;
        while let Some(chunk) = reader.next().expect("it decodes") {
            assert_eq!(chunk.len() % 2, 0, "stereo");
            frames += chunk.len() / 2;
        }
        // One second at 22.05 kHz comes out as about one second at 44.1.
        assert!((frames as i64 - 44_100).abs() < 2_000, "frames {frames}");

        let landed = reader.seek(500).expect("it seeks");
        assert!((landed as i32 - 500).abs() < 100, "landed at {landed}");
        assert!(reader.next().expect("it decodes after a seek").is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_that_is_not_music_is_an_error_not_a_crash() {
        let dir = std::env::temp_dir().join(format!("chanceify-deck-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a test folder");
        let path = dir.join("notes.mp3");
        std::fs::write(&path, b"this is only text").expect("write");
        assert!(Reader::open(&path).is_err());
        assert!(Reader::open(&dir.join("missing.mp3")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
