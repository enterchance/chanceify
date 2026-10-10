//! Music files and the rest of the world: which are also on Spotify, and what
//! genre they are (MusicBrainz, by artist and title).

use std::collections::VecDeque;

use super::*;
use crate::api::models::Track;
use crate::local_match;

/// The least time between two Spotify look-ups for files.
const MATCH_GAP: Duration = Duration::from_millis(1500);
/// A look-up with no answer after this long is given up on.
const MATCH_WAIT: Duration = Duration::from_secs(30);
/// How long to pause when Spotify asks for it.
const MATCH_BACKOFF: Duration = Duration::from_secs(60);
/// The answers are written to disk this often while a run goes on.
const MATCH_SAVE_EVERY: usize = 10;
/// How many genre questions about files may be out at once.
const LOCAL_GENRES_IN_FLIGHT: usize = 24;
/// The least time between two looks for files that need a genre.
const LOCAL_GENRE_GAP: Duration = Duration::from_secs(2);

/// The state of the background run that looks files up on Spotify, and of
/// the genres shown for files.
#[derive(Default)]
pub struct LocalSync {
    queue: VecDeque<String>,
    inflight: Option<String>,
    sent: Option<Instant>,
    next: Option<Instant>,
    total: usize,
    done: usize,
    found: usize,
    unsaved: usize,
    genres_next: Option<Instant>,
    saved_next: Option<Instant>,
    /// What each file with no genre of its own shows, by path.
    genres: HashMap<String, String>,
    genre_stamp: Option<(u64, usize, usize)>,
}

impl LocalSync {
    /// `(done, total)` while a run is on.
    pub fn progress(&self) -> Option<(usize, usize)> {
        (self.inflight.is_some() || !self.queue.is_empty()).then_some((self.done, self.total))
    }

    /// The genre text for a file that has none of its own.
    pub fn genre_of(&self, path: &str) -> &str {
        self.genres.get(path).map_or("", String::as_str)
    }

    /// Forget what was worked out; the next look rebuilds it.
    pub fn forget_genres(&mut self) {
        self.genre_stamp = None;
    }
}

impl App {
    /// The Spotify song a file that is playing is, if it has been matched.
    pub fn spotify_uri_for_playing_file(&self, file_uri: &str) -> Option<String> {
        let state = self.deck_state()?;
        let track = state.track?;
        if track.uri != file_uri {
            return None;
        }
        let path = track.path.to_string_lossy().to_string();
        self.local_matches.uri_of(&path).map(str::to_string)
    }

    /// Whether a Spotify look-up of files can be asked for now.
    pub fn can_match_files(&self) -> bool {
        self.user.is_some()
    }

    /// Starts looking every file that has not been looked up on Spotify.
    pub(super) fn queue_spotify_matches(&mut self) {
        if !self.can_match_files() {
            self.toast("Sign in to Spotify first, then try again");
            return;
        }
        let queued: HashSet<String> = self.local_sync.queue.iter().cloned().collect();
        let mut added = 0usize;
        for song in &self.local_songs.songs {
            if song.error.is_some()
                || song.title.trim().is_empty()
                || song.artist.trim().is_empty()
                || !self.local_matches.needs(&song.path)
                || queued.contains(&song.path)
                || self.local_sync.inflight.as_deref() == Some(song.path.as_str())
            {
                continue;
            }
            self.local_sync.queue.push_back(song.path.clone());
            added += 1;
        }
        if added == 0 {
            self.toast("Every song with a title and artist has already been looked up");
            return;
        }
        if self.local_sync.progress().is_none() || self.local_sync.total == 0 {
            self.local_sync.done = 0;
            self.local_sync.found = 0;
            self.local_sync.total = added;
        } else {
            self.local_sync.total += added;
        }
        self.local_sync.next = None;
        self.toast(format!(
            "Looking {added} songs up on Spotify, a few every second, in the background"
        ));
    }

    /// Each pass: send the next file's look-up when it is time.
    pub(super) fn drive_matches(&mut self) {
        if self
            .local_sync
            .sent
            .is_some_and(|sent| sent.elapsed() > MATCH_WAIT)
        {
            // No answer came; drop this one and carry on.
            self.local_sync.inflight = None;
            self.local_sync.sent = None;
            self.local_sync.done += 1;
        }
        if self.local_sync.inflight.is_some() || self.local_sync.queue.is_empty() {
            return;
        }
        if self
            .local_sync
            .next
            .is_some_and(|next| Instant::now() < next)
        {
            return;
        }
        if !self.can_match_files() {
            self.local_sync.queue.clear();
            return;
        }
        let Some(path) = self.local_sync.queue.pop_front() else {
            return;
        };
        let query = self
            .local_songs
            .songs
            .iter()
            .find(|song| song.path == path)
            .map(|song| local_match::query_for(&song.title, &song.artist));
        let Some(query) = query else {
            self.local_sync.done += 1;
            return;
        };
        self.local_sync.inflight = Some(path.clone());
        self.local_sync.sent = Some(Instant::now());
        self.local_sync.next = Some(Instant::now() + MATCH_GAP);
        self.backend.api(ApiRequest::MatchLocal { path, query });
    }

