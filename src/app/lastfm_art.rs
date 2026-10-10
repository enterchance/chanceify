//! Saves the covers Last.fm lacks, ready to upload by hand.
//!
//! Every album and artist that plays is looked up on Last.fm (see
//! [`crate::lastfm_art`]). Its cover is kept in `lastfm-art/albums` or
//! `lastfm-art/artists` in the index folder, and copied to `albums-missing`
//! or `artists-missing` when Last.fm has only the placeholder.

use crate::lastfm_art::{self, Kind};

use super::*;

/// A cover on its way to the folders.
pub struct ArtExport {
    kind: Kind,
    key: String,
    file: String,
    url: Option<String>,
    artist_id: Option<String>,
    asked_artist: bool,
    missing: Option<bool>,
    saved_all: bool,
    started: Instant,
}

impl App {
    fn art_root(&self) -> PathBuf {
        self.dirs.index_dir().join("lastfm-art")
    }

    fn art_key(&self) -> String {
        let own = self.settings.lastfm_api_key.trim();
        if own.is_empty() || self.settings.lastfm_secret.trim().is_empty() {
            crate::lastfm::DEFAULT_API_KEY.to_string()
        } else {
            own.to_string()
        }
    }

    /// The Spotify artist's picture, once the artist is known.
    pub(super) fn note_artist_image(&mut self, artist: &crate::api::models::Artist) {
        let url = artist.images.first().map(|image| image.url.clone());
        for export in &mut self.art_exports {
            if export.kind == Kind::Artist
                && export.url.is_none()
                && export.artist_id.as_deref() == Some(artist.id.as_str())
            {
                export.url = url.clone();
            }
        }
    }

