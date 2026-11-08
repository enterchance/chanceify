//! Every song in the user's playlists, by name, for "Search your library".
//!
//! The membership index (see [`crate::membership`]) knows which playlists
//! hold a song, but only by its URI, which nobody can type. This index keeps
//! the name, artists and album of every song the playlist walk reads, so
//! the library filter can find a song by any of them and say which of the
//! user's playlists it sits in.
//!
//! It is filled by the same background walk as the membership index, from
//! the same reads (the playlist cache on disk first, then the streaming
//! session, which spends no Web API quota), and kept on disk between runs.
//! Each playlist remembers the snapshot it was read at, so a playlist that
//! changed elsewhere is read again the next time the library list arrives.
//!
//! Liked Songs, saved albums and local files are not stored here: the app
//! already holds those lists, and [`search`] reads them as they stand.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::api::models::{
    Album, ArtistRef, PlayableItem, Playlist, PlaylistItem, SavedAlbum, SavedTrack, Track,
};
use crate::local_library::LocalSong;

/// Bumped when what is stored changes enough that an old file should be
/// read again from scratch.
const VERSION: u32 = 1;

/// The most song rows the search hands back; the rest are only counted.
pub const MAX_HITS: usize = 200;

/// One song, reduced to what a search needs to find it, show it and play it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Song {
    pub uri: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Each artist as (id, name). The id may be empty for a local file.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artists: Vec<(String, String)>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub album_id: String,
    /// The album's name, or the show's for an episode.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub album: String,
    #[serde(default)]
    pub duration_ms: u32,
}

impl Song {
    /// The parts of a track or episode the index keeps.
    pub fn from_playable(item: &PlayableItem) -> Self {
        match item {
            PlayableItem::Track(track) => Self::from_track(track, None),
            PlayableItem::Episode(episode) => Self {
                uri: episode.uri.clone(),
                name: episode.name.clone(),
                artists: Vec::new(),
                album_id: String::new(),
                album: episode
                    .show
                    .as_ref()
                    .map(|show| show.name.clone())
                    .unwrap_or_default(),
                duration_ms: episode.duration_ms,
            },
        }
    }

    /// The parts of a track the index keeps. `album` stands in for the
    /// track's own album where it has none, as an album's own track list
    /// leaves it out.
    pub fn from_track(track: &Track, album: Option<&Album>) -> Self {
        let album = track.album.as_ref().or(album);
        Self {
            uri: track.uri.clone(),
            name: track.name.clone(),
            artists: track
                .artists
                .iter()
                .map(|artist| (artist.id.clone().unwrap_or_default(), artist.name.clone()))
                .collect(),
            album_id: album.map(|album| album.id.clone()).unwrap_or_default(),
            album: album.map(|album| album.name.clone()).unwrap_or_default(),
            duration_ms: track.duration_ms,
        }
    }

    /// The songs of a playlist's rows, in order. A row whose details did
    /// not come with it keeps only its URI.
    pub fn from_rows(rows: &[PlaylistItem]) -> Vec<Self> {
        rows.iter()
            .filter_map(PlaylistItem::playable)
            .filter(|playable| !playable.uri().is_empty())
            .map(Self::from_playable)
            .collect()
    }

    /// Whether the name came with it, which is what makes it findable.
    pub fn has_details(&self) -> bool {
        !self.name.is_empty()
    }

    /// The artists as one line, "A, B".
    pub fn artist_line(&self) -> String {
        crate::api::models::join_names(self.artists.iter().map(|(_, name)| name.as_str()))
    }

