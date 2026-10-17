//! Songs that are files on this computer.
//!
//! Put audio files (mp3, flac, ogg, wav, m4a) anywhere inside the
//! `songs` folder of the Chanceify index. A scan reads each file's tags,
//! measures its tempo, key and waveform (see [`crate::analysis`]) and keeps
//! the result in `index/local_songs.json`, so a file is only ever analysed
//! once: the next scan skips every file whose size and date are unchanged.
//!
//! The scan runs on its own thread; the window reads its progress from
//! [`Scan`] and never waits for it.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, MetadataRevision, StandardTagKey};
use symphonia::core::probe::Hint;

pub const EXTENSIONS: &[&str] = &["mp3", "flac", "ogg", "oga", "wav", "m4a", "aac", "mp4"];

/// The sample rate the analysis works at (see `analysis`).
const ANALYSIS_RATE: f32 = 11025.0;
/// Longest stretch of one file that is decoded, in seconds.
const MAX_SECONDS: f32 = 15.0 * 60.0;
/// Bumped when the analysis changes enough that old results should be redone.
const VERSION: u32 = 1;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LocalSong {
    pub path: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub modified: u64,
    pub title: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub album: String,
    #[serde(default)]
    pub genre: String,
    #[serde(default)]
    pub track_number: u32,
    #[serde(default)]
    pub year: String,
    #[serde(default)]
    pub duration_ms: u64,
    #[serde(default)]
    pub bpm: Option<f32>,
    #[serde(default)]
    pub bpm_confidence: f32,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub camelot: Option<String>,
    #[serde(default)]
    pub key_confidence: f32,
    #[serde(default)]
    pub waveform: Vec<u8>,
    /// Set when the file could not be read; shown instead of silently lost.
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Library {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub songs: Vec<LocalSong>,
}

impl Library {
    pub fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Library>(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        if let Ok(bytes) = serde_json::to_vec(self) {
            let temporary = path.with_extension("json.tmp");
            if std::fs::write(&temporary, bytes).is_ok() {
                std::fs::rename(&temporary, path).ok();
            }
        }
    }

    /// The file for a song by the same title and first artist, if any.
    pub fn find_match(&self, title: &str, artists: &str) -> Option<&LocalSong> {
        let wanted = crate::exportify::match_key(title, artists);
        self.songs
            .iter()
            .find(|song| crate::exportify::match_key(&song.title, &song.artist) == wanted)
    }
}

/// `index/songs`, where the audio files go.
pub fn songs_dir(index: &Path) -> PathBuf {
    index.join("songs")
}

pub fn library_file(index: &Path) -> PathBuf {
    index.join("local_songs.json")
}

/// Every audio file under `root`, in a stable order.
pub fn find_files(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
            } else if path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
            {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

#[derive(Default)]
struct Tags {
    title: String,
    artist: String,
    album: String,
    genre: String,
    year: String,
    track_number: u32,
}

impl Tags {
    fn read(&mut self, revision: &MetadataRevision) {
        for tag in revision.tags() {
            let value = tag.value.to_string();
            let value = value.trim();
            if value.is_empty() {
                continue;
            }
            match tag.std_key {
                Some(StandardTagKey::TrackTitle) if self.title.is_empty() => {
                    self.title = value.to_string();
                }
                Some(StandardTagKey::Artist) if self.artist.is_empty() => {
                    self.artist = value.to_string();
                }
                Some(StandardTagKey::AlbumArtist) if self.artist.is_empty() => {
                    self.artist = value.to_string();
                }
                Some(StandardTagKey::Album) if self.album.is_empty() => {
                    self.album = value.to_string();
                }
                Some(StandardTagKey::Genre) if self.genre.is_empty() => {
                    self.genre = value.to_string();
                }
                Some(StandardTagKey::Date | StandardTagKey::OriginalDate)
                    if self.year.is_empty() =>
                {
                    self.year = value.chars().take(4).collect();
                }
                Some(StandardTagKey::TrackNumber) if self.track_number == 0 => {
                    self.track_number = value
                        .split('/')
                        .next()
                        .and_then(|n| n.trim().parse().ok())
                        .unwrap_or(0);
                }
                _ => {}
            }
        }
    }
}

/// Reads one file: tags, then the whole sound, then the measurements.
pub fn read_song(path: &Path) -> LocalSong {
    let metadata = std::fs::metadata(path).ok();
    let mut song = LocalSong {
        path: path.to_string_lossy().to_string(),
        size: metadata.as_ref().map_or(0, |m| m.len()),
        modified: metadata
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs()),
        ..LocalSong::default()
    };
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    match decode(path) {
        Ok(decoded) => {
            let result = crate::analysis::analyze(&decoded.average, ANALYSIS_RATE_U32);
            song.title = if decoded.tags.title.is_empty() {
                stem
            } else {
                decoded.tags.title
            };
            song.artist = decoded.tags.artist;
            song.album = decoded.tags.album;
            song.genre = decoded.tags.genre;
            song.year = decoded.tags.year;
            song.track_number = decoded.tags.track_number;
            song.duration_ms = decoded.duration_ms;
            song.bpm = result.bpm;
            song.bpm_confidence = result.bpm_confidence;
            if let Some(key) = result.key {
                song.key = Some(key.name());
                song.camelot = Some(key.camelot());
                song.key_confidence = key.confidence;
            }
            // The waveform is drawn from the loudest sample in each slice,
            // not from the averaged samples the tempo and key work from.
            song.waveform = crate::analysis::waveform(&decoded.peaks);
        }
        Err(error) => {
            song.title = stem;
            song.error = Some(error);
        }
    }
    song
}

const ANALYSIS_RATE_U32: u32 = 11025;

struct Decoded {
    tags: Tags,
    /// Mono, averaged down to about 11 kHz.
    average: Vec<f32>,
    /// Loudest sample in each group that was averaged.
    peaks: Vec<f32>,
    duration_ms: u64,
}

fn decode(path: &Path) -> Result<Decoded, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
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
        .map_err(|e| format!("not a song chanceify can read ({e})"))?;
    let mut tags = Tags::default();
    if let Some(metadata) = probed.metadata.get() {
        if let Some(revision) = metadata.current() {
            tags.read(revision);
        }
    }
    let mut format = probed.format;
    {
        let metadata = format.metadata();
        if let Some(revision) = metadata.current() {
            tags.read(revision);
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
    let sample_rate = params
        .sample_rate
        .ok_or_else(|| "unknown sample rate".to_string())?;
    let mut decoder = symphonia::default::get_codecs()
        .make(&params, &DecoderOptions::default())
        .map_err(|e| format!("cannot decode this kind of file ({e})"))?;
    let factor = ((sample_rate as f32 / ANALYSIS_RATE).round() as usize).max(1);
    let limit_frames = (sample_rate as f32 * MAX_SECONDS) as u64;
    let mut average: Vec<f32> = Vec::new();
    let mut peaks: Vec<f32> = Vec::new();
    let (mut sum, mut peak, mut count) = (0.0f32, 0.0f32, 0usize);
    let mut frames_total: u64 = 0;
    let mut buffer: Option<SampleBuffer<f32>> = None;
    loop {
        if frames_total >= limit_frames {
            break;
        }
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(SymphoniaError::ResetRequired) => break,
            Err(error) => {
                if frames_total > 0 {
                    break;
                }
                return Err(error.to_string());
            }
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = *decoded.spec();
                let channels = spec.channels.count().max(1);
                let needed = decoded.frames() * channels;
                if buffer.as_ref().is_none_or(|b| b.capacity() < needed) {
                    buffer = Some(SampleBuffer::<f32>::new(decoded.capacity() as u64, spec));
                }
                let Some(buffer) = buffer.as_mut() else {
                    continue;
                };
                buffer.copy_interleaved_ref(decoded);
                for frame in buffer.samples().chunks(channels) {
                    let mono = frame.iter().sum::<f32>() / channels as f32;
                    frames_total += 1;
                    sum += mono;
                    peak = peak.max(mono.abs());
                    count += 1;
                    if count == factor {
                        average.push(sum / factor as f32);
                        peaks.push(peak);
                        sum = 0.0;
                        peak = 0.0;
                        count = 0;
                    }
                }
            }
            // One bad packet is skipped; the rest of the song is still there.
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(error) => {
                if frames_total > 0 {
                    break;
                }
                return Err(error.to_string());
            }
        }
    }
    if frames_total == 0 {
        return Err("no sound could be read from this file".into());
    }
    let duration_ms = params
        .n_frames
        .map(|frames| frames * 1000 / u64::from(sample_rate))
        .unwrap_or(frames_total * 1000 / u64::from(sample_rate));
    // `analysis::analyze` expects the rate it is told; the averaged samples
    // are at sample_rate / factor, which is close to 11025 but not exact.
    // Telling it 11025 would skew tempo by that ratio, so resample the
    // factor into the rate by correcting the lengths instead.
    let true_rate = sample_rate as f32 / factor as f32;
    let average = if (true_rate - ANALYSIS_RATE).abs() > 1.0 {
        retime(&average, true_rate, ANALYSIS_RATE)
    } else {
        average
    };
    Ok(Decoded {
        tags,
        average,
        peaks,
        duration_ms,
    })
}

