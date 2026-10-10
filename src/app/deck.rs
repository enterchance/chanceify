//! Songs that are files: starting them from the Local songs page, showing
//! them in the player bar like any other song, and keeping the controls
//! (play, skip, seek, volume, shuffle, repeat) pointed at them while they
//! are on.

use std::sync::{Arc, PoisonError};

use super::*;
use crate::file_deck::{self, DeckConfig, DeckEntry, DeckPlayback, DeckState, FileDeck};

/// The address a file's cover is shown under (see `ArtLoader::seed`).
pub fn cover_url(uri: &str) -> String {
    format!(
        "https://local.chanceify.invalid/cover/{}",
        uri.trim_start_matches(file_deck::URI_PREFIX)
    )
}

impl App {
    /// How strong the bass of what is playing is right now, 0 to 1 (0 when
    /// nothing is playing). Read from the same tap the visualizers use.
    pub fn music_bass_level(&self) -> f32 {
        self.music_bass_level_raw().max(self.jump_level())
    }

    /// The press of the bass-jump key: 1 at the press, falling to 0 in a
    /// quarter of a second.
    pub fn jump_level(&self) -> f32 {
        self.jump_at.map_or(0.0, |at| {
            (1.0 - at.elapsed().as_secs_f32() / 0.28).clamp(0.0, 1.0)
        })
    }

    /// The screen jumps by itself when the music does what the visualizer
    /// settings ask: a rise in loudness, in the bass, or a beat.
    pub fn auto_jump(&mut self) {
        let mode = self.settings.vis_jump_react.min(2);
        if !self.settings.vis_jump_on || !self.now_playing().is_some_and(|now| now.playing && now.local) {
            return;
        }
        let energy = if mode == 1 {
            self.music_bass_level_raw()
        } else {
            let window = self.winamp.tap.window(1536, 0);
            if window.is_empty() {
                return;
            }
            (window.iter().map(|x| x * x).sum::<f32>() / window.len() as f32).sqrt() * 2.0
        };
        if !energy.is_finite() {
            return;
        }
        let ratio = match mode {
            0 => 1.3,
            1 => 1.2,
            _ => 1.15,
        };
        let rested = self.jump_at.is_none_or(|at| at.elapsed().as_secs_f32() > 0.32);
        if rested && energy > 0.03 && energy > self.jump_avg * ratio + 0.01 {
            self.jump_at = Some(Instant::now());
        }
        self.jump_avg += (energy - self.jump_avg) * 0.05;
    }

    fn music_bass_level_raw(&self) -> f32 {
        if !self.now_playing().is_some_and(|now| now.playing) {
            return 0.0;
        }
        let window = self.winamp.tap.window(1536, 0);
        if window.is_empty() {
            return 0.0;
        }
        // A one-pole low-pass keeps roughly what is under 150 Hz.
        let mut low = 0.0_f32;
        let mut sum = 0.0_f32;
        for sample in &window {
            low += (sample - low) * 0.02;
            sum += low * low;
        }
        let level = (sum / window.len() as f32).sqrt() * 5.0;
        if level.is_finite() { level.clamp(0.0, 1.0) } else { 0.0 }
    }

    /// The deck's snapshot while a song from a file is on (playing or paused).
    pub(super) fn deck_state(&self) -> Option<DeckState> {
        let state = self.deck.as_ref()?.state();
        state.is_active().then_some(state)
    }

    pub fn deck_active(&self) -> bool {
        self.deck_state().is_some()
    }

    /// The file being played, if one is.
    pub fn deck_current_path(&self) -> Option<PathBuf> {
        self.deck_state()
            .and_then(|state| state.track.map(|track| track.path))
    }

    /// Whether the song from a file that is on is playing, not paused.
    pub fn deck_playing(&self) -> bool {
        self.deck_state()
            .is_some_and(|state| state.playback == DeckPlayback::Playing)
    }

    /// Runs `command` on the deck if a song from a file is on. Returns whether
    /// it did, so the caller knows not to send the command to Spotify too.
    pub(super) fn deck_control(&self, command: impl FnOnce(&FileDeck)) -> bool {
        if !self.deck_active() {
            return false;
        }
        match &self.deck {
            Some(deck) => {
                command(deck);
                true
            }
            None => false,
        }
    }

