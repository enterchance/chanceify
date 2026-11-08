//! Song files sent to the Recycle Bin from the Local songs page, and
//! brought back with Ctrl+Z.
//!
//! The page drops a song the moment it is sent away, and shows it again the
//! moment it is asked back; the file moves in the background, and only a
//! move that fails puts the list back the way it was.

use std::sync::{Arc, Mutex, PoisonError};

use super::*;
use crate::local_library::LocalSong;

/// Songs sent to the Recycle Bin together, kept for the rest of the session
/// so Ctrl+Z can bring them back.
pub struct TrashedSongs {
    pub songs: Vec<LocalSong>,
    /// When they went, to tell this apart from a newer playlist removal
    /// that Ctrl+Z should take back first.
    pub at: Instant,
}

/// What a background move found.
enum Finished {
    /// Sent away: the songs whose files are gone, and the ones still there
    /// with what went wrong.
    Recycled {
        gone: Vec<LocalSong>,
        kept: Vec<LocalSong>,
        error: Option<String>,
    },
    /// Asked back: the songs whose files are there again, and the ones
    /// that are not.
    Restored {
        back: Vec<LocalSong>,
        missing: Vec<LocalSong>,
        error: Option<String>,
    },
}

#[derive(Default)]
pub struct LocalTrash {
    /// Every batch sent to the Recycle Bin this session, the latest last.
    pub stack: Vec<TrashedSongs>,
    finished: Arc<Mutex<Vec<Finished>>>,
}

/// The name a song is shown by in a message.
fn song_name(song: &LocalSong) -> String {
    if song.title.trim().is_empty() {
        std::path::Path::new(&song.path)
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| song.path.clone())
    } else {
        song.title.clone()
    }
}

impl App {
    /// Takes these files off the Local songs page at once and moves them to
    /// the Recycle Bin in the background. Local playlists keep them, so they
    /// show as not found until Ctrl+Z brings them back.
    pub(super) fn trash_local_files(&mut self, ctx: &egui::Context, paths: Vec<String>) {
        self.dialog = None;
        let mut songs: Vec<LocalSong> = Vec::new();
        for path in paths {
            if songs.iter().any(|song| song.path == path) {
                continue;
            }
            match self.local_songs.songs.iter().position(|song| song.path == path) {
                Some(at) => songs.push(self.local_songs.songs.remove(at)),
                None => songs.push(LocalSong {
                    path,
                    ..LocalSong::default()
                }),
            }
        }
        if songs.is_empty() {
            return;
        }
        let finished = Arc::clone(&self.local_trash.finished);
        let wake = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("recycle".into())
            .spawn(move || {
                let files: Vec<PathBuf> = songs.iter().map(|song| PathBuf::from(&song.path)).collect();
                let result = crate::trash::recycle(&files);
                let (gone, kept): (Vec<LocalSong>, Vec<LocalSong>) = songs
                    .into_iter()
                    .partition(|song| !std::path::Path::new(&song.path).exists());
                let error = match result {
                    Err(error) => Some(error),
                    Ok(()) if !kept.is_empty() => Some("Windows kept the file".to_string()),
                    Ok(()) => None,
                };
                finished
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(Finished::Recycled { gone, kept, error });
                wake.request_repaint();
            });
        if spawned.is_err() {
            self.toast_error("Could not move the files to the Recycle Bin");
            // The songs went with the closure that never ran: read the
            // folders again to show them.
            self.start_music_scan();
        }
    }

    /// Brings back the latest songs sent to the Recycle Bin, if there are
    /// any. Answers whether there were.
    pub(super) fn restore_trashed_files(&mut self, ctx: &egui::Context) -> bool {
        let Some(batch) = self.local_trash.stack.pop() else {
            return false;
        };
        // Back on the page at once; the scan after the restore puts them in
        // their usual order.
        for song in &batch.songs {
            if song.size > 0
                && !self.local_songs.songs.iter().any(|known| known.path == song.path)
            {
                self.local_songs.songs.push(song.clone());
            }
        }
        let finished = Arc::clone(&self.local_trash.finished);
        let wake = ctx.clone();
        let songs = batch.songs;
        let spawned = std::thread::Builder::new()
            .name("unrecycle".into())
            .spawn(move || {
                let files: Vec<PathBuf> = songs.iter().map(|song| PathBuf::from(&song.path)).collect();
                let error = crate::trash::restore(&files).err();
                let (back, missing): (Vec<LocalSong>, Vec<LocalSong>) = songs
                    .into_iter()
                    .partition(|song| std::path::Path::new(&song.path).exists());
                finished
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(Finished::Restored {
                        back,
                        missing,
                        error,
                    });
                wake.request_repaint();
            });
        if spawned.is_err() {
            self.toast_error("Could not bring the files back from the Recycle Bin");
        }
        true
    }

    /// Takes what the background moves found, once each has finished.
    pub(super) fn poll_local_trash(&mut self) {
        let done: Vec<Finished> = std::mem::take(
            &mut *self
                .local_trash
                .finished
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        );
        for finished in done {
            match finished {
                Finished::Recycled { gone, kept, error } => {
                    if !kept.is_empty() {
                        let names: Vec<String> = kept.iter().map(song_name).collect();
                        for song in kept {
                            if song.size > 0
                                && !self.local_songs.songs.iter().any(|known| known.path == song.path)
                            {
                                self.local_songs.songs.push(song);
                            }
                        }
                        let error = error.unwrap_or_default();
                        self.toast_error(format!(
                            "Could not move {} to the Recycle Bin: {error}",
                            if names.len() == 1 {
                                format!("“{}”", names[0])
                            } else {
                                format!("{} songs", names.len())
                            }
                        ));
                    }
                    if !gone.is_empty() {
                        self.local_trash.stack.push(TrashedSongs {
                            songs: gone,
                            at: Instant::now(),
                        });
                        self.toast("Moved to the Recycle Bin. Ctrl+Z to undo.");
                        self.save_local_songs();
                    }
                }
                Finished::Restored {
                    back,
                    missing,
                    error,
                } => {
                    if !missing.is_empty() {
                        let names: Vec<String> = missing.iter().map(song_name).collect();
                        self.local_songs
                            .songs
                            .retain(|song| !missing.iter().any(|gone| gone.path == song.path));
                        // Still in the bin, as far as can be told: Ctrl+Z
                        // can try again.
                        self.local_trash.stack.push(TrashedSongs {
                            songs: missing,
                            at: Instant::now(),
                        });
                        let why = error.unwrap_or_else(|| "it was not found in the Recycle Bin".to_string());
                        self.toast_error(format!(
                            "Could not bring back {}: {why}",
                            if names.len() == 1 {
                                format!("“{}”", names[0])
                            } else {
                                format!("{} songs", names.len())
                            }
                        ));
                    }
                    if !back.is_empty() {
                        self.toast(if back.len() == 1 {
                            format!("Brought back “{}”", song_name(&back[0]))
                        } else {
                            format!("Brought back {} songs", back.len())
                        });
                        // Read the folders again, so the list is exactly
                        // what is on disk (the measurements are kept).
                        self.start_music_scan();
                    }
                }
            }
        }
    }

    /// Writes the list of song files to the index, in the background.
    fn save_local_songs(&self) {
        let library = self.local_songs.clone();
        let file = crate::local_library::library_file(&self.dirs.index_dir());
        let spawned = std::thread::Builder::new()
            .name("local-songs-save".into())
            .spawn(move || library.save(&file));
        if spawned.is_err() {
            log::warn!("could not save the list of song files");
        }
    }
}
