//! Which of the user's own playlists a song appears in.
//!
//! Spotify has no endpoint for "which playlists contain this track", so the
//! index is built the only way it can be: walk the playlists the user owns,
//! read each one's tracks, and record the reverse mapping. That is a lot of
//! requests for a large library, so it is fetched in the background, in
//! batches, and cached on disk; rows show nothing until the answer arrives and
//! never wait on it.
//!
//! Only playlists the user owns are walked. A followed playlist belongs to
//! someone else, and reading all of them would be far larger still.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::api::models::{Playlist, pick_image};

/// One playlist a song was found in, reduced to what an icon needs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Membership {
    pub id: String,
    pub name: String,
    /// The playlist's cover, already reduced to the size an icon shows.
    #[serde(default)]
    pub image: Option<String>,
}

impl Membership {
    /// What a row shows for this playlist. Spotify omits images on a playlist
    /// with no art, and nulls them for one the owner cannot supply, so both
    /// cases fall back to the initial.
    pub fn initial(&self) -> char {
        self.name
            .chars()
            .find(|character| character.is_alphanumeric())
            .unwrap_or('?')
            .to_uppercase()
            .next()
            .unwrap_or('?')
    }
}

/// The track URI to the playlists containing it, and the playlists walked so
/// far. A partial index still answers correctly for the songs it has seen,
/// which is what makes showing it while the walk continues safe.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Index {
    /// Sorted by name so a row's icons do not jump about between rebuilds.
    #[serde(default)]
    by_track: HashMap<String, Vec<Membership>>,
    /// The playlists already walked, so a rebuild resumes rather than
    /// starting over.
    #[serde(default)]
    scanned: Vec<String>,
}

impl Index {
    /// The playlists containing `uri`, in a stable order.
    pub fn get(&self, uri: &str) -> &[Membership] {
        self.by_track.get(uri).map_or(&[], Vec::as_slice)
    }

    /// Whether every playlist has been walked.
    pub fn is_complete(&self, expected: usize) -> bool {
        self.scanned.len() >= expected
    }

    /// Whether anything has been recorded yet.
    pub fn is_empty(&self) -> bool {
        self.scanned.is_empty()
    }

    /// How many playlists have been walked.
    pub fn scanned(&self) -> usize {
        self.scanned.len()
    }

    /// Whether this playlist has already been walked.
    pub fn has(&self, id: &str) -> bool {
        self.scanned.iter().any(|scanned| scanned == id)
    }

    /// Record the songs in one playlist. A playlist already recorded is
    /// skipped, so a repeated page adds nothing.
    ///
    /// Songs are appended rather than replacing the whole entry, because the
    /// walk visits each playlist once but a song can appear in many.
    pub fn add(&mut self, playlist: &Playlist, uris: impl IntoIterator<Item = String>) {
        if self.scanned.contains(&playlist.id) {
            return;
        }
        let membership = Membership {
            id: playlist.id.clone(),
            name: playlist.name.clone(),
            image: pick_image(&playlist.images, 64).map(str::to_string),
        };
        for uri in uris {
            let entry = self.by_track.entry(uri).or_default();
            if !entry.iter().any(|found| found.id == membership.id) {
                entry.push(membership.clone());
            }
        }
        self.scanned.push(playlist.id.clone());
        for playlists in self.by_track.values_mut() {
            playlists.sort_by(|left, right| left.name.cmp(&right.name));
        }
    }

    /// Forget a playlist, so its icons stop appearing after it is deleted.
    pub fn remove(&mut self, id: &str) {
        self.scanned.retain(|scanned| scanned != id);
        for playlists in self.by_track.values_mut() {
            playlists.retain(|membership| membership.id != id);
        }
        self.by_track.retain(|_, playlists| !playlists.is_empty());
    }

    /// How many songs were found across the playlists walked.
    pub fn tracks(&self) -> usize {
        self.by_track.len()
    }

    /// Records a song as being in the named playlists, for a test that needs an
    /// index without walking a whole library to build one.
    #[cfg(test)]
    pub fn add_test(&mut self, uri: &str, in_playlists: &[&str]) {
        let mut held: Vec<Membership> = in_playlists
            .iter()
            .map(|name| Membership {
                id: name.to_lowercase(),
                name: (*name).to_string(),
                image: None,
            })
            .collect();
        held.sort_by(|left, right| left.name.cmp(&right.name));
        self.by_track.insert(uri.to_string(), held);
        for name in in_playlists {
            if !self
                .scanned
                .iter()
                .any(|scanned| scanned == &name.to_lowercase())
            {
                self.scanned.push(name.to_lowercase());
            }
        }
    }

    /// Drop everything, so the next build starts clean.
    pub fn clear(&mut self) {
        self.by_track.clear();
        self.scanned.clear();
    }