    /// Called every frame: queues what plays, collects answers, saves files.
    pub(super) fn drive_lastfm_art(&mut self, ctx: &egui::Context) {
        let api_key = self.art_key();
        if api_key.is_empty() || self.offline {
            return;
        }
        let playing = self
            .now_playing()
            .filter(|now| now.uri.starts_with("spotify:track:") && !now.title.is_empty());
        if let Some(now) = &playing
            && self.art_seen_for.as_deref() != Some(now.uri.as_str())
        {
            self.art_seen_for = Some(now.uri.clone());
            let artist = now
                .artists
                .first()
                .map(|artist| artist.name.clone())
                .unwrap_or_default();
            let artist_id = now.artists.first().and_then(|artist| artist.id.clone());
            let cover = now.art_url.clone().or_else(|| now.art_small.clone());
            let root = self.art_root();
            if !artist.is_empty() && !now.album_name.is_empty() {
                let key = lastfm_art::album_key(&artist, &now.album_name);
                let file = lastfm_art::file_name(Kind::Album, &artist, &now.album_name);
                self.queue_art(
                    &api_key,
                    Kind::Album,
                    key,
                    file,
                    cover,
                    None,
                    &artist,
                    &now.album_name,
                    &root,
                );
            }
            if !artist.is_empty() {
                let key = lastfm_art::artist_key(&artist);
                let file = lastfm_art::file_name(Kind::Artist, &artist, "");
                self.queue_art(&api_key, Kind::Artist, key, file, None, artist_id, &artist, "", &root);
            }
        }
        // The library sweep: one album (and its artist) at a time, never
        // more than a few waiting, so Last.fm is asked gently.
        if self.art_sweep && self.art_sweep_count as usize > self.library.albums.items.len() * 3 + 10 {
            // Last.fm is not answering; stop rather than ask forever.
            self.art_sweep = false;
            self.toast("Last.fm is not answering. Try the library check again later.");
        }
        if self.art_sweep && self.art_exports.len() < 4 {
            let root = self.art_root();
            let next = self.library.albums.items.iter().find_map(|saved| {
                let album = &saved.album;
                let artist = album.artists.first()?;
                if album.name.is_empty() || artist.name.is_empty() {
                    return None;
                }
                let key = lastfm_art::album_key(&artist.name, &album.name);
                (!self.art_asked.contains(&key)).then(|| {
                    (
                        artist.name.clone(),
                        artist.id.clone(),
                        album.name.clone(),
                        album.images.first().map(|image| image.url.clone()),
                    )
                })
            });
            match next {
                Some((artist, artist_id, album, cover)) => {
                    self.art_sweep_count += 1;
                    let key = lastfm_art::album_key(&artist, &album);
                    let file = lastfm_art::file_name(Kind::Album, &artist, &album);
                    self.queue_art(&api_key, Kind::Album, key, file, cover, None, &artist, &album, &root);
                    let key = lastfm_art::artist_key(&artist);
                    let file = lastfm_art::file_name(Kind::Artist, &artist, "");
                    self.queue_art(&api_key, Kind::Artist, key, file, None, artist_id, &artist, "", &root);
                }
                None => {
                    self.art_sweep = false;
                    let count = self.art_sweep_count;
                    self.toast(format!("Checked {count} albums from your library"));
                }
            }
            ctx.request_repaint_after(Duration::from_millis(400));
        }
        // Answers from Last.fm.
        let answers = self.art_checker.take();
        if !answers.is_empty() {
            for answer in answers {
                match answer.missing {
                    Some(missing) => {
                        self.art_store.checked.insert(answer.key.clone(), missing);
                        self.art_store
                            .at
                            .insert(answer.key.clone(), lastfm_art::now_secs());
                        if let Some((kind, file)) = self.art_names.get(&answer.key).cloned() {
                            let folder = match kind {
                                Kind::Album => "albums",
                                Kind::Artist => "artists",
                            };
                            let name = format!("{folder}/{file}");
                            if missing {
                                // Still missing after the check: it is
                                // listed again.
                                self.art_store.removed.remove(&name);
                            } else {
                                // Last.fm has a picture now: off the list.
                                let marker = self
                                    .art_root()
                                    .join(format!("{folder}-missing"))
                                    .join(format!("{file}.jpg"));
                                std::fs::remove_file(marker).ok();
                                self.art_store.removed.remove(&name);
                            }
                        }
                        for export in &mut self.art_exports {
                            if export.key == answer.key {
                                export.missing = Some(missing);
                            }
                        }
                    }
                    None => {
                        // Could not tell; ask again the next time it plays.
                        self.art_asked.remove(&answer.key);
                        self.art_exports.retain(|export| export.key != answer.key);
                    }
                }
            }
            self.art_store.save(&self.dirs.cache.join("lastfm_art.json"));
        }
        // Saving the pictures.
        let root = self.art_root();
        let mut exports = std::mem::take(&mut self.art_exports);
        exports.retain_mut(|export| self.step_art_export(ctx, export, &root));
        // Anything queued meanwhile is kept.
        exports.append(&mut self.art_exports);
        self.art_exports = exports;
        if !self.art_exports.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(700));
        }
        // The badge on the player bar.
        self.art_missing_now = playing.is_some_and(|now| {
            now.artists.first().is_some_and(|artist| {
                self.art_store
                    .checked
                    .get(&lastfm_art::album_key(&artist.name, &now.album_name))
                    == Some(&true)
            })
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn queue_art(
        &mut self,
        api_key: &str,
        kind: Kind,
        key: String,
        file: String,
        url: Option<String>,
        artist_id: Option<String>,
        artist: &str,
        album: &str,
        root: &std::path::Path,
    ) {
        if !self.art_asked.insert(key.clone()) {
            return;
        }
        let mut known = self.art_store.checked.get(&key).copied();
        // A picture Last.fm lacked is asked about again after a week, so
        // an upload (or anyone else's) clears it from the list.
        if known == Some(true) {
            let asked = self.art_store.at.get(&key).copied().unwrap_or(0);
            if lastfm_art::now_secs().saturating_sub(asked) > lastfm_art::RECHECK_AFTER {
                known = None;
            }
        }
        self.art_names.insert(key.clone(), (kind, file.clone()));
        if known.is_none() {
            self.art_checker.check(lastfm_art::Job {
                kind,
                key: key.clone(),
                artist: artist.to_string(),
                album: album.to_string(),
                api_key: api_key.to_string(),
            });
        }
        let folder = match kind {
            Kind::Album => "albums",
            Kind::Artist => "artists",
        };
        let all = root.join(folder).join(format!("{file}.jpg"));
        let missing_copy = root
            .join(format!("{folder}-missing"))
            .join(format!("{file}.jpg"));
        // Already saved on an earlier run, or taken off the list by the
        // reader: nothing more to do.
        let taken_off = self.art_store.removed.contains(&format!("{folder}/{file}"));
        if all.exists() && (known != Some(true) || missing_copy.exists() || taken_off) {
            return;
        }
        self.art_exports.push(ArtExport {
            kind,
            key,
            file,
            url,
            artist_id,
            asked_artist: false,
            missing: known,
            saved_all: all.exists(),
            started: Instant::now(),
        });
    }

    /// Takes a picture off the "missing" list after the reader added it to
    /// Last.fm. It stays off until a later check finds it still missing.
    pub(super) fn remove_missing_art(&mut self, artists: bool, file: &str) {
        let folder = if artists { "artists" } else { "albums" };
        // Only a plain file name from the list, never a path.
        if file.is_empty() || file.contains(['/', '\\']) {
            return;
        }
        let marker = self
            .art_root()
            .join(format!("{folder}-missing"))
            .join(format!("{file}.jpg"));
        std::fs::remove_file(marker).ok();
        self.art_store.removed.insert(format!("{folder}/{file}"));
        self.art_store.save(&self.dirs.cache.join("lastfm_art.json"));
    }

    /// One step for one picture; false once it is finished or given up.
    fn step_art_export(&mut self, ctx: &egui::Context, export: &mut ArtExport, root: &std::path::Path) -> bool {
        if export.started.elapsed() > Duration::from_secs(120) {
            return false;
        }
        if export.url.is_none() && export.kind == Kind::Artist {
            // Spotify's picture of the artist, from an open page or a lookup.
            if let Some(id) = export.artist_id.clone() {
                if let Some(page) = self.artist_pages.get(&id)
                    && let Some(artist) = page.artist.get()
                {
                    export.url = artist.images.first().map(|image| image.url.clone());
                }
                if export.url.is_none() && !export.asked_artist {
                    export.asked_artist = true;
                    self.backend.api(ApiRequest::Artist { id });
                }
            }
            if export.url.is_none() {
                return true;
            }
        }
        let Some(url) = export.url.clone() else {
            return false;
        };
        let art = self.backend.art();
        let Some(source) = art.cached_file(&url) else {
            art.prefetch(ctx, &url);
            return true;
        };
        let folder = match export.kind {
            Kind::Album => "albums",
            Kind::Artist => "artists",
        };
        if !export.saved_all {
            let target = root.join(folder).join(format!("{}.jpg", export.file));
            if copy_picture(&source, &target) {
                export.saved_all = true;
            } else {
                return false;
            }
        }
        match export.missing {
            Some(true) => {
                let target = root
                    .join(format!("{folder}-missing"))
                    .join(format!("{}.jpg", export.file));
                if !target.exists() && !copy_picture(&source, &target) {
                    return false;
                }
                false
            }
            Some(false) => false,
            // Last.fm has not answered yet.
            None => true,
        }
    }
}

fn copy_picture(source: &std::path::Path, target: &std::path::Path) -> bool {
    if let Some(parent) = target.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return false;
    }
    match std::fs::copy(source, target) {
        Ok(_) => true,
        Err(error) => {
            log::warn!("could not save {}: {error}", target.display());
            false
        }
    }
}