    /// The song from a file as the interface sees any playing song.
    pub(super) fn deck_now_playing(&self) -> Option<NowPlaying> {
        let state = self.deck_state()?;
        let track = state.track.as_ref()?;
        // The cover is offered once it has been handed to the art loader.
        let art = (track.cover.is_some() && self.deck_art.as_deref() == Some(track.uri.as_str()))
            .then(|| cover_url(&track.uri));
        let artists = if track.artist.is_empty() {
            Vec::new()
        } else {
            vec![ArtistRef {
                name: track.artist.clone(),
                ..ArtistRef::default()
            }]
        };
        Some(NowPlaying {
            local: true,
            device_name: None,
            uri: track.uri.clone(),
            id: None,
            title: track.title.clone(),
            artists,
            subtitle: track.artist.clone(),
            album_name: track.album.clone(),
            album_id: None,
            show_id: None,
            art_url: art.clone(),
            art_small: art,
            duration_ms: track.duration_ms,
            position_ms: state.position_now(),
            playing: state.playback == DeckPlayback::Playing,
            loading: false,
            shuffle: state.shuffle,
            repeat: state.repeat,
            volume_percent: volume_to_percent(state.volume),
            can_control: true,
            can_set_volume: true,
            is_episode: false,
            resuming: false,
        })
    }

    fn deck_config(&self) -> DeckConfig {
        DeckConfig {
            device: self
                .settings
                .audio_device
                .clone()
                .filter(|device| !device.trim().is_empty()),
            buffer_ms: self.settings.audio_buffer_ms,
            tap: Arc::clone(&self.winamp.tap),
            eq: Arc::clone(&self.winamp.eq),
            speed: Arc::clone(&self.winamp.speed),
            crossfade_ms: u32::from(self.settings.crossfade_secs) * 1000,
            volume: self.settings.volume,
        }
    }

    /// Plays these files in this order, starting with `index`. `shuffle`
    /// sets shuffle first; `None` leaves it as it is.
    pub(super) fn play_local_files(
        &mut self,
        ctx: &egui::Context,
        paths: Vec<String>,
        index: usize,
        shuffle: Option<bool>,
    ) {
        if paths.is_empty() {
            return;
        }
        // Spotify steps aside: two songs at once is never what was asked.
        if self.local.playback == Playback::Playing {
            self.backend.player(PlayerCommand::Toggle);
        }
        self.optimistic_playing = None;
        self.intent_track = None;
        let entries: Vec<DeckEntry> = paths
            .iter()
            .map(|path| {
                match self
                    .local_songs
                    .songs
                    .iter()
                    .find(|song| &song.path == path)
                {
                    Some(song) => DeckEntry {
                        path: PathBuf::from(path),
                        title: song.title.clone(),
                        artist: song.artist.clone(),
                        album: song.album.clone(),
                        duration_ms: song.duration_ms.min(u64::from(u32::MAX)) as u32,
                    },
                    None => DeckEntry {
                        path: PathBuf::from(path),
                        ..DeckEntry::default()
                    },
                }
            })
            .collect();
        let config = self.deck_config();
        if self.deck.is_none() {
            let wake = ctx.clone();
            self.deck = Some(FileDeck::new(
                config.clone(),
                Arc::new(move || wake.request_repaint()),
            ));
        }
        let index = index.min(entries.len() - 1);
        if let Some(deck) = &self.deck {
            if let Some(shuffle) = shuffle {
                deck.set_shuffle(shuffle);
            }
            deck.play(config, entries, index);
        }
        self.deck_art = None;
    }

    /// Stops the song from a file, if one is on. Anything that starts a
    /// Spotify song calls this first.
    pub(super) fn deck_stop(&mut self) {
        if self.deck_active()
            && let Some(deck) = &self.deck
        {
            deck.stop();
        }
    }

    /// Each pass: hand a new song's cover to the art loader, and show what
    /// the deck wants said.
    pub(super) fn poll_deck(&mut self) {
        let Some(deck) = &self.deck else {
            return;
        };
        let error = deck.take_error();
        let state = deck.state();
        if let Some(track) = &state.track
            && state.is_active()
            && self.deck_art.as_deref() != Some(track.uri.as_str())
        {
            let url = cover_url(&track.uri);
            if let Some(cover) = &track.cover {
                self.backend.art().seed(&url, Arc::clone(cover));
                self.tint_for(Some(&url));
            }
            // A file that is also on Spotify shows its heart.
            let matched = self
                .local_matches
                .uri_of(&track.path.to_string_lossy())
                .map(str::to_string);
            self.deck_art = Some(track.uri.clone());
            if let Some(uri) = matched {
                self.request_contains(vec![uri]);
            }
            // A song from a file changes without the Spotify bookkeeping
            // that normally asks for lyrics, so ask here.
            if self.show_lyrics_panel || (self.fullscreen_vis && self.settings.vis_lyrics) {
                self.request_lyrics();
            }
        }
        if let Some(error) = error {
            self.toast_error(error);
        }
    }

