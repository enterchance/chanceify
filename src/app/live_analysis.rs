//! Key, tempo and shape of songs from Spotify, measured from the sound while
//! they play. Spotify no longer gives these to new apps, and nothing leaves
//! this computer: the audio that is playing is thinned, measured once the song
//! has played, and only the answers (a key, a tempo, a few hundred numbers
//! for the shape) are kept, in the index folder.

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender, channel};

use serde::{Deserialize, Serialize};

use super::*;

/// Enough of a song to call its key and tempo.
const EARLY_SECONDS: f32 = 75.0;
/// The least that is worth measuring when a song is left early.
const LEAST_SECONDS: f32 = 45.0;
/// The part of a song that must have played, unbroken, for its shape.
const WHOLE: f32 = 0.9;

/// What was measured for one song.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Measured {
    pub bpm: Option<f32>,
    /// "8A" and "A minor".
    pub camelot: Option<String>,
    pub key: Option<String>,
    /// Peaks of the whole song, 0 to 255; empty when it was not heard through.
    #[serde(default)]
    pub waveform: Vec<u8>,
}

struct Job {
    uri: String,
    isrc: Option<String>,
    result: Measured,
}

pub struct LiveAnalysis {
    results: HashMap<String, Measured>,
    loaded: bool,
    dirty: bool,
    saved_at: Option<Instant>,
    current: Option<String>,
    isrc: Option<String>,
    duration_ms: u32,
    last_position: u32,
    broken: bool,
    early_done: bool,
    tx: Sender<Job>,
    rx: Receiver<Job>,
}

impl Default for LiveAnalysis {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self {
            results: HashMap::new(),
            loaded: false,
            dirty: false,
            saved_at: None,
            current: None,
            isrc: None,
            duration_ms: 0,
            last_position: 0,
            broken: false,
            early_done: false,
            tx,
            rx,
        }
    }
}

impl LiveAnalysis {
    pub fn get(&self, uri: &str) -> Option<&Measured> {
        self.results.get(uri)
    }
}

fn measure(samples: &[f32], rate: u32, with_shape: bool) -> Measured {
    let analysis = crate::analysis::analyze(samples, rate);
    Measured {
        bpm: analysis.bpm.filter(|_| analysis.bpm_confidence > 0.0),
        camelot: analysis.key.map(|key| key.camelot()),
        key: analysis.key.map(|key| key.name()),
        waveform: if with_shape { analysis.waveform } else { Vec::new() },
    }
}

impl App {
    fn live_file(&self) -> std::path::PathBuf {
        self.dirs.cache.join("live_analysis.json")
    }

    /// The key a Spotify song was measured to be in, such as "8A".
    pub fn live_key_for(&self, item: &PlayableItem) -> Option<String> {
        let PlayableItem::Track(track) = item else {
            return None;
        };
        self.live.get(&track.uri)?.camelot.clone()
    }

    /// The measured tempo of a song with none from Deezer.
    pub fn live_bpm_for(&self, item: &PlayableItem) -> Option<f32> {
        let PlayableItem::Track(track) = item else {
            return None;
        };
        self.live.get(&track.uri)?.bpm
    }