    /// A track carrying what is known, for the song menu and for playing.
    pub fn to_track(&self) -> Track {
        let id_of = |uri: &str, kind: &str| {
            uri.strip_prefix(&format!("spotify:{kind}:"))
                .map(str::to_string)
        };
        Track {
            id: id_of(&self.uri, "track"),
            name: self.name.clone(),
            uri: self.uri.clone(),
            duration_ms: self.duration_ms,
            is_local: self.uri.starts_with("spotify:local:"),
            artists: self
                .artists
                .iter()
                .map(|(id, name)| ArtistRef {
                    id: (!id.is_empty()).then(|| id.clone()),
                    name: name.clone(),
                    uri: (!id.is_empty()).then(|| format!("spotify:artist:{id}")),
                })
                .collect(),
            album: (!self.album.is_empty()).then(|| Album {
                id: self.album_id.clone(),
                name: self.album.clone(),
                uri: if self.album_id.is_empty() {
                    String::new()
                } else {
                    format!("spotify:album:{}", self.album_id)
                },
                ..Album::default()
            }),
            ..Track::default()
        }
    }

    /// What the search looks through: name, artists and album, folded.
    fn key(&self) -> SearchKey {
        SearchKey::new(&self.name, &self.artist_line(), &self.album)
    }
}

/// A song as stored: the song, and the playlists (by id) that hold it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct Stored {
    #[serde(flatten)]
    song: Song,
    #[serde(default)]
    playlists: Vec<String>,
    /// Rebuilt after loading rather than saved, so the file stays small.
    #[serde(skip)]
    key: SearchKey,
}

/// A playlist the walk has read, and the snapshot it was read at.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct Walked {
    #[serde(default)]
    name: String,
    #[serde(default)]
    snapshot: Option<String>,
}

/// Every song found in the playlists walked so far.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SongIndex {
    #[serde(default)]
    version: u32,
    /// The account this was read for, so another account's songs are not
    /// offered after a switch.
    #[serde(default)]
    account: String,
    #[serde(default)]
    songs: HashMap<String, Stored>,
    #[serde(default)]
    playlists: HashMap<String, Walked>,
    /// Bumped on every change, so a cached search result knows it is stale.
    #[serde(skip)]
    generation: u64,
}

impl SongIndex {
    /// Read a saved index. A missing, unreadable or outdated file is an
    /// empty index, which only costs the walk again.
    pub fn load(path: &std::path::Path) -> Self {
        let mut index: Self = std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .filter(|index: &Self| index.version == VERSION)
            .unwrap_or_default();
        for stored in index.songs.values_mut() {
            stored.key = stored.song.key();
        }
        index.version = VERSION;
        index
    }

    /// Save the index in the background, through a temporary file, so a
    /// crash mid-write leaves the last good copy. It is a cache: a failure
    /// is not worth reporting.
    pub fn save(&self, path: &std::path::Path) {
        crate::util::save_json_in_background(self.clone(), path.to_path_buf());
    }

    /// Keep the index only if it belongs to `account`, and file it under
    /// that account from now on.
    pub fn adopt_account(&mut self, account: &str) {
        if !self.account.is_empty() && self.account != account {
            *self = Self::default();
        }
        self.account = account.to_string();
        self.version = VERSION;
        self.generation += 1;
    }

    /// Changes whenever anything in the index does.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// How many songs are known by name.
    pub fn len(&self) -> usize {
        self.songs.len()
    }

    /// Whether no song is known yet.
    pub fn is_empty(&self) -> bool {
        self.songs.is_empty()
    }

    /// How many playlists have been read.
    pub fn walked(&self) -> usize {
        self.playlists.len()
    }

    /// Whether this playlist still has to be read: never read, or changed
    /// since. A list entry with no snapshot cannot say it changed, so one
    /// read is taken as good until it does.
    pub fn needs_walk(&self, playlist: &Playlist) -> bool {
        match self.playlists.get(&playlist.id) {
            None => true,
            Some(walked) => {
                playlist.snapshot_id.is_some() && walked.snapshot != playlist.snapshot_id
            }
        }
    }