    /// Opens the system's folder chooser off the window's thread and adds the
    /// folder it returns to the music folders.
    pub(super) fn pick_music_folder(&mut self, ctx: &egui::Context) {
        if self.folder_pick.lock().unwrap_or_else(PoisonError::into_inner).is_some() {
            return;
        }
        let slot = Arc::clone(&self.folder_pick);
        let wake = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("folder-pick".into())
            .spawn(move || {
                let chosen = rfd::FileDialog::new()
                    .set_title("Choose a folder of music")
                    .pick_folder();
                *slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(chosen);
                wake.request_repaint();
            });
        if spawned.is_err() {
            self.toast_error("Could not open the folder chooser");
        }
    }

    /// Opens the file chooser off the window's thread and copies the songs it
    /// returns into the song index (the songs folder of the index).
    pub(super) fn pick_music_files(&mut self, ctx: &egui::Context) {
        if self.files_pick.lock().unwrap_or_else(PoisonError::into_inner).is_some() {
            return;
        }
        let slot = Arc::clone(&self.files_pick);
        let wake = ctx.clone();
        let target = crate::local_library::songs_dir(&self.dirs.index_dir());
        let spawned = std::thread::Builder::new()
            .name("files-pick".into())
            .spawn(move || {
                let chosen = rfd::FileDialog::new()
                    .set_title("Choose songs to add to the song index")
                    .add_filter("Music", &["mp3", "flac", "ogg", "wav", "m4a", "aac", "opus"])
                    .pick_files()
                    .unwrap_or_default();
                let mut copied = 0usize;
                let mut failed = 0usize;
                if std::fs::create_dir_all(&target).is_err() {
                    failed = chosen.len();
                } else {
                    for file in chosen {
                        let Some(name) = file.file_name().map(|name| name.to_os_string()) else {
                            failed += 1;
                            continue;
                        };
                        // Never overwrite a song that is already there.
                        let mut destination = target.join(&name);
                        let mut number = 2;
                        while destination.exists() {
                            let stem = std::path::Path::new(&name)
                                .file_stem()
                                .map(|stem| stem.to_string_lossy().to_string())
                                .unwrap_or_default();
                            let extension = std::path::Path::new(&name)
                                .extension()
                                .map(|ext| format!(".{}", ext.to_string_lossy()))
                                .unwrap_or_default();
                            destination = target.join(format!("{stem} ({number}){extension}"));
                            number += 1;
                        }
                        match std::fs::copy(&file, &destination) {
                            Ok(_) => copied += 1,
                            Err(_) => failed += 1,
                        }
                    }
                }
                *slot.lock().unwrap_or_else(PoisonError::into_inner) = Some((copied, failed));
                wake.request_repaint();
            });
        if spawned.is_err() {
            self.toast_error("Could not open the file chooser");
        }
    }

    /// Takes the answer of the file chooser, if it has one.
    pub(super) fn poll_files_pick(&mut self) {
        let answer = self
            .files_pick
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        let Some((copied, failed)) = answer else {
            return;
        };
        if copied > 0 {
            self.toast(format!("Added {copied} song(s) to the song index"));
            self.start_music_scan();
        }
        if failed > 0 {
            self.toast_error(format!("{failed} file(s) could not be copied"));
        }
    }

    /// Takes the folder the chooser returned, if it has.
    pub(super) fn poll_folder_pick(&mut self) {
        let answer = self
            .folder_pick
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        let Some(answer) = answer else {
            return;
        };
        let Some(folder) = answer else {
            return;
        };
        let text = folder.to_string_lossy().to_string();
        if !self.settings.music_folders.contains(&text) {
            self.settings.music_folders.push(text);
            self.mark_settings_dirty();
        }
        self.start_music_scan();
    }

    /// Reads the songs folder and every music folder, in the background.
    pub fn start_music_scan(&mut self) {
        let index = self.dirs.index_dir();
        let extra: Vec<PathBuf> = self
            .settings
            .music_folders
            .iter()
            .map(PathBuf::from)
            .collect();
        std::fs::create_dir_all(crate::local_library::songs_dir(&index)).ok();
        self.local_scan
            .start(&index, &extra, self.local_songs.clone());
    }
}