/// Stretches or squeezes `samples` (recorded at `from` Hz) so they play at
/// `to` Hz with the same duration, by picking the nearest sample.
fn retime(samples: &[f32], from: f32, to: f32) -> Vec<f32> {
    if samples.is_empty() {
        return Vec::new();
    }
    let seconds = samples.len() as f32 / from;
    let n = (seconds * to) as usize;
    (0..n)
        .map(|i| {
            let source = ((i as f32 / to) * from) as usize;
            samples[source.min(samples.len() - 1)]
        })
        .collect()
}

// ---- the background scan ------------------------------------------------

#[derive(Clone, Debug, Default)]
pub struct ScanView {
    pub running: bool,
    pub done: usize,
    pub total: usize,
    pub current: String,
    pub failed: usize,
}

#[derive(Default)]
struct Shared {
    view: ScanView,
    finished: Option<Library>,
}

/// A scan in the background, and what the window can see of it.
#[derive(Clone, Default)]
pub struct Scan {
    shared: Arc<Mutex<Shared>>,
}

impl Scan {
    pub fn view(&self) -> ScanView {
        self.lock().view.clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The finished library, once, after a scan ends.
    pub fn take_finished(&self) -> Option<Library> {
        self.lock().finished.take()
    }

    /// Starts a scan of `index/songs` and every folder in `extra`, unless one
    /// is already running.
    pub fn start(&self, index: &Path, extra: &[PathBuf], existing: Library) {
        {
            let mut shared = self.lock();
            if shared.view.running {
                return;
            }
            shared.view = ScanView {
                running: true,
                ..ScanView::default()
            };
        }
        let shared = self.clone();
        let root = songs_dir(index);
        let extra: Vec<PathBuf> = extra.to_vec();
        let file = library_file(index);
        std::thread::spawn(move || {
            std::fs::create_dir_all(&root).ok();
            let mut files = find_files(&root);
            for folder in &extra {
                for found in find_files(folder) {
                    if !files.contains(&found) {
                        files.push(found);
                    }
                }
            }
            shared.lock().view.total = files.len();
            let mut songs: Vec<LocalSong> = Vec::with_capacity(files.len());
            let mut failed = 0;
            for (i, path) in files.iter().enumerate() {
                shared.lock().view.current = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let text = path.to_string_lossy();
                let known = existing.songs.iter().find(|s| s.path == text.as_ref());
                let unchanged = known.filter(|s| {
                    let meta = std::fs::metadata(path).ok();
                    existing.version == VERSION
                        && s.error.is_none()
                        && meta.as_ref().is_some_and(|m| m.len() == s.size)
                        && meta
                            .and_then(|m| m.modified().ok())
                            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                            .is_some_and(|d| d.as_secs() == s.modified)
                });
                let song = match unchanged {
                    Some(song) => song.clone(),
                    None => read_song(path),
                };
                if song.error.is_some() {
                    failed += 1;
                }
                songs.push(song);
                let mut guard = shared.lock();
                guard.view.done = i + 1;
                guard.view.failed = failed;
            }
            let library = Library {
                version: VERSION,
                songs,
            };
            library.save(&file);
            let mut guard = shared.lock();
            guard.view.running = false;
            guard.view.current.clear();
            guard.finished = Some(library);
        });
    }
}

// ---- the pictures in the files -------------------------------------------

/// Pictures bigger than this are not kept (the artwork loader refuses them).
const MAX_COVER_BYTES: usize = 8 * 1024 * 1024;

/// `index/local_covers_none.json`: the files known to hold no picture, so
/// they are not opened again each time the app starts.
fn no_cover_file(index: &Path) -> PathBuf {
    index.join("local_covers_none.json")
}

/// What a file's picture is known by: its path, size and date, so a file
/// that is changed (a picture added to it) is looked at again.
fn cover_key(path: &str, size: u64, modified: u64) -> String {
    use sha1::Digest;
    let digest = sha1::Sha1::digest(format!("{path}\0{size}\0{modified}").as_bytes());
    let mut key = String::with_capacity(16);
    for byte in digest.iter().take(8) {
        use std::fmt::Write;
        let _ = write!(key, "{byte:02x}");
    }
    key
}

/// The address a file's picture is shown under. The artwork loader holds
/// the bytes (see `ArtLoader::seed`) and keeps them in its disk cache, so
/// this is never fetched; the `.invalid` name makes sure of it.
fn cover_address(key: &str) -> String {
    format!("https://local.chanceify.invalid/song-cover/{key}")
}

#[derive(Default)]
struct CoversShared {
    /// By key: the address of the picture, or `None` for a file with none.
    known: std::collections::HashMap<String, Option<String>>,
    /// Files waiting to be looked at; the newest asked for goes first, so
    /// the rows on screen fill in before the ones scrolled past.
    queue: Vec<(String, PathBuf)>,
    queued: std::collections::HashSet<String>,
    running: bool,
    /// Keys of files with no picture, as saved in [`no_cover_file`].
    none: Option<std::collections::HashSet<String>>,
}

/// The pictures inside the song files, read in the background the first
/// time a row wants one, and handed to the artwork loader so they are drawn
/// like any other cover. The window never waits for a file.
#[derive(Clone, Default)]
pub struct Covers {
    shared: Arc<Mutex<CoversShared>>,
}

impl Covers {
    fn lock(&self) -> std::sync::MutexGuard<'_, CoversShared> {
        self.shared.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The address of the picture in the file at `path`: `Some(Some(_))`
    /// once it is ready, `Some(None)` when the file has none, and `None`
    /// while it is still being looked at (it is asked for here).
    pub fn lookup(
        &self,
        ctx: &egui::Context,
        art: &crate::images::ArtLoader,
        index: &Path,
        path: &str,
        size: u64,
        modified: u64,
    ) -> Option<Option<String>> {
        let key = cover_key(path, size, modified);
        let mut shared = self.lock();
        if let Some(known) = shared.known.get(&key) {
            return Some(known.clone());
        }
        if shared.queued.insert(key.clone()) {
            shared.queue.push((key, PathBuf::from(path)));
        }
        if !shared.running {
            shared.running = true;
            drop(shared);
            self.start(ctx.clone(), art.clone(), index.to_path_buf());
        }
        None
    }