    /// Record the songs of one playlist as it stands now, replacing what an
    /// earlier read of it found. Songs without a name are left out: they
    /// could never be found.
    pub fn record(&mut self, playlist: &Playlist, songs: &[Song]) {
        self.forget_songs_of(&playlist.id);
        self.playlists.insert(
            playlist.id.clone(),
            Walked {
                name: playlist.name.clone(),
                snapshot: playlist.snapshot_id.clone(),
            },
        );
        for song in songs.iter().filter(|song| song.has_details()) {
            self.insert(song, &playlist.id);
        }
        self.generation += 1;
    }

    /// Songs were just added to a playlist here: they are found in it at
    /// once. A song the index has never seen by name is left for the next
    /// read of the playlist.
    pub fn add_songs(&mut self, playlist_id: &str, songs: &[Song]) {
        if !self.playlists.contains_key(playlist_id) {
            return;
        }
        for song in songs {
            if song.has_details() {
                self.insert(song, playlist_id);
            } else if let Some(stored) = self.songs.get_mut(&song.uri)
                && !stored.playlists.iter().any(|id| id == playlist_id)
            {
                stored.playlists.push(playlist_id.to_string());
            }
        }
        self.generation += 1;
    }

    /// Songs were just taken out of a playlist here.
    pub fn remove_songs(&mut self, playlist_id: &str, uris: &[String]) {
        for uri in uris {
            if let Some(stored) = self.songs.get_mut(uri) {
                stored.playlists.retain(|id| id != playlist_id);
                if stored.playlists.is_empty() {
                    self.songs.remove(uri);
                }
            }
        }
        self.generation += 1;
    }

    /// Bring the index in line with the user's playlist list: names follow
    /// renames, and playlists no longer in the library are forgotten.
    /// Whether anything changed, so an unchanged index is not saved again.
    pub fn follow_list(&mut self, list: &[Playlist]) -> bool {
        let listed: HashMap<&str, &Playlist> = list
            .iter()
            .map(|playlist| (playlist.id.as_str(), playlist))
            .collect();
        let gone: Vec<String> = self
            .playlists
            .keys()
            .filter(|id| !listed.contains_key(id.as_str()))
            .cloned()
            .collect();
        let mut changed = !gone.is_empty();
        for id in &gone {
            self.forget_songs_of(id);
            self.playlists.remove(id);
        }
        for (id, walked) in &mut self.playlists {
            if let Some(playlist) = listed.get(id.as_str())
                && walked.name != playlist.name
            {
                walked.name.clone_from(&playlist.name);
                changed = true;
            }
        }
        if changed {
            self.generation += 1;
        }
        changed
    }

    /// Drop everything.
    pub fn clear(&mut self) {
        self.songs.clear();
        self.playlists.clear();
        self.generation += 1;
    }

    fn insert(&mut self, song: &Song, playlist_id: &str) {
        let stored = self
            .songs
            .entry(song.uri.clone())
            .or_insert_with(|| Stored {
                song: song.clone(),
                playlists: Vec::new(),
                key: song.key(),
            });
        // A later read may carry a corrected name; take it.
        if stored.song != *song {
            stored.song = song.clone();
            stored.key = song.key();
        }
        if !stored.playlists.iter().any(|id| id == playlist_id) {
            stored.playlists.push(playlist_id.to_string());
        }
    }

    fn forget_songs_of(&mut self, playlist_id: &str) {
        self.songs.retain(|_, stored| {
            stored.playlists.retain(|id| id != playlist_id);
            !stored.playlists.is_empty()
        });
    }

    fn playlist_name(&self, id: &str) -> String {
        self.playlists
            .get(id)
            .map(|walked| walked.name.clone())
            .unwrap_or_default()
    }

    #[cfg(test)]
    fn holds(&self, playlist_id: &str, uri: &str) -> bool {
        self.songs
            .get(uri)
            .is_some_and(|stored| stored.playlists.iter().any(|id| id == playlist_id))
    }
}