    /// Spotify's answer to a file's look-up.
    pub(super) fn local_match_answer(
        &mut self,
        path: String,
        result: Result<Vec<Track>, crate::api::ApiError>,
    ) {
        if self.local_sync.inflight.as_deref() == Some(path.as_str()) {
            self.local_sync.inflight = None;
            self.local_sync.sent = None;
        } else {
            // An answer to a look-up already given up on.
            return;
        }
        match result {
            Ok(tracks) => {
                let found = self
                    .local_songs
                    .songs
                    .iter()
                    .find(|song| song.path == path)
                    .and_then(|song| {
                        local_match::pick(&tracks, &song.title, &song.artist, song.duration_ms)
                    })
                    .map(|track| {
                        (
                            track.uri.clone(),
                            track.external_ids.isrc.clone().unwrap_or_default(),
                        )
                    });
                match found {
                    Some((uri, isrc)) => {
                        self.local_matches.remember(&path, &uri, &isrc);
                        self.local_sync.found += 1;
                        self.request_contains(vec![uri]);
                    }
                    None => self.local_matches.remember(&path, "", ""),
                }
                self.local_sync.done += 1;
                self.local_sync.unsaved += 1;
            }
            Err(error) => {
                if matches!(error, crate::api::ApiError::SignInExpired { .. }) {
                    self.local_sync.queue.clear();
                    self.toast_error("Spotify sign-in ran out. Sign in again, then press Match with Spotify");
                } else if error.is_rate_limited() {
                    // Spotify asked for a pause: wait, then ask for this one again.
                    self.local_sync.queue.push_front(path);
                    self.local_sync.next = Some(Instant::now() + MATCH_BACKOFF);
                } else {
                    // Not remembered, so a later run tries this file again.
                    self.local_sync.done += 1;
                }
            }
        }
        let finished = self.local_sync.queue.is_empty() && self.local_sync.inflight.is_none();
        if finished || self.local_sync.unsaved >= MATCH_SAVE_EVERY {
            self.save_local_matches();
        }
        if finished && self.local_sync.total > 0 {
            let found = self.local_sync.found;
            let total = self.local_sync.total;
            self.local_sync.total = 0;
            self.toast(format!("{found} of {total} songs are on Spotify"));
        }
    }

    pub(super) fn save_local_matches(&mut self) {
        self.local_matches.save(&self.dirs.index_dir()).ok();
        self.local_sync.unsaved = 0;
    }

    /// Asks Spotify whether the matched songs are in the account's library,
    /// for the hearts on the Local songs page. Cheap once answered.
    pub fn request_local_saved(&mut self) {
        if !self.can_match_files()
            || self.local_matches.matched() == 0
            || self
                .local_sync
                .saved_next
                .is_some_and(|at| Instant::now() < at)
        {
            return;
        }
        self.local_sync.saved_next = Some(Instant::now() + Duration::from_secs(5));
        let uris: Vec<String> = self
            .local_songs
            .songs
            .iter()
            .filter_map(|song| self.local_matches.uri_of(&song.path))
            .filter(|uri| !self.saved.contains_key(*uri))
            .map(str::to_string)
            .collect();
        if !uris.is_empty() {
            self.request_contains(uris);
        }
    }

    /// Works out the genre shown for each file that has none of its own:
    /// what MusicBrainz said by title and artist, or by the matched song's
    /// ISRC. Only redone when something it depends on changed.
    pub fn refresh_local_genres(&mut self) {
        let stamp = (
            self.genres_revision,
            self.local_songs.songs.len(),
            self.local_matches.len(),
        );
        if self.local_sync.genre_stamp == Some(stamp) {
            return;
        }
        let mut shown = HashMap::new();
        for song in &self.local_songs.songs {
            if !song.genre.trim().is_empty() {
                continue;
            }
            let mut names =
                crate::genres::genres_of(&crate::genres::name_key(&song.artist, &song.title));
            if names.is_empty()
                && let Some(isrc) = self.local_matches.isrc_of(&song.path)
            {
                names = crate::genres::genres_of(isrc);
            }
            if !names.is_empty() {
                shown.insert(song.path.clone(), names.join(", "));
            }
        }
        self.local_sync.genres = shown;
        self.local_sync.genre_stamp = Some(stamp);
    }

    /// Asks MusicBrainz about the files that have no genre, a few at a time.
    pub fn request_local_genres(&mut self) {
        if self
            .genre_retry_after
            .is_some_and(|at| Instant::now() < at)
            || self
                .local_sync
                .genres_next
                .is_some_and(|at| Instant::now() < at)
        {
            return;
        }
        self.local_sync.genres_next = Some(Instant::now() + LOCAL_GENRE_GAP);
        let mut batch: Vec<(String, String, String)> = Vec::new();
        for song in &self.local_songs.songs {
            if self.genre_pending.len() + batch.len() >= LOCAL_GENRES_IN_FLIGHT {
                break;
            }
            if !song.genre.trim().is_empty()
                || song.error.is_some()
                || song.artist.trim().is_empty()
                || song.title.trim().is_empty()
            {
                continue;
            }
            let key = crate::genres::name_key(&song.artist, &song.title);
            if self.genre_pending.contains(&key)
                || !self.genre_store.needs_fetch(&key)
                || batch.iter().any(|(held, _, _)| held == &key)
            {
                continue;
            }
            batch.push((key, song.artist.clone(), song.title.clone()));
        }
        for (key, artist, title) in batch {
            self.genre_pending.insert(key.clone());
            self.backend.send(Command::GenreByName { key, artist, title });
        }
    }
}
