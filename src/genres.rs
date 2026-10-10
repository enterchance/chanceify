//! Song genres, from MusicBrainz and never from Spotify.
//!
//! MusicBrainz is free, needs no key and asks for at most one request a
//! second, with a User-Agent that says who is asking. A song is found by its
//! ISRC (`/ws/2/isrc/{isrc}?inc=genres+artist-credits`); a recording with no
//! genres of its own takes its artist's (`/ws/2/artist/{id}?inc=genres`).
//!
//! Every answer is written to `genres.json` in the cache folder and nothing
//! is asked twice: a song with no genre is dated and asked again only after
//! a month. The answers are also kept in one shared map so the table's sort
//! can read them without being handed the app.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

pub fn cache_file(cache: &Path) -> PathBuf {
    cache.join("genres.json")
}

/// A song with no genre is asked about again after this long.
const REASK_SECONDS: u64 = 30 * 24 * 60 * 60;

/// The least time between two requests: MusicBrainz asks for one a second.
const GAP: std::time::Duration = std::time::Duration::from_millis(1100);

/// How many genres a row shows at most.
const SHOWN: usize = 3;

static SHARED: Mutex<Option<HashMap<String, Vec<String>>>> = Mutex::new(None);
static ARTISTS: Mutex<Option<HashMap<String, Vec<String>>>> = Mutex::new(None);
static NEXT_SLOT: Mutex<Option<tokio::time::Instant>> = Mutex::new(None);

fn shared<R>(f: impl FnOnce(&mut HashMap<String, Vec<String>>) -> R) -> R {
    let mut guard = SHARED.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    f(guard.get_or_insert_with(HashMap::new))
}