    /// Called every pass: collects the song that is playing, measures it as
    /// soon as there is enough of it, and files the answers.
    pub(super) fn drive_live_analysis(&mut self) {
        if !self.live.loaded {
            self.live.loaded = true;
            if let Ok(text) = std::fs::read_to_string(self.live_file())
                && let Ok(map) = serde_json::from_str::<HashMap<String, Measured>>(&text)
            {
                self.live.results = map;
            }
        }
        // Answers that came back from the measuring thread.
        while let Ok(job) = self.live.rx.try_recv() {
            // A tempo from the sound fills the tempo column for a song
            // Deezer had nothing for.
            if let (Some(isrc), Some(bpm)) = (job.isrc.clone(), job.result.bpm)
                && !matches!(self.bpm_store.get(&isrc), Some(Some(_)))
            {
                self.bpm_store.remember(&isrc, Some(bpm));
                self.bpm_dirty = true;
                self.bpms.insert(isrc, Some(bpm));
            }
            let entry = self.live.results.entry(job.uri).or_default();
            if job.result.bpm.is_some() {
                entry.bpm = job.result.bpm;
            }
            if job.result.camelot.is_some() {
                entry.camelot = job.result.camelot;
                entry.key = job.result.key;
            }
            if !job.result.waveform.is_empty() {
                entry.waveform = job.result.waveform;
            }
            self.live.dirty = true;
            self.bpms_revision = self.bpms_revision.wrapping_add(1);
        }
        if self.live.dirty
            && self
                .live
                .saved_at
                .is_none_or(|at| at.elapsed() > Duration::from_secs(10))
        {
            self.save_live_analysis();
        }

        let now = self.now_playing();
        let song = now.as_ref().filter(|now| {
            !now.is_episode && !now.resuming && !now.uri.starts_with(crate::file_deck::URI_PREFIX)
        });
        let uri = song.map(|now| now.uri.clone());
        if uri != self.live.current {
            // The song changed: measure what was heard of the last one.
            self.finish_live_song();
            self.live.current = uri.clone();
            self.live.broken = false;
            self.live.early_done = false;
            self.live.last_position = song.map_or(0, |now| now.position_ms);
            self.live.duration_ms = song.map_or(0, |now| now.duration_ms);
            self.live.isrc = match self.now_playing_item() {
                Some(PlayableItem::Track(track)) => track
                    .external_ids
                    .isrc
                    .as_ref()
                    .map(|isrc| isrc.trim().to_ascii_uppercase())
                    .filter(|isrc| !isrc.is_empty()),
                _ => None,
            };
            match &uri {
                Some(uri) if self.live.results.get(uri).is_none_or(|m| m.camelot.is_none()) => {
                    self.winamp.tap.capture_start();
                }
                _ => {
                    let _ = self.winamp.tap.capture_take();
                }
            }
            return;
        }
        let (Some(song), Some(uri)) = (song, uri) else {
            return;
        };
        // A jump in the song breaks the whole-song shape (the key is fine).
        let position = song.position_ms;
        if song.playing
            && (position + 1500 < self.live.last_position
                || position > self.live.last_position.saturating_add(6000))
        {
            self.live.broken = true;
        }
        self.live.last_position = position;
        if !self.live.early_done
            && self.winamp.tap.capture_seconds() >= EARLY_SECONDS
            && self.live.results.get(&uri).is_none_or(|m| m.camelot.is_none())
        {
            self.live.early_done = true;
            let (samples, rate) = self.winamp.tap.capture_peek();
            self.spawn_measure(uri, samples, rate, false);
        }
    }

    /// Measures what was collected of the song that just ended.
    fn finish_live_song(&mut self) {
        let Some(uri) = self.live.current.clone() else {
            return;
        };
        let (samples, rate) = self.winamp.tap.capture_take();
        let seconds = samples.len() as f32 / rate.max(1) as f32;
        if seconds < LEAST_SECONDS {
            return;
        }
        let whole = !self.live.broken
            && self.live.duration_ms > 0
            && seconds >= WHOLE * self.live.duration_ms as f32 / 1000.0;
        let had_key = self.live.results.get(&uri).is_some_and(|m| m.camelot.is_some());
        if had_key && !whole {
            return;
        }
        self.spawn_measure(uri, samples, rate, whole);
    }

    fn spawn_measure(&mut self, uri: String, samples: Vec<f32>, rate: u32, whole: bool) {
        let tx = self.live.tx.clone();
        let isrc = self.live.isrc.clone();
        let spawned = std::thread::Builder::new()
            .name("measure-song".into())
            .spawn(move || {
                // A mistake in the measuring costs this one song's answers.
                let result = std::panic::catch_unwind(|| measure(&samples, rate, whole));
                if let Ok(result) = result {
                    let _ = tx.send(Job { uri, isrc, result });
                }
            });
        if let Err(error) = spawned {
            log::warn!("could not start measuring a song: {error}");
        }
    }

    pub(super) fn save_live_analysis(&mut self) {
        self.live.dirty = false;
        self.live.saved_at = Some(Instant::now());
        if let Ok(text) = serde_json::to_string(&self.live.results) {
            let path = self.live_file();
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let temporary = path.with_extension("tmp");
            if std::fs::write(&temporary, text).is_ok() {
                let _ = std::fs::rename(&temporary, &path);
            }
        }
    }
}