    /// Read a saved index. A missing or unreadable file is an empty index,
    /// which only costs the walk again.
    pub fn load(path: &std::path::Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Save the index, so the walk is done once rather than every launch.
    ///
    /// Written through a temporary file and moved into place, so a crash
    /// mid-write leaves the last good index rather than a half-written one.
    /// The index is a cache: a failure here is not worth reporting.
    pub fn save(&self, path: &std::path::Path) {
        let Ok(text) = serde_json::to_string(self) else {
            return;
        };
        let temporary = path.with_extension("json.tmp");
        if std::fs::write(&temporary, text).is_ok() {
            let _ = crate::util::replace_file(&temporary, path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::models::{Image, Playlist};

    fn playlist(id: &str, name: &str) -> Playlist {
        Playlist {
            id: id.into(),
            name: name.into(),
            images: vec![Image {
                url: format!("https://example.test/{id}.jpg"),
                height: Some(64),
                width: Some(64),
            }],
            ..Playlist::default()
        }
    }

    #[test]
    fn a_song_records_every_playlist_it_is_in() {
        let mut index = Index::default();
        index.add(&playlist("a", "Alpha"), ["spotify:track:1".into()]);
        index.add(&playlist("b", "Beta"), ["spotify:track:1".into()]);
        let found = index.get("spotify:track:1");
        assert_eq!(found.len(), 2);
        // Sorted by name, so a row's icons do not reorder between rebuilds.
        assert_eq!(found[0].name, "Alpha");
        assert_eq!(found[1].name, "Beta");
    }

    #[test]
    fn a_song_in_no_playlist_reports_nothing() {
        let mut index = Index::default();
        index.add(&playlist("a", "Alpha"), ["spotify:track:1".into()]);
        assert!(index.get("spotify:track:2").is_empty());
    }

    #[test]
    fn a_playlist_walked_twice_adds_nothing() {
        let mut index = Index::default();
        let king = playlist("k", "King Engine");
        index.add(&king, ["spotify:track:1".into()]);
        index.add(&king, ["spotify:track:1".into()]);
        assert_eq!(index.get("spotify:track:1").len(), 1);
        assert_eq!(index.scanned(), 1);
    }

    #[test]
    fn a_song_repeated_inside_one_playlist_is_listed_once() {
        let mut index = Index::default();
        index.add(
            &playlist("a", "Alpha"),
            ["spotify:track:1".into(), "spotify:track:1".into()],
        );
        assert_eq!(index.get("spotify:track:1").len(), 1);
    }

    #[test]
    fn deleting_a_playlist_takes_its_icons_away() {
        let mut index = Index::default();
        index.add(&playlist("a", "Alpha"), ["spotify:track:1".into()]);
        index.add(&playlist("b", "Beta"), ["spotify:track:1".into()]);
        index.remove("a");
        let found = index.get("spotify:track:1");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "Beta");
    }

    #[test]
    fn removing_the_last_playlist_forgets_the_song() {
        let mut index = Index::default();
        index.add(&playlist("a", "Alpha"), ["spotify:track:1".into()]);
        index.remove("a");
        assert!(index.get("spotify:track:1").is_empty());
        assert_eq!(index.tracks(), 0);
    }

    #[test]
    fn a_playlist_with_no_art_falls_back_to_its_initial() {
        let mut bare = playlist("a", "Alpha");
        bare.images.clear();
        let mut index = Index::default();
        index.add(&bare, ["spotify:track:1".into()]);
        let found = index.get("spotify:track:1");
        assert_eq!(found[0].image, None);
        assert_eq!(found[0].initial(), 'A');
    }

    #[test]
    fn a_name_starting_with_a_symbol_still_has_an_initial() {
        let mut index = Index::default();
        index.add(&playlist("a", "!!! loud"), ["spotify:track:1".into()]);
        assert_eq!(index.get("spotify:track:1")[0].initial(), 'L');
    }

    #[test]
    fn completeness_waits_for_every_playlist() {
        let mut index = Index::default();
        index.add(&playlist("a", "Alpha"), ["spotify:track:1".into()]);
        assert!(!index.is_complete(2));
        index.add(&playlist("b", "Beta"), ["spotify:track:2".into()]);
        assert!(index.is_complete(2));
    }

    #[test]
    fn the_index_survives_a_round_trip_through_the_cache() {
        let mut index = Index::default();
        index.add(&playlist("a", "Alpha"), ["spotify:track:1".into()]);
        index.add(&playlist("b", "Beta"), ["spotify:track:1".into()]);
        let restored: Index =
            serde_json::from_str(&serde_json::to_string(&index).unwrap()).unwrap();
        assert_eq!(restored, index);
        assert_eq!(restored.get("spotify:track:1").len(), 2);
    }

    #[test]
    fn an_empty_or_broken_cache_reads_as_an_empty_index() {
        let restored: Index = serde_json::from_str("{}").unwrap();
        assert_eq!(restored, Index::default());
        // A cache from an older shape must not stop the app starting.
        let restored: Index = serde_json::from_str(r#"{"broken":true}"#).unwrap();
        assert!(restored.get("spotify:track:1").is_empty());
    }
}