fn artists<R>(f: impl FnOnce(&mut HashMap<String, Vec<String>>) -> R) -> R {
    let mut guard = ARTISTS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    f(guard.get_or_insert_with(HashMap::new))
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// The genres known for a recording, best first. Empty when none are.
pub fn genres_of(isrc: &str) -> Vec<String> {
    let key = isrc.trim().to_ascii_uppercase();
    shared(|map| map.get(&key).cloned().unwrap_or_default())
}

/// The first genre, for sorting.
pub fn first_genre(isrc: &str) -> Option<String> {
    genres_of(isrc).into_iter().next()
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Entry {
    #[serde(default)]
    genres: Vec<String>,
    #[serde(default)]
    at: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct File {
    #[serde(default)]
    songs: BTreeMap<String, Entry>,
}

/// Every answer this profile has been given.
#[derive(Clone, Debug, Default)]
pub struct Store {
    songs: BTreeMap<String, Entry>,
}

impl Store {
    /// Reads the cache and fills the shared map. A missing or unreadable
    /// file is an empty store: genres are a nicety.
    pub fn load(cache: &Path) -> Self {
        let file = std::fs::read_to_string(cache_file(cache))
            .ok()
            .and_then(|text| serde_json::from_str::<File>(&text).ok())
            .unwrap_or_default();
        shared(|map| {
            for (isrc, entry) in &file.songs {
                if !entry.genres.is_empty() {
                    map.insert(isrc.clone(), entry.genres.clone());
                }
            }
        });
        Self { songs: file.songs }
    }

    /// Whether this recording still needs asking about.
    pub fn needs_fetch(&self, isrc: &str) -> bool {
        match self.songs.get(&isrc.trim().to_ascii_uppercase()) {
            None => true,
            Some(entry) if !entry.genres.is_empty() => false,
            Some(entry) => now().saturating_sub(entry.at) >= REASK_SECONDS,
        }
    }

    /// Whether MusicBrainz has ever answered about this key (even with none).
    pub fn answered(&self, key: &str) -> bool {
        self.songs.contains_key(&key.trim().to_ascii_uppercase())
    }

    /// Records an answer, in the file's map and the shared one.
    pub fn remember(&mut self, isrc: &str, genres: Vec<String>) {
        let key = isrc.trim().to_ascii_uppercase();
        if key.is_empty() {
            return;
        }
        shared(|map| {
            if genres.is_empty() {
                map.remove(&key);
            } else {
                map.insert(key.clone(), genres.clone());
            }
        });
        self.songs.insert(key, Entry { genres, at: now() });
    }

    pub fn save(&self, cache: &Path) -> std::io::Result<()> {
        if let Some(parent) = cache_file(cache).parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string(&File {
            songs: self.songs.clone(),
        })
        .map_err(std::io::Error::other)?;
        let path = cache_file(cache);
        let temporary = path.with_extension("json.new");
        std::fs::write(&temporary, json)?;
        std::fs::rename(&temporary, &path)
    }
}

/// Names of the genres in a MusicBrainz `genres` array, most-voted first,
/// with the votes of the same name added together across `lists`.
fn ranked_names(lists: &[&serde_json::Value]) -> Vec<String> {
    let mut votes: Vec<(String, i64)> = Vec::new();
    for list in lists {
        let Some(array) = list.as_array() else {
            continue;
        };
        for genre in array {
            let Some(name) = genre.get("name").and_then(|name| name.as_str()) else {
                continue;
            };
            let count = genre.get("count").and_then(|count| count.as_i64()).unwrap_or(1);
            match votes.iter_mut().find(|(held, _)| held == name) {
                Some((_, total)) => *total += count,
                None => votes.push((name.to_string(), count)),
            }
        }
    }
    votes.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    votes.into_iter().map(|(name, _)| name).take(SHOWN).collect()
}

/// What an ISRC answer holds: the genres of its recordings, and the id of
/// the first artist, for a second question when there are none.
pub fn parse_recordings(body: &str) -> (Vec<String>, Option<String>) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return (Vec::new(), None);
    };
    let Some(recordings) = value.get("recordings").and_then(|r| r.as_array()) else {
        return (Vec::new(), None);
    };
    let lists: Vec<&serde_json::Value> = recordings
        .iter()
        .filter_map(|recording| recording.get("genres"))
        .collect();
    let artist = recordings.iter().find_map(first_artist_id);
    (ranked_names(&lists), artist)
}

/// The MusicBrainz id of a recording's first credited artist.
fn first_artist_id(recording: &serde_json::Value) -> Option<String> {
    recording
        .get("artist-credit")?
        .as_array()?
        .first()?
        .get("artist")?
        .get("id")?
        .as_str()
        .map(str::to_string)
}

/// What one recording's answer (`/recording/{id}`) holds: its genres, and
/// its first artist's id for a second question when there are none.
pub fn parse_recording(body: &str) -> (Vec<String>, Option<String>) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return (Vec::new(), None);
    };
    let genres = match value.get("genres") {
        Some(list) => ranked_names(&[list]),
        None => Vec::new(),
    };
    (genres, first_artist_id(&value))
}

/// The recording a search answer says is this title by this artist: the first
/// with a high score whose title and first artist read the same. Returns its
/// id and its artist's id.
pub fn parse_search(body: &str, title: &str, artist: &str) -> Option<(String, Option<String>)> {
    let value = serde_json::from_str::<serde_json::Value>(body).ok()?;
    let wanted = crate::exportify::match_key(title, artist);
    value
        .get("recordings")?
        .as_array()?
        .iter()
        .find_map(|recording| {
            let score = recording.get("score").and_then(|score| score.as_i64()).unwrap_or(0);
            if score < 85 {
                return None;
            }
            let name = recording.get("title")?.as_str()?;
            let credited = recording
                .get("artist-credit")?
                .as_array()?
                .first()?
                .get("artist")?
                .get("name")?
                .as_str()?;
            if crate::exportify::match_key(name, credited) != wanted {
                return None;
            }
            Some((
                recording.get("id")?.as_str()?.to_string(),
                first_artist_id(recording),
            ))
        })
}

/// The key a song asked about by name is kept under: its first artist and
/// title, boiled down, so the same song matches whatever its punctuation.
pub fn name_key(artist: &str, title: &str) -> String {
    format!("NAME:{}", crate::exportify::match_key(title, artist)).to_ascii_uppercase()
}