/// Where a found song lives in the library.
#[derive(Clone, Debug, PartialEq)]
pub enum Place {
    Playlist {
        id: String,
        name: String,
    },
    LikedSongs,
    Album {
        id: String,
        name: String,
    },
    /// A file on this computer, by its path.
    Local {
        path: String,
    },
}

/// One song the search found, and everywhere it lives.
#[derive(Clone, Debug)]
pub struct Hit {
    pub song: Song,
    pub places: Vec<Place>,
    rank: u8,
    /// The folded name, for ordering.
    sort_name: String,
}

impl Hit {
    /// The file, when this is a song on this computer.
    pub fn local_path(&self) -> Option<&str> {
        self.places.iter().find_map(|place| match place {
            Place::Local { path } => Some(path.as_str()),
            _ => None,
        })
    }

    /// The playlists holding it, in name order.
    pub fn playlists(&self) -> impl Iterator<Item = (&str, &str)> {
        self.places.iter().filter_map(|place| match place {
            Place::Playlist { id, name } => Some((id.as_str(), name.as_str())),
            _ => None,
        })
    }

    pub fn liked(&self) -> bool {
        self.places.contains(&Place::LikedSongs)
    }
}

/// What a search found: the first [`MAX_HITS`] songs, best first, and how
/// many matched in all.
#[derive(Clone, Debug, Default)]
pub struct Results {
    pub hits: Vec<Hit>,
    pub total: usize,
}

/// The lists the search reads besides the index.
pub struct Sources<'a> {
    pub liked: &'a [SavedTrack],
    pub albums: &'a [SavedAlbum],
    pub local: &'a [LocalSong],
}

/// Every song in the library matching `query`: each word of it must appear
/// in the song's name, artists or album, ignoring case and accents.
pub fn search(index: &SongIndex, sources: &Sources<'_>, query: &str) -> Results {
    let words: Vec<String> = fold(query).split_whitespace().map(str::to_string).collect();
    if words.is_empty() {
        return Results::default();
    }
    let phrase = words.join(" ");
    let mut found: HashMap<String, Hit> = HashMap::new();
    for stored in index.songs.values() {
        if let Some(rank) = stored.key.rank(&words, &phrase) {
            let sort_name = stored.key.name().to_string();
            let mut places: Vec<Place> = stored
                .playlists
                .iter()
                .map(|id| Place::Playlist {
                    id: id.clone(),
                    name: index.playlist_name(id),
                })
                .collect();
            places.sort_by(|left, right| place_name(left).cmp(place_name(right)));
            found.insert(
                stored.song.uri.clone(),
                Hit {
                    song: stored.song.clone(),
                    places,
                    rank,
                    sort_name,
                },
            );
        }
    }
    // Liked Songs: every liked song a playlist also holds is marked, and
    // one held nowhere else joins the results.
    let liked: HashSet<&str> = sources
        .liked
        .iter()
        .map(|saved| saved.track.uri.as_str())
        .collect();
    for hit in found.values_mut() {
        if liked.contains(hit.song.uri.as_str()) {
            hit.places.insert(0, Place::LikedSongs);
        }
    }
    for saved in sources.liked {
        let track = &saved.track;
        if track.uri.is_empty() || found.contains_key(&track.uri) {
            continue;
        }
        let song = Song::from_track(track, None);
        let key = song.key();
        if let Some(rank) = key.rank(&words, &phrase) {
            found.insert(
                song.uri.clone(),
                Hit {
                    song,
                    places: vec![Place::LikedSongs],
                    rank,
                    sort_name: key.name().to_string(),
                },
            );
        }
    }
    // Saved albums, as far as their first page of tracks goes.
    for saved in sources.albums {
        let album = &saved.album;
        let Some(tracks) = &album.tracks else {
            continue;
        };
        let place = Place::Album {
            id: album.id.clone(),
            name: album.name.clone(),
        };
        for track in &tracks.items {
            if track.uri.is_empty() {
                continue;
            }
            if let Some(hit) = found.get_mut(&track.uri) {
                if !hit.places.contains(&place) {
                    hit.places.push(place.clone());
                }
                continue;
            }
            let song = Song::from_track(track, Some(album));
            let key = song.key();
            if let Some(rank) = key.rank(&words, &phrase) {
                found.insert(
                    song.uri.clone(),
                    Hit {
                        song,
                        places: vec![place.clone()],
                        rank,
                        sort_name: key.name().to_string(),
                    },
                );
            }
        }
    }
    let mut hits: Vec<Hit> = found.into_values().collect();
    for local in sources.local {
        if local.error.is_some() || local.title.is_empty() {
            continue;
        }
        let key = SearchKey::new(&local.title, &local.artist, &local.album);
        if let Some(rank) = key.rank(&words, &phrase) {
            hits.push(Hit {
                song: Song {
                    uri: String::new(),
                    name: local.title.clone(),
                    artists: if local.artist.is_empty() {
                        Vec::new()
                    } else {
                        vec![(String::new(), local.artist.clone())]
                    },
                    album_id: String::new(),
                    album: local.album.clone(),
                    duration_ms: u32::try_from(local.duration_ms).unwrap_or(u32::MAX),
                },
                places: vec![Place::Local {
                    path: local.path.clone(),
                }],
                rank,
                sort_name: key.name().to_string(),
            });
        }
    }
    let total = hits.len();
    // Best match first, then by name, then by artist, so the order is the
    // same every time the same words are typed.
    let order = |left: &Hit, right: &Hit| {
        left.rank
            .cmp(&right.rank)
            .then_with(|| left.sort_name.cmp(&right.sort_name))
            .then_with(|| left.song.artist_line().cmp(&right.song.artist_line()))
            .then_with(|| left.song.uri.cmp(&right.song.uri))
    };
    if hits.len() > MAX_HITS {
        hits.select_nth_unstable_by(MAX_HITS - 1, order);
        hits.truncate(MAX_HITS);
    }
    hits.sort_by(order);
    Results { hits, total }
}

