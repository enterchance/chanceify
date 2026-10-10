//! Songs in the user's playlists that Spotify no longer lets them play.
//!
//! A song can leave Spotify, or leave one country's catalogue, while it stays
//! in every playlist that held it, greyed out. This keeps a per-account list
//! of those songs so they are not lost: the title and artist are what a reader
//! needs to find the song on SoundCloud, YouTube or anywhere else.

use serde::{Deserialize, Serialize};

/// One unavailable song in one playlist.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// Empty when Spotify has dropped the song so completely that not even
    /// its address is left; such an entry cannot be removed from here.
    #[serde(default)]
    pub uri: String,
    pub title: String,
    #[serde(default)]
    pub artists: String,
    pub playlist_id: String,
    pub playlist_name: String,
    #[serde(default)]
    pub position: u32,
    #[serde(default)]
    pub added_at: Option<String>,
}

impl Entry {
    /// Whether the song is known well enough to search for elsewhere.
    pub fn has_title(&self) -> bool {
        !self.uri.is_empty() && !self.title.trim().is_empty()
    }

    fn query(&self) -> String {
        urlencoding::encode(&format!("{} {}", self.artists, self.title)).into_owned()
    }

    pub fn youtube_url(&self) -> String {
        format!("https://www.youtube.com/results?search_query={}", self.query())
    }

    pub fn soundcloud_url(&self) -> String {
        format!("https://soundcloud.com/search?q={}", self.query())
    }
}

/// The saved result of the last scan.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Store {
    /// The account the list belongs to; another account's list is not shown.
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub scanned_at: Option<String>,
    #[serde(default)]
    pub entries: Vec<Entry>,
    /// Songs held twice by one playlist, from the same scan.
    #[serde(default)]
    pub duplicates: Vec<crate::cleanup::Duplicate>,
    /// Playlists that mostly hold the same songs, from the same scan.
    #[serde(default)]
    pub overlaps: Vec<crate::cleanup::Overlap>,
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
        if let Ok(bytes) = serde_json::to_vec_pretty(self) {
            let temporary = path.with_extension("json.tmp");
            if std::fs::write(&temporary, bytes).is_ok() {
                std::fs::rename(&temporary, path).ok();
            }
        }
    }

    /// The entries for `user`, or none if the list is someone else's.
    pub fn entries_for(&self, user: &str) -> &[Entry] {
        if self.user == user {
            &self.entries
        } else {
            &[]
        }
    }
}
