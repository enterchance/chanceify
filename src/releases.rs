//! New releases from the artists in the reader's playlists.
//!
//! The artists come from the playlists' songs (the same walk that finds the
//! unavailable songs, or one playlist's open page). For each of the most
//! heard artists Spotify is asked for the newest albums and singles, and
//! anything from the last weeks is kept.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::api::models::Album;

/// How far back a release still counts as new.
pub const NEW_FOR_DAYS: i64 = 90;

/// The most artists checked for all playlists at once, and for one playlist.
pub const MAX_ARTISTS_ALL: usize = 150;
pub const MAX_ARTISTS_ONE: usize = 80;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Release {
    pub album_id: String,
    pub album_uri: String,
    pub name: String,
    pub artists: String,
    /// "album" or "single".
    pub kind: String,
    /// As Spotify gives it: a year, a month, or a day.
    pub date: String,
    pub image: Option<String>,
}

/// The artists of one playlist: Spotify ID, name, and how many of its songs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaylistArtists {
    pub id: String,
    pub name: String,
    pub artists: Vec<(String, String, u32)>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Store {
    /// The account the lists belong to.
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub checked_at: Option<String>,
    /// The playlist the last check was for, or none for all of them.
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub found: Vec<Release>,
    #[serde(default)]
    pub playlists: Vec<PlaylistArtists>,
    /// Albums the reader took off the list: never put back by a later check.
    #[serde(default)]
    pub dismissed: Vec<String>,
}

impl Store {
    pub fn load(path: &std::path::Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &std::path::Path) {
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
}

/// The artists of a playlist's songs, most heard first.
pub fn artists_of(songs: &[crate::cleanup::Song]) -> Vec<(String, String, u32)> {
    let mut counts: HashMap<&str, (&str, u32)> = HashMap::new();
    for song in songs {
        for (id, name) in &song.artist_refs {
            counts.entry(id.as_str()).or_insert((name.as_str(), 0)).1 += 1;
        }
    }
    let mut list: Vec<(String, String, u32)> = counts
        .into_iter()
        .map(|(id, (name, count))| (id.to_string(), name.to_string(), count))
        .collect();
    list.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.1.cmp(&b.1)));
    list
}

/// Who to check: the artists of `scope` (one playlist) or of all the
/// playlists, the most heard first, at most `limit` of them.
pub fn pick_artists(
    playlists: &[PlaylistArtists],
    scope: Option<&str>,
    limit: usize,
) -> Vec<(String, String)> {
    let mut total: HashMap<&str, (&str, u32)> = HashMap::new();
    for playlist in playlists
        .iter()
        .filter(|playlist| scope.is_none_or(|wanted| wanted == playlist.id))
    {
        for (id, name, count) in &playlist.artists {
            total.entry(id.as_str()).or_insert((name.as_str(), 0)).1 += count;
        }
    }
    let mut list: Vec<(&str, &str, u32)> = total
        .into_iter()
        .map(|(id, (name, count))| (id, name, count))
        .collect();
    list.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.1.cmp(b.1)));
    list.into_iter()
        .take(limit)
        .map(|(id, name, _)| (id.to_string(), name.to_string()))
        .collect()
}

/// The day, as `YYYY-MM-DD`, that many days before `now` (seconds since 1970).
pub fn cutoff(now: i64, days: i64) -> String {
    let at = jiff::Timestamp::from_second(now - days * 86_400).unwrap_or(jiff::Timestamp::UNIX_EPOCH);
    at.to_zoned(jiff::tz::TimeZone::UTC).date().to_string()
}

/// Whether a release date is on or after the cutoff. A date of only a year
/// or a month is too vague to call new.
pub fn is_recent(date: &str, cutoff: &str) -> bool {
    date.len() >= 10 && date.get(..10).is_some_and(|day| day >= cutoff)
}

/// The release an album is, if it came out since `cutoff`.
pub fn release_from(album: &Album, cutoff: &str) -> Option<Release> {
    let date = album.release_date.as_deref()?;
    if album.id.is_empty() || !is_recent(date, cutoff) {
        return None;
    }
    let group = album
        .album_group
        .as_deref()
        .or(album.album_type.as_deref())
        .unwrap_or("album");
    Some(Release {
        album_id: album.id.clone(),
        album_uri: album.uri.clone(),
        name: album.name.clone(),
        artists: crate::api::models::join_names(album.artists.iter().map(|a| a.name.as_str())),
        kind: if group == "single" { "single" } else { "album" }.to_string(),
        date: date.to_string(),
        image: album.images.first().map(|image| image.url.clone()),
    })
}

/// Adds `release` unless the album is already there; the newest come first.
pub fn add(found: &mut Vec<Release>, release: Release) {
    if found.iter().any(|held| held.album_id == release.album_id) {
        return;
    }
    found.push(release);
    found.sort_by(|a, b| b.date.cmp(&a.date).then_with(|| a.name.cmp(&b.name)));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lists() -> Vec<PlaylistArtists> {
        vec![
            PlaylistArtists {
                id: "p1".into(),
                name: "One".into(),
                artists: vec![("a".into(), "A".into(), 5), ("b".into(), "B".into(), 1)],
            },
            PlaylistArtists {
                id: "p2".into(),
                name: "Two".into(),
                artists: vec![("b".into(), "B".into(), 6), ("c".into(), "C".into(), 2)],
            },
        ]
    }

    #[test]
    fn the_most_heard_artists_come_first_across_playlists() {
        let picked = pick_artists(&lists(), None, 2);
        assert_eq!(picked, vec![("b".to_string(), "B".to_string()), ("a".to_string(), "A".to_string())]);
    }

    #[test]
    fn one_playlist_only_gives_its_own_artists() {
        let picked = pick_artists(&lists(), Some("p2"), 10);
        assert_eq!(picked.len(), 2);
        assert_eq!(picked[0].0, "b");
        assert!(picked.iter().all(|(id, _)| id != "a"));
    }

    #[test]
    fn dates_are_compared_by_the_day() {
        assert_eq!(cutoff(86_400 * 100, 10), "1970-04-01");
        assert!(is_recent("2026-10-01", "2026-09-01"));
        assert!(!is_recent("2026-08-31", "2026-09-01"));
        assert!(!is_recent("2026", "2026-01-01"));
        assert!(!is_recent("2026-10", "2026-01-01"));
    }

    #[test]
    fn an_album_is_kept_only_when_recent_and_once() {
        let album = |id: &str, date: &str| Album {
            id: id.into(),
            name: id.into(),
            release_date: Some(date.into()),
            ..Album::default()
        };
        assert!(release_from(&album("x", "2020-01-01"), "2026-01-01").is_none());
        let release = release_from(&album("x", "2026-05-05"), "2026-01-01").unwrap();
        let mut found = Vec::new();
        add(&mut found, release.clone());
        add(&mut found, release);
        add(&mut found, release_from(&album("y", "2026-06-06"), "2026-01-01").unwrap());
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].album_id, "y");
    }

    #[test]
    fn artists_are_counted_per_song() {
        let song = |refs: &[(&str, &str)]| crate::cleanup::Song {
            uri: "u".into(),
            title: "t".into(),
            artists: String::new(),
            artist_refs: refs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
        };
        let counted = artists_of(&[song(&[("a", "A")]), song(&[("a", "A"), ("b", "B")])]);
        assert_eq!(counted[0], ("a".to_string(), "A".to_string(), 2));
        assert_eq!(counted[1].2, 1);
    }
}