/// The genres for a Spotify song: those kept under its ISRC, else those kept
/// under its name (asked when it has no ISRC or MusicBrainz has no genres
/// under it).
pub fn track_genres(track: &crate::api::models::Track) -> Vec<String> {
    let by_isrc = track
        .external_ids
        .isrc
        .as_deref()
        .map(genres_of)
        .unwrap_or_default();
    if !by_isrc.is_empty() {
        return by_isrc;
    }
    match track.artists.first() {
        Some(artist) if !artist.name.is_empty() && !track.name.is_empty() => {
            genres_of(&name_key(&artist.name, &track.name))
        }
        _ => Vec::new(),
    }
}

/// The genres in an artist answer.
pub fn parse_artist(body: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return Vec::new();
    };
    match value.get("genres") {
        Some(list) => ranked_names(&[list]),
        None => Vec::new(),
    }
}

/// Waits for this request's turn, one a second whichever song asks.
async fn wait_for_slot() {
    let at = {
        let mut next = NEXT_SLOT.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let now = tokio::time::Instant::now();
        let at = next.map_or(now, |next| next.max(now));
        *next = Some(at + GAP);
        at
    };
    tokio::time::sleep_until(at).await;
}

/// One paced GET. `Ok(Some(body))` is an answer, `Ok(None)` is MusicBrainz
/// saying it has no such thing (404), `Err` is "could not be asked".
async fn get(client: &reqwest::Client, url: &str) -> Result<Option<String>, ()> {
    for attempt in 0..4u32 {
        wait_for_slot().await;
        let response = client
            .get(url)
            .header(
                "User-Agent",
                concat!("chanceify/", env!("CARGO_PKG_VERSION"), " ( https://github.com/madlygeeked/chanceify )"),
            )
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|error| log::warn!("MusicBrainz could not be reached: {error}"))?;
        let status = response.status();
        if status.as_u16() == 404 {
            return Ok(None);
        }
        let busy = status.as_u16() == 429 || status.as_u16() == 503 || status.is_server_error();
        if status.is_success() {
            return response.text().await.map(Some).map_err(|_| ());
        }
        if !busy {
            log::warn!("MusicBrainz answered {status} for {url}");
            return Err(());
        }
        tokio::time::sleep(std::time::Duration::from_secs(3u64 << attempt)).await;
    }
    Err(())
}

/// Looks one recording's genres up. `Some(genres)` is an answer (empty means
/// MusicBrainz has none); `None` means it could not be asked, so the caller
/// tries again later and remembers nothing.
pub async fn lookup(client: &reqwest::Client, isrc: &str) -> Option<Vec<String>> {
    let url = format!(
        "https://musicbrainz.org/ws/2/isrc/{isrc}?inc=genres+artist-credits&fmt=json"
    );
    let body = match get(client, &url).await {
        Ok(Some(body)) => body,
        Ok(None) => return Some(Vec::new()),
        Err(()) => return None,
    };
    let (genres, artist) = parse_recordings(&body);
    if !genres.is_empty() {
        return Some(genres);
    }
    let Some(artist) = artist else {
        return Some(Vec::new());
    };
    artist_genres(client, artist).await
}

/// An artist's own genres, asked once per artist per run.
async fn artist_genres(client: &reqwest::Client, artist: String) -> Option<Vec<String>> {
    if let Some(known) = artists(|map| map.get(&artist).cloned()) {
        return Some(known);
    }
    let url = format!("https://musicbrainz.org/ws/2/artist/{artist}?inc=genres&fmt=json");
    let genres = match get(client, &url).await {
        Ok(Some(body)) => parse_artist(&body),
        Ok(None) => Vec::new(),
        Err(()) => return None,
    };
    artists(|map| map.insert(artist, genres.clone()));
    Some(genres)
}