fn place_name(place: &Place) -> &str {
    match place {
        Place::Playlist { name, .. } | Place::Album { name, .. } => name,
        Place::LikedSongs | Place::Local { .. } => "",
    }
}

/// A song's name, artists and album, folded and joined, with where the
/// name and artists end, so a match can be ranked by where it fell.
#[derive(Clone, Debug, Default, PartialEq)]
struct SearchKey {
    text: String,
    name_end: usize,
    artists_end: usize,
}

impl SearchKey {
    fn new(name: &str, artists: &str, album: &str) -> Self {
        let mut text = fold(name);
        let name_end = text.len();
        text.push('\n');
        text.push_str(&fold(artists));
        let artists_end = text.len();
        text.push('\n');
        text.push_str(&fold(album));
        Self {
            text,
            name_end,
            artists_end,
        }
    }

    /// The folded name alone.
    fn name(&self) -> &str {
        &self.text[..self.name_end]
    }

    /// `None` when some word is missing; else how good a match it is,
    /// lower being better: the name starting with what was typed, the name
    /// holding it, an artist holding it, and anything else.
    fn rank(&self, words: &[String], phrase: &str) -> Option<u8> {
        if !words.iter().all(|word| self.text.contains(word.as_str())) {
            return None;
        }
        let name = self.name();
        let artists = &self.text[self.name_end + 1..self.artists_end];
        Some(if name.starts_with(phrase) {
            0
        } else if name.contains(phrase) {
            1
        } else if artists.contains(phrase) {
            2
        } else {
            3
        })
    }
}

/// Lowercase with the accents taken off the Latin letters titles most often
/// carry, and curly quotes made straight, so "Beyonce" finds "Beyoncé".
pub fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match fold_char(character) {
            Some(plain) => out.push_str(plain),
            None => out.extend(character.to_lowercase()),
        }
    }
    out
}