    fn start(&self, ctx: egui::Context, art: crate::images::ArtLoader, index: PathBuf) {
        let covers = self.clone();
        let spawned = std::thread::Builder::new()
            .name("local-covers".into())
            .spawn(move || covers.work(&ctx, &art, &index));
        if spawned.is_err() {
            self.lock().running = false;
        }
    }

    fn work(&self, ctx: &egui::Context, art: &crate::images::ArtLoader, index: &Path) {
        let file = no_cover_file(index);
        if self.lock().none.is_none() {
            let none: std::collections::HashSet<String> = std::fs::read(&file)
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .unwrap_or_default();
            self.lock().none = Some(none);
        }
        let mut found_none = false;
        loop {
            let job = {
                let mut shared = self.lock();
                match shared.queue.pop() {
                    Some(job) => job,
                    None => {
                        shared.running = false;
                        break;
                    }
                }
            };
            let (key, path) = job;
            let known_none = self
                .lock()
                .none
                .as_ref()
                .is_some_and(|none| none.contains(&key));
            let address = cover_address(&key);
            let result = if known_none {
                None
            } else if art.cached_file(&address).is_some() {
                // Read on an earlier day: the loader has it on disk.
                Some(address)
            } else {
                match crate::file_deck::read_cover(&path)
                    .filter(|bytes| bytes.len() <= MAX_COVER_BYTES)
                {
                    Some(bytes) => {
                        art.seed(&address, bytes);
                        Some(address)
                    }
                    None => {
                        found_none = true;
                        if let Some(none) = self.lock().none.as_mut() {
                            none.insert(key.clone());
                        }
                        None
                    }
                }
            };
            {
                let mut shared = self.lock();
                shared.queued.remove(&key);
                shared.known.insert(key, result);
            }
            ctx.request_repaint();
        }
        if found_none {
            let none: Vec<String> = self
                .lock()
                .none
                .as_ref()
                .map(|none| none.iter().cloned().collect())
                .unwrap_or_default();
            if let Ok(bytes) = serde_json::to_vec(&none) {
                let temporary = file.with_extension("json.tmp");
                if std::fs::write(&temporary, bytes).is_ok() {
                    std::fs::rename(&temporary, &file).ok();
                }
            }
        }
    }
}

/// A plain text report of everything measured, to check it against what
/// another player shows for the same songs.
pub fn report(library: &Library) -> String {
    let mut out = String::from(
        "Title,Artist,Album,Genre,BPM,BPM confidence,Key,Camelot,Key confidence,Length (s),File\n",
    );
    let esc = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
    for song in &library.songs {
        out.push_str(&format!(
            "{},{},{},{},{},{:.2},{},{},{:.2},{},{}\n",
            esc(&song.title),
            esc(&song.artist),
            esc(&song.album),
            esc(&song.genre),
            song.bpm.map_or(String::new(), |b| format!("{b:.1}")),
            song.bpm_confidence,
            song.key.clone().unwrap_or_default(),
            song.camelot.clone().unwrap_or_default(),
            song.key_confidence,
            song.duration_ms / 1000,
            esc(&song.path),
        ));
    }
    out
}