/// Looks a song's genres up by its artist and title, for a song with no ISRC
/// or one MusicBrainz knows no genres under (and for every music file).
/// Same answers as `lookup`: `Some` is an answer, `None` could not be asked.
pub async fn lookup_by_name(
    client: &reqwest::Client,
    artist: &str,
    title: &str,
) -> Option<Vec<String>> {
    fn term(text: &str) -> String {
        text.chars()
            .map(|c| if c == '"' || c == '\\' { ' ' } else { c })
            .collect::<String>()
            .trim()
            .to_string()
    }
    let first_artist = artist.split([',', ';', '&']).next().unwrap_or(artist);
    let (artist_term, title_term) = (term(first_artist), term(title.split(" - ").next().unwrap_or(title)));
    if artist_term.is_empty() || title_term.is_empty() {
        return Some(Vec::new());
    }
    let query = format!("recording:\"{title_term}\" AND artist:\"{artist_term}\"");
    let url = format!(
        "https://musicbrainz.org/ws/2/recording?query={}&limit=5&fmt=json",
        urlencoding::encode(&query)
    );
    let body = match get(client, &url).await {
        Ok(Some(body)) => body,
        Ok(None) => return Some(Vec::new()),
        Err(()) => return None,
    };
    let Some((id, searched_artist)) = parse_search(&body, title, first_artist) else {
        return Some(Vec::new());
    };
    let url = format!("https://musicbrainz.org/ws/2/recording/{id}?inc=genres+artist-credits&fmt=json");
    let body = match get(client, &url).await {
        Ok(Some(body)) => body,
        Ok(None) => return Some(Vec::new()),
        Err(()) => return None,
    };
    let (genres, credited) = parse_recording(&body);
    if !genres.is_empty() {
        return Some(genres);
    }
    match credited.or(searched_artist) {
        Some(artist) => artist_genres(client, artist).await,
        None => Some(Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_genres_are_ranked_by_votes() {
        let body = r#"{"isrc":"X","recordings":[{"id":"r","title":"T","genres":[{"name":"pop","count":2},{"name":"synth-pop","count":5}],"artist-credit":[{"artist":{"id":"a1","name":"A"}}]}]}"#;
        let (genres, artist) = parse_recordings(body);
        assert_eq!(genres, vec!["synth-pop", "pop"]);
        assert_eq!(artist.as_deref(), Some("a1"));
    }

    #[test]
    fn a_recording_with_no_genres_still_names_its_artist() {
        let body = r#"{"recordings":[{"id":"r","genres":[],"artist-credit":[{"artist":{"id":"a2"}}]}]}"#;
        let (genres, artist) = parse_recordings(body);
        assert!(genres.is_empty());
        assert_eq!(artist.as_deref(), Some("a2"));
    }

    #[test]
    fn an_artist_answer_gives_its_genres() {
        let body = r#"{"id":"a","genres":[{"name":"rock","count":9},{"name":"indie","count":3},{"name":"a","count":1},{"name":"b","count":1}]}"#;
        assert_eq!(parse_artist(body), vec!["rock", "indie", "a"]);
        assert!(parse_artist("not json").is_empty());
    }

    #[test]
    fn a_search_answer_picks_the_recording_that_reads_the_same() {
        let body = r#"{"recordings":[
            {"id":"low","score":60,"title":"Song","artist-credit":[{"artist":{"id":"x","name":"Band"}}]},
            {"id":"other","score":100,"title":"Song","artist-credit":[{"artist":{"id":"y","name":"Other"}}]},
            {"id":"right","score":98,"title":"Song (Remastered)","artist-credit":[{"artist":{"id":"z","name":"Band"}}]}]}"#;
        let (id, artist) = parse_search(body, "Song", "Band").unwrap();
        assert_eq!(id, "right");
        assert_eq!(artist.as_deref(), Some("z"));
        assert!(parse_search(body, "Missing", "Band").is_none());
        assert!(parse_search("nope", "Song", "Band").is_none());
    }

    #[test]
    fn one_recording_answer_gives_genres_and_artist() {
        let body = r#"{"id":"r","genres":[{"name":"jazz","count":4}],"artist-credit":[{"artist":{"id":"q"}}]}"#;
        let (genres, artist) = parse_recording(body);
        assert_eq!(genres, vec!["jazz"]);
        assert_eq!(artist.as_deref(), Some("q"));
    }

    #[test]
    fn a_name_key_ignores_case_and_brackets() {
        assert_eq!(
            name_key("The Band, Guest", "Song (Live)"),
            name_key("the band", "SONG")
        );
    }
}
