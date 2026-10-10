//! Last.fm from the player's side: keeping it signed in, telling it what is
//! playing, and counting a song once enough of it has been heard.

use super::*;
use crate::lastfm::Track;

/// A song being listened to, and how much of it has been heard.
pub(super) struct Listen {
    uri: String,
    track: Track,
    /// When the song began, in seconds since 1970.
    started_at: i64,
    listened: Duration,
    playing_since: Option<Instant>,
    now_sent: bool,
    scrobbled: bool,
    last_position_ms: u32,
}

/// A song counts once half of it, or four minutes, has been heard. Under
/// half a minute never counts.
fn heard_enough(duration_s: u32, listened: Duration) -> bool {
    duration_s >= 30
        && listened >= Duration::from_secs((u64::from(duration_s) / 2).min(240))
}

impl App {
    /// The key and secret to sign in with: the person's own if they entered
    /// them, else the ones built in.
    fn lastfm_credentials(&self) -> (String, String) {
        let key = self.settings.lastfm_api_key.trim();
        let secret = self.settings.lastfm_secret.trim();
        if key.is_empty() || secret.is_empty() {
            (
                crate::lastfm::DEFAULT_API_KEY.to_string(),
                crate::lastfm::DEFAULT_SECRET.to_string(),
            )
        } else {
            (key.to_string(), secret.to_string())
        }
    }

    /// Whether there is a key, a secret and a signed-in account.
    pub fn lastfm_ready(&self) -> bool {
        let (key, secret) = self.lastfm_credentials();
        !key.is_empty() && !secret.is_empty() && !self.settings.lastfm_session.is_empty()
    }

    /// Whether Last.fm has a key and secret to work with at all.
    pub fn lastfm_configured(&self) -> bool {
        let (key, secret) = self.lastfm_credentials();
        !key.is_empty() && !secret.is_empty()
    }

    /// Tells Last.fm's thread what to sign in with, when that has changed.
    pub(super) fn sync_lastfm_config(&mut self) {
        let (key, secret) = self.lastfm_credentials();
        let wanted = (
            key,
            secret,
            self.settings.lastfm_session.clone(),
            self.settings.lastfm_user.clone(),
        );
        if self.lastfm_sent.as_ref() == Some(&wanted) {
            return;
        }
        // Nothing to say until there is something to say it with.
        if self.lastfm_sent.is_none() && wanted.0.is_empty() && wanted.2.is_empty() {
            return;
        }
        self.lastfm.configure(
            &wanted.0,
            &wanted.1,
            &wanted.2,
            &wanted.3,
            self.dirs.index_dir().join("lastfm_queue.json"),
        );
        self.lastfm_sent = Some(wanted);
    }

    /// Each pass: keep Last.fm configured, take what it has to say, and
    /// follow the song that is playing.
    pub(super) fn drive_lastfm(&mut self) {
        self.sync_lastfm_config();
        if let Some((name, key)) = self.lastfm.take_new_session() {
            self.settings.lastfm_user = name;
            self.settings.lastfm_session = key;
            self.mark_settings_dirty();
            self.sync_lastfm_config();
        }
        for notice in self.lastfm.take_notices() {
            self.toast(notice);
        }
        if !self.lastfm_ready()
            || !(self.settings.lastfm_scrobble || self.settings.lastfm_now_playing)
        {
            self.lastfm_listen = None;
            return;
        }
        let Some(now) = self.now_playing_live().filter(|now| !now.is_episode) else {
            self.lastfm_listen = None;
            return;
        };
        if crate::file_deck::is_file_uri(&now.uri) && !self.settings.lastfm_files {
            self.lastfm_listen = None;
            return;
        }
        let artist = now
            .artists
            .first()
            .map(|artist| artist.name.clone())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| now.subtitle.clone());
        if artist.is_empty() || now.title.is_empty() {
            self.lastfm_listen = None;
            return;
        }
        // The same song starting over counts again, once the last play of it
        // has been counted.
        let started_over = self.lastfm_listen.as_ref().is_some_and(|listen| {
            listen.uri == now.uri
                && listen.scrobbled
                && now.position_ms.saturating_add(10_000) < listen.last_position_ms
        });
        let continuing = self
            .lastfm_listen
            .as_ref()
            .is_some_and(|listen| listen.uri == now.uri)
            && !started_over;
        if !continuing {
            self.lastfm_listen = Some(Listen {
                uri: now.uri.clone(),
                track: Track {
                    artist,
                    title: now.title.clone(),
                    album: now.album_name.clone(),
                    duration_s: now.duration_ms / 1000,
                },
                started_at: jiff::Timestamp::now().as_second(),
                listened: Duration::ZERO,
                playing_since: now.playing.then(Instant::now),
                now_sent: false,
                scrobbled: false,
                last_position_ms: now.position_ms,
            });
        }
        let Some(listen) = self.lastfm_listen.as_mut() else {
            return;
        };
        match (now.playing, listen.playing_since) {
            (false, Some(since)) => {
                listen.listened += since.elapsed();
                listen.playing_since = None;
            }
            (true, None) => listen.playing_since = Some(Instant::now()),
            _ => {}
        }
        listen.last_position_ms = now.position_ms;
        let listened = listen.listened
            + listen
                .playing_since
                .map(|since| since.elapsed())
                .unwrap_or_default();
        if now.playing && !listen.now_sent && self.settings.lastfm_now_playing {
            listen.now_sent = true;
            self.lastfm.now_playing(listen.track.clone());
        }
        if !listen.scrobbled
            && self.settings.lastfm_scrobble
            && heard_enough(listen.track.duration_s, listened)
        {
            listen.scrobbled = true;
            self.lastfm
                .scrobble(listen.track.clone(), listen.started_at);
        }
    }

    /// Loves the playing song on Last.fm.
    pub(super) fn lastfm_love_now(&mut self) {
        let Some(now) = self.now_playing().filter(|now| !now.is_episode) else {
            self.toast("Nothing is playing to love");
            return;
        };
        let artist = now
            .artists
            .first()
            .map(|artist| artist.name.clone())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| now.subtitle.clone());
        self.sync_lastfm_config();
        self.lastfm.love(
            Track {
                artist,
                title: now.title,
                album: now.album_name,
                duration_s: now.duration_ms / 1000,
            },
            true,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_song_counts_after_half_of_it_or_four_minutes() {
        assert!(!heard_enough(20, Duration::from_secs(20)), "too short");
        assert!(!heard_enough(200, Duration::from_secs(99)));
        assert!(heard_enough(200, Duration::from_secs(100)));
        assert!(!heard_enough(900, Duration::from_secs(239)));
        assert!(heard_enough(900, Duration::from_secs(240)));
    }
}
