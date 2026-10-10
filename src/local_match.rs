//! Which of the music files are also on Spotify.
//!
//! A file is looked up on Spotify by title and artist, one song at a time and
//! in the background. When a result has the same title and first artist (and,
//! when both sides know it, nearly the same length) the file is tied to that
//! Spotify song: it can then be liked, opened on Spotify, and its genre asked
//! of MusicBrainz by the song's ISRC.
//!
//! Every answer, including "not on Spotify", is written to
//! `local_matches.json` in the index folder, so a file is asked about once. A
//! file that was not found is asked about again after a month.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::api::models::Track;

/// A file with no match is looked up again after this long.
const REASK_SECONDS: u64 = 30 * 24 * 60 * 60;

/// How far apart, in milliseconds, a file and a Spotify song may be in
/// length and still be the same recording.
const DURATION_SLACK_MS: i64 = 8_000;

pub fn file(index: &Path) -> PathBuf {
    index.join("local_matches.json")
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// The Spotify song's URI; empty when the file is not on Spotify.
    #[serde(default)]
    pub uri: String,
    #[serde(default)]
    pub isrc: String,
    #[serde(default)]
    pub at: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Stored {
    #[serde(default)]
    files: BTreeMap<String, Entry>,
}

/// Every answer so far, by the file's path.
#[derive(Clone, Debug, Default)]
pub struct Matches {
    files: BTreeMap<String, Entry>,
}

impl Matches {
    /// A missing or unreadable file is an empty list: matching is a nicety.
    pub fn load(index: &Path) -> Self {
        let stored = std::fs::read_to_string(file(index))
            .ok()
            .and_then(|text| serde_json::from_str::<Stored>(&text).ok())
            .unwrap_or_default();
        Self {
            files: stored.files,
        }
    }

    pub fn save(&self, index: &Path) -> std::io::Result<()> {
        let path = file(index);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string(&Stored {
            files: self.files.clone(),
        })
        .map_err(std::io::Error::other)?;
        let temporary = path.with_extension("json.new");
        std::fs::write(&temporary, json)?;
        std::fs::rename(&temporary, &path)
    }

    /// The matched Spotify song's URI, if this file has one.
    pub fn uri_of(&self, path: &str) -> Option<&str> {
        self.files
            .get(path)
            .map(|entry| entry.uri.as_str())
            .filter(|uri| !uri.is_empty())
    }

    pub fn isrc_of(&self, path: &str) -> Option<&str> {
        self.files
            .get(path)
            .map(|entry| entry.isrc.as_str())
            .filter(|isrc| !isrc.is_empty())
    }

    /// Whether this file still needs looking up.
    pub fn needs(&self, path: &str) -> bool {
        match self.files.get(path) {
            None => true,
            Some(entry) if !entry.uri.is_empty() => false,
            Some(entry) => now().saturating_sub(entry.at) >= REASK_SECONDS,
        }
    }

    pub fn remember(&mut self, path: &str, uri: &str, isrc: &str) {
        self.files.insert(
            path.to_string(),
            Entry {
                uri: uri.to_string(),
                isrc: isrc.trim().to_ascii_uppercase(),
                at: now(),
            },
        );
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn matched(&self) -> usize {
        self.files
            .values()
            .filter(|entry| !entry.uri.is_empty())
            .count()
    }
}

/// The text a Spotify search is asked, for a file's title and first artist.
pub fn query_for(title: &str, artist: &str) -> String {
    fn term(text: &str) -> String {
        text.chars()
            .map(|c| if c == '"' || c == '\\' { ' ' } else { c })
            .collect::<String>()
            .trim()
            .to_string()
    }
    let first_artist = artist.split([',', ';', '&']).next().unwrap_or(artist);
    let title = term(title.split(" - ").next().unwrap_or(title));
    let artist = term(first_artist);
    if artist.is_empty() {
        format!("track:\"{title}\"")
    } else {
        format!("track:\"{title}\" artist:\"{artist}\"")
    }
}

/// The result that is this file's song: same title and first artist and, when
/// both lengths are known, within a few seconds. The closest in length wins.
pub fn pick<'a>(
    tracks: &'a [Track],
    title: &str,
    artist: &str,
    duration_ms: u64,
) -> Option<&'a Track> {
    if title.trim().is_empty() || artist.trim().is_empty() {
        return None;
    }
    let wanted = crate::exportify::match_key(title, artist);
    let known = i64::try_from(duration_ms).unwrap_or(0);
    let mut best: Option<(&Track, i64)> = None;
    for track in tracks {
        if track.is_local || track.uri.is_empty() {
            continue;
        }
        if crate::exportify::match_key(&track.name, &track.artist_names()) != wanted {
            continue;
        }
        let gap = if known > 0 && track.duration_ms > 0 {
            (i64::from(track.duration_ms) - known).abs()
        } else {
            0
        };
        if known > 0 && track.duration_ms > 0 && gap > DURATION_SLACK_MS {
            continue;
        }
        if best.is_none_or(|(_, held)| gap < held) {
            best = Some((track, gap));
        }
    }
    best.map(|(track, _)| track)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::models::ArtistRef;

    fn track(name: &str, artist: &str, uri: &str, ms: u32) -> Track {
        Track {
            name: name.into(),
            uri: uri.into(),
            duration_ms: ms,
            artists: vec![ArtistRef {
                name: artist.into(),
                ..ArtistRef::default()
            }],
            ..Track::default()
        }
    }

    #[test]
    fn the_same_song_is_picked_and_others_are_not() {
        let tracks = vec![
            track("Blinding Lights (Remix)", "The Weeknd", "spotify:track:remix", 260_000),
            track("Blinding Lights", "Someone Else", "spotify:track:cover", 200_000),
            track("Blinding Lights", "The Weeknd", "spotify:track:real", 200_040),
        ];
        let found = pick(&tracks, "Blinding Lights", "The Weeknd", 200_000).unwrap();
        assert_eq!(found.uri, "spotify:track:real");
        assert!(pick(&tracks, "Other Song", "The Weeknd", 200_000).is_none());
    }

    #[test]
    fn a_very_different_length_is_a_different_recording() {
        let tracks = vec![track("Song", "Band", "spotify:track:live", 420_000)];
        assert!(pick(&tracks, "Song", "Band", 200_000).is_none());
        // With no length known on the file, the title and artist decide.
        assert!(pick(&tracks, "Song", "Band", 0).is_some());
    }

    #[test]
    fn a_file_with_no_artist_is_never_guessed() {
        let tracks = vec![track("Song", "Band", "spotify:track:x", 1000)];
        assert!(pick(&tracks, "Song", "", 0).is_none());
    }

    #[test]
    fn the_search_text_has_no_quotes_that_could_break_it() {
        assert_eq!(
            query_for("Say \"Hi\" - Radio Edit", "A, B"),
            "track:\"Say  Hi\" artist:\"A\""
        );
    }

    #[test]
    fn answers_survive_a_round_trip_and_not_found_is_remembered() {
        let mut matches = Matches::default();
        matches.remember("a.mp3", "spotify:track:1", "usabc");
        matches.remember("b.mp3", "", "");
        assert_eq!(matches.uri_of("a.mp3"), Some("spotify:track:1"));
        assert_eq!(matches.isrc_of("a.mp3"), Some("USABC"));
        assert_eq!(matches.uri_of("b.mp3"), None);
        assert!(!matches.needs("a.mp3"));
        assert!(!matches.needs("b.mp3"));
        assert!(matches.needs("c.mp3"));
        assert_eq!(matches.matched(), 1);
    }
}
