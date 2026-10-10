//! Playlist cleanup: songs that appear twice in one playlist, and playlists
//! that mostly hold the same songs. Both are found from the same walk of the
//! playlists that finds the unavailable songs (see [`crate::unavailable`]).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// One song of a playlist, as much as the report needs.
#[derive(Clone, Debug, PartialEq)]
pub struct Song {
    pub uri: String,
    pub title: String,
    pub artists: String,
    /// The artists as (Spotify ID, name), for finding their new releases.
    pub artist_refs: Vec<(String, String)>,
}

/// What one playlist's walk found: the unplayable songs, and every song.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScanResult {
    pub entries: Vec<crate::unavailable::Entry>,
    pub songs: Vec<Song>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlaylistSongs {
    pub id: String,
    pub name: String,
    pub songs: Vec<Song>,
}

/// A song that sits in one playlist more than once.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Duplicate {
    pub playlist_id: String,
    pub playlist_name: String,
    pub title: String,
    pub artists: String,
    pub copies: u32,
}

/// Two playlists that share most of their songs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Overlap {
    pub a_id: String,
    pub a_name: String,
    pub b_id: String,
    pub b_name: String,
    pub shared: u32,
    /// Of the smaller playlist, how much of it the other holds.
    pub percent: u32,
}

/// The songs held more than once by the same playlist.
pub fn duplicates(playlists: &[PlaylistSongs]) -> Vec<Duplicate> {
    let mut found = Vec::new();
    for playlist in playlists {
        let mut counts: HashMap<&str, (u32, &Song)> = HashMap::new();
        for song in &playlist.songs {
            if song.uri.is_empty() {
                continue;
            }
            counts.entry(song.uri.as_str()).or_insert((0, song)).0 += 1;
        }
        let mut here: Vec<Duplicate> = counts
            .into_values()
            .filter(|(copies, _)| *copies > 1)
            .map(|(copies, song)| Duplicate {
                playlist_id: playlist.id.clone(),
                playlist_name: playlist.name.clone(),
                title: song.title.clone(),
                artists: song.artists.clone(),
                copies,
            })
            .collect();
        here.sort_by(|a, b| a.title.cmp(&b.title));
        found.extend(here);
    }
    found
}

/// Pairs of playlists where at least `MIN_SHARED` songs, and at least 70% of
/// the smaller playlist, are in both. The biggest overlaps come first.
pub fn overlaps(playlists: &[PlaylistSongs]) -> Vec<Overlap> {
    const MIN_SHARED: u32 = 8;
    let mut holders: HashMap<&str, Vec<usize>> = HashMap::new();
    let mut sizes = vec![0u32; playlists.len()];
    for (index, playlist) in playlists.iter().enumerate() {
        let mut seen = std::collections::HashSet::new();
        for song in &playlist.songs {
            if !song.uri.is_empty() && seen.insert(song.uri.as_str()) {
                holders.entry(song.uri.as_str()).or_default().push(index);
            }
        }
        sizes[index] = seen.len() as u32;
    }
    let mut shared: HashMap<(usize, usize), u32> = HashMap::new();
    for in_playlists in holders.values() {
        // A song in dozens of playlists would make dozens-squared pairs.
        if in_playlists.len() > 40 {
            continue;
        }
        for (position, a) in in_playlists.iter().enumerate() {
            for b in &in_playlists[position + 1..] {
                *shared.entry((*a, *b)).or_default() += 1;
            }
        }
    }
    let mut found: Vec<Overlap> = shared
        .into_iter()
        .filter_map(|((a, b), count)| {
            let smaller = sizes[a].min(sizes[b]).max(1);
            let percent = count * 100 / smaller;
            (count >= MIN_SHARED && percent >= 70).then(|| Overlap {
                a_id: playlists[a].id.clone(),
                a_name: playlists[a].name.clone(),
                b_id: playlists[b].id.clone(),
                b_name: playlists[b].name.clone(),
                shared: count,
                percent,
            })
        })
        .collect();
    found.sort_by(|a, b| {
        b.percent
            .cmp(&a.percent)
            .then(b.shared.cmp(&a.shared))
            .then_with(|| a.a_name.cmp(&b.a_name))
    });
    found.truncate(50);
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(uri: &str) -> Song {
        Song {
            uri: uri.into(),
            title: uri.to_uppercase(),
            artists: "A".into(),
            artist_refs: Vec::new(),
        }
    }

    fn list(id: &str, uris: &[&str]) -> PlaylistSongs {
        PlaylistSongs {
            id: id.into(),
            name: id.to_uppercase(),
            songs: uris.iter().map(|uri| song(uri)).collect(),
        }
    }

    #[test]
    fn a_song_twice_in_one_playlist_is_a_duplicate() {
        let found = duplicates(&[list("p", &["a", "b", "a", "a"]), list("q", &["a", "b"])]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].playlist_id, "p");
        assert_eq!(found[0].copies, 3);
    }

    #[test]
    fn mostly_the_same_songs_make_an_overlap() {
        let many: Vec<String> = (0..20).map(|n| format!("s{n}")).collect();
        let refs: Vec<&str> = many.iter().map(String::as_str).collect();
        let half: Vec<&str> = refs[..18].to_vec();
        let other: Vec<String> = (100..120).map(|n| format!("s{n}")).collect();
        let other_refs: Vec<&str> = other.iter().map(String::as_str).collect();
        let found = overlaps(&[
            list("big", &refs),
            list("small", &half),
            list("different", &other_refs),
        ]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].shared, 18);
        assert_eq!(found[0].percent, 100);
    }

    #[test]
    fn a_few_shared_songs_are_not_an_overlap() {
        let found = overlaps(&[list("a", &["x", "y", "z"]), list("b", &["x", "y", "z"])]);
        assert!(found.is_empty());
    }
}