fn fold_char(character: char) -> Option<&'static str> {
    Some(match character {
        'À'..='Å' | 'à'..='å' | 'Ā' | 'ā' | 'Ă' | 'ă' | 'Ą' | 'ą' => "a",
        'Æ' | 'æ' => "ae",
        'Ç' | 'ç' | 'Ć' | 'ć' | 'Č' | 'č' => "c",
        'Ď' | 'ď' | 'Đ' | 'đ' | 'Ð' | 'ð' => "d",
        'È'..='Ë' | 'è'..='ë' | 'Ē' | 'ē' | 'Ė' | 'ė' | 'Ę' | 'ę' | 'Ě' | 'ě' => "e",
        'Ğ' | 'ğ' => "g",
        'Ì'..='Ï' | 'ì'..='ï' | 'Ī' | 'ī' | 'Į' | 'į' | 'İ' | 'ı' => "i",
        'Ł' | 'ł' => "l",
        'Ñ' | 'ñ' | 'Ń' | 'ń' | 'Ň' | 'ň' => "n",
        'Ò'..='Ö' | 'Ø' | 'ò'..='ö' | 'ø' | 'Ō' | 'ō' | 'Ő' | 'ő' => "o",
        'Œ' | 'œ' => "oe",
        'Ř' | 'ř' => "r",
        'Ś' | 'ś' | 'Š' | 'š' | 'Ş' | 'ş' => "s",
        'ß' => "ss",
        'Ť' | 'ť' | 'Ţ' | 'ţ' => "t",
        'Ù'..='Ü' | 'ù'..='ü' | 'Ū' | 'ū' | 'Ů' | 'ů' | 'Ű' | 'ű' => "u",
        'Ý' | 'ý' | 'ÿ' | 'Ÿ' => "y",
        'Ź' | 'ź' | 'Ż' | 'ż' | 'Ž' | 'ž' => "z",
        '\u{2018}' | '\u{2019}' | '`' => "'",
        '\u{201C}' | '\u{201D}' => "\"",
        '\u{2010}'..='\u{2015}' => "-",
        // Combining accents, as a decomposed "e" + "´" carries them.
        '\u{0300}'..='\u{036F}' => "",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playlist(id: &str, name: &str, snapshot: &str) -> Playlist {
        Playlist {
            id: id.into(),
            name: name.into(),
            snapshot_id: Some(snapshot.into()),
            ..Playlist::default()
        }
    }

    fn song(uri: &str, name: &str, artist: &str, album: &str) -> Song {
        Song {
            uri: uri.into(),
            name: name.into(),
            artists: vec![("a1".into(), artist.into())],
            album_id: "al1".into(),
            album: album.into(),
            duration_ms: 1000,
        }
    }

    fn no_sources() -> Sources<'static> {
        Sources {
            liked: &[],
            albums: &[],
            local: &[],
        }
    }

    #[test]
    fn a_song_is_found_by_name_artist_or_album_and_lists_its_playlists() {
        let mut index = SongIndex::default();
        let one = song("spotify:track:1", "Halo", "Beyoncé", "I Am... Sasha Fierce");
        index.record(&playlist("b", "Beta", "s1"), std::slice::from_ref(&one));
        index.record(&playlist("a", "Alpha", "s1"), &[one]);
        for query in ["halo", "BEYONCE", "sasha", "beyonce halo"] {
            let found = search(&index, &no_sources(), query);
            assert_eq!(found.total, 1, "{query}");
            let names: Vec<&str> = found.hits[0].playlists().map(|(_, name)| name).collect();
            assert_eq!(names, ["Alpha", "Beta"]);
        }
        assert_eq!(search(&index, &no_sources(), "nothing like it").total, 0);
    }

    #[test]
    fn a_name_match_comes_before_an_artist_match() {
        let mut index = SongIndex::default();
        index.record(
            &playlist("a", "Alpha", "s1"),
            &[
                song("spotify:track:1", "Something", "Queen", "X"),
                song("spotify:track:2", "Queen of Hearts", "Someone", "Y"),
            ],
        );
        let found = search(&index, &no_sources(), "queen");
        assert_eq!(found.hits[0].song.uri, "spotify:track:2");
        assert_eq!(found.hits[1].song.uri, "spotify:track:1");
    }

    #[test]
    fn reading_a_playlist_again_replaces_what_it_held() {
        let mut index = SongIndex::default();
        let list = playlist("a", "Alpha", "s1");
        index.record(&list, &[song("spotify:track:1", "Old", "X", "Y")]);
        assert!(!index.needs_walk(&list));
        let changed = playlist("a", "Alpha", "s2");
        assert!(index.needs_walk(&changed));
        index.record(&changed, &[song("spotify:track:2", "New", "X", "Y")]);
        assert!(!index.holds("a", "spotify:track:1"));
        assert!(index.holds("a", "spotify:track:2"));
        assert_eq!(index.len(), 1);
    }

    #[test]
    fn a_playlist_gone_from_the_library_is_forgotten() {
        let mut index = SongIndex::default();
        index.record(
            &playlist("a", "Alpha", "s1"),
            &[song("spotify:track:1", "One", "X", "Y")],
        );
        assert!(index.follow_list(&[playlist("b", "Beta", "s1")]));
        assert!(index.is_empty());
        assert_eq!(index.walked(), 0);
    }

    #[test]
    fn liked_and_local_songs_are_found_too() {
        let index = SongIndex::default();
        let liked = [SavedTrack {
            added_at: None,
            track: song("spotify:track:9", "Liked One", "X", "Y").to_track(),
        }];
        let local = [LocalSong {
            path: "C:/music/one.mp3".into(),
            title: "Liked One (demo)".into(),
            ..LocalSong::default()
        }];
        let found = search(
            &index,
            &Sources {
                liked: &liked,
                albums: &[],
                local: &local,
            },
            "liked one",
        );
        assert_eq!(found.total, 2);
        assert!(found.hits.iter().any(Hit::liked));
        assert!(found.hits.iter().any(|hit| hit.local_path().is_some()));
    }

    #[test]
    fn many_matches_are_capped_but_counted() {
        let mut index = SongIndex::default();
        let songs: Vec<Song> = (0..(MAX_HITS + 50))
            .map(|n| {
                song(
                    &format!("spotify:track:{n}"),
                    &format!("Song {n}"),
                    "X",
                    "Y",
                )
            })
            .collect();
        index.record(&playlist("a", "Alpha", "s1"), &songs);
        let found = search(&index, &no_sources(), "song");
        assert_eq!(found.total, MAX_HITS + 50);
        assert_eq!(found.hits.len(), MAX_HITS);
    }

    #[test]
    fn the_index_survives_a_round_trip_through_the_cache() {
        let mut index = SongIndex::default();
        index.adopt_account("me");
        index.record(
            &playlist("a", "Alpha", "s1"),
            &[song("spotify:track:1", "Café", "X", "Y")],
        );
        let dir = std::env::temp_dir().join(format!(
            "chanceify-song-index-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("song_index.json");
        std::fs::write(&path, serde_json::to_vec(&index).unwrap()).unwrap();
        let restored = SongIndex::load(&path);
        assert_eq!(search(&restored, &no_sources(), "cafe").total, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn another_accounts_index_is_dropped() {
        let mut index = SongIndex::default();
        index.adopt_account("first");
        index.record(
            &playlist("a", "Alpha", "s1"),
            &[song("spotify:track:1", "One", "X", "Y")],
        );
        index.adopt_account("first");
        assert_eq!(index.len(), 1);
        index.adopt_account("second");
        assert!(index.is_empty());
    }
}
