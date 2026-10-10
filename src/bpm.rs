//! Song tempo, and the disk cache that keeps us asking for it once.
//!
//! Spotify used to answer `GET /v1/audio-features/{id}` with a tempo and a
//! key, but it retired that endpoint for every app without a quota
//! extension from before 27 November 2024. It is gone, and there is no
//! replacement on Spotify's side: the client shows a key and tempo it
//! already knows and never asks a server for one.
//!
//! So the tempo comes from Deezer, which publishes it for free with no
//! account. The two catalogues share one identifier for a recording, the
//! ISRC, and Spotify already hands us one on every track it returns
//! (`Track::recording_key`), so joining on it is exact rather than a fuzzy
//! title-and-artist match. `https://api.deezer.com/track/isrc:{isrc}` takes
//! it directly.
//!
//! **A tempo is a property of the recording, not of the release**, so it is
//! cached by ISRC and shared by every market's release of the same song.
//!
//! **Nothing is ever asked twice.** A recording Deezer has no tempo for is
//! remembered as `null` just as one with a tempo is remembered as a number,
//! so an untempoed song — a spoken word recording, a live crowd track —
//! costs one request for the life of the profile rather than one per launch.
//! This matters more than it sounds: a playlist of ambient or spoken tracks
//! would otherwise re-request every row on every single visit.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Where the cache lives, inside the profile's own cache directory.
pub fn cache_file(cache: &Path) -> PathBuf {
    cache.join("bpm.json")
}

/// The endpoint for one recording's tempo.
/// Which layout the cache file is written in. Bumped whenever a change
/// would make what is already on disk mean something other than what it
/// says.
const VERSION: u32 = 3;

/// How long a recording Deezer had not measured is believed before it is
/// asked again.
///
/// An absence was worth remembering forever once, because a spoken word
/// recording has no tempo and asking again every launch is a request per
/// row per launch. But Deezer measures more of its catalogue over time, and
/// a reader who keeps hearing a blank next to a hardtekk track should get
/// the tempo when it turns up rather than never. Two weeks is long enough
/// that the common case costs nothing and short enough to feel like the
/// column is being looked after.
pub const REASK_DAYS: u64 = 14;
const REASK_SECONDS: u64 = REASK_DAYS * 24 * 60 * 60;

/// Seconds since the epoch, or 0 if the machine's clock says otherwise.
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0)
}

/// The cache as it sits on disk.
#[derive(serde::Serialize, serde::Deserialize)]
struct File {
    version: u32,
    tempos: HashMap<String, Option<f32>>,
    /// When each absence was learned, so it can be asked again later.
    #[serde(default)]
    asked: HashMap<String, u64>,
}

/// Deezer's exact lookup for one recording.
pub fn url(isrc: &str) -> String {
    format!("https://api.deezer.com/track/isrc:{isrc}")
}

/// The second request: Deezer leaves the tempo out of an ISRC lookup and
/// only puts it on the track's own page, so an ISRC has to be traded for
/// the id it stands for before there is anything to read.
///
/// `https://api.deezer.com/track/isrc:GBARL9300135` answers with the whole
/// track record except `bpm` and `gain`;
/// `https://api.deezer.com/track/15646529` answers with both.
pub fn track_url(id: u64) -> String {
    format!("https://api.deezer.com/track/{id}")
}

/// The numeric id out of a Deezer track body.
pub fn track_id(body: &str) -> Option<u64> {
    field(body, "id")?.parse().ok()
}

/// A tempo below this is Deezer's "not measured" sentinel rather than a
/// song that somehow moves at no beats a minute. Anything slower than
/// half a beat a minute is not music.
const MIN_TEMPO: f32 = 20.0;
/// The fastest a tempo can read before it stops being one. Faster than
/// this is a detector doubling a half-time reading, not a song.
const MAX_TEMPO: f32 = 400.0;

/// The tempo in a Deezer track body, or `None` when there is not one.
///
/// Deezer answers `0` for a recording it has not measured, and answers an
/// unrecognised ISRC with a JSON error object instead of a track. Both are
/// "no tempo", and both are cached as such.
pub fn parse(body: &str) -> Option<f32> {
    let value = field(body, "bpm")?;
    // `bpm` is a bare number in Deezer's replies, but a reader of this
    // should not have to trust that: strip anything that is not part of a
    // number before deciding.
    let digits: String = value
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    let tempo: f32 = digits.parse().ok()?;
    if tempo < MIN_TEMPO || tempo > MAX_TEMPO {
        return None;
    }
    // Deezer reports one decimal; a tempo has no more precision than that,
    // and rounding keeps a cached value identical to a fresh one.
    Some((tempo * 10.0).round() / 10.0)
}

/// One JSON field's value, without pulling in a parser for a body we only
/// read a single number out of.
fn field(body: &str, name: &str) -> Option<String> {
    let needle = format!("\"{name}\":");
    let rest = body.split(&needle).nth(1)?;
    let rest = rest.trim_start();
    let end = rest
        .find(|c: char| c == ',' || c == '}' || c == ']' || c.is_whitespace())
        .unwrap_or(rest.len());
    Some(rest[..end].to_string())
}

/// Every tempo this profile has ever learned, and which ones it has asked
/// about and been told nothing.
#[derive(Clone, Debug, Default)]
pub struct Store {
    /// ISRC to tempo. `None` is a recording that was asked about and has
    /// none, which is a different fact from never having asked.
    known: HashMap<String, Option<f32>>,
    /// When each absence was learned, so it can be asked again later
    /// rather than trusted for the life of the profile.
    asked: HashMap<String, u64>,
}

impl Store {
    /// Reads the cache. A missing or unreadable file is an empty store
    /// rather than a failure: tempos are a nicety, and a corrupt cache
    /// must not stop the app opening.
    pub fn load(cache: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(cache_file(cache)) else {
            return Self::default();
        };
        // The file is
        // `{"version":3,"tempos":{"ISRC":113.3,"OTHER":null},"asked":{"OTHER":1712}}`,
        // so a bare map of option numbers and one of timestamps read it. A
        // file written by a hand or by a build that does not fit is
        // discarded rather than fatal.
        match serde_json::from_str::<File>(&text) {
            // Version 1 was written by a build that read the tempo off the
            // ISRC lookup, where Deezer does not put it. Every recording in
            // it was therefore written down as "no tempo", and those
            // absences would have been trusted for the life of the profile.
            // Thrown away rather than honoured. Version 2 is kept, and its
            // absences simply have no timestamp: they are asked again on the
            // first launch, which is what a reader whose column is empty
            // wants.
            Ok(file) if file.version == VERSION || file.version == VERSION - 1 => Self {
                known: file
                    .tempos
                    .into_iter()
                    .filter(|(isrc, _)| !isrc.trim().is_empty())
                    .map(|(isrc, tempo)| (isrc.trim().to_ascii_uppercase(), tempo))
                    .collect(),
                asked: file
                    .asked
                    .into_iter()
                    .filter(|(isrc, _)| !isrc.trim().is_empty())
                    .map(|(isrc, at)| (isrc.trim().to_ascii_uppercase(), at))
                    .collect(),
            },
            _ => Self::default(),
        }
    }

    /// Whether this recording still needs asking about.
    ///
    /// A tempo settles a recording for good. An absence settles it for a
    /// while: after [`REASK_DAYS`] the recording is asked again, because
    /// Deezer measures more of its catalogue over time and a tempo that has
    /// turned up since should be taken.
    pub fn needs_fetch(&self, isrc: &str) -> bool {
        let isrc = isrc.trim().to_ascii_uppercase();
        match self.known.get(&isrc) {
            None => true,
            Some(Some(_)) => false,
            Some(None) => self
                .asked
                .get(&isrc)
                .is_none_or(|asked| now().saturating_sub(*asked) >= REASK_SECONDS),
        }
    }

    /// What is known about a recording: `Some(Some(tempo))` for a tempo,
    /// `Some(None)` for a confirmed absence, `None` for never asked.
    pub fn get(&self, isrc: &str) -> Option<Option<f32>> {
        self.known.get(&isrc.trim().to_ascii_uppercase()).copied()
    }

    /// What a row should show right now: a tempo, or nothing yet.
    pub fn tempo(&self, isrc: &str) -> Option<f32> {
        self.get(isrc).flatten()
    }

    /// Records an answer, from either direction. Returns whether this is a
    /// fact we did not have, so the caller can ask for a save.
    /// Records an answer and its date. Returns whether the fact is new,
    /// and only then does anything change, so the cache is only ever
    /// rewritten when it carries something it did not already hold.
    pub fn remember(&mut self, isrc: &str, tempo: Option<f32>) -> bool {
        let isrc = isrc.trim().to_ascii_uppercase();
        if isrc.is_empty() {
            return false;
        }
        match self.known.get_mut(&isrc) {
            Some(held) if *held == tempo => false,
            Some(held) => {
                *held = tempo;
                false
            }
            None => {
                self.known.insert(isrc.clone(), tempo);
                // An absence is dated so it can be asked again later; a
                // tempo is not, because it never needs asking again.
                if tempo.is_none() {
                    self.asked.insert(isrc, now());
                }
                true
            }
        }
    }

    /// Writes the cache. The caller batches these, because one playlist
    /// scroll asks about dozens of songs and rewriting the file for each
    /// would be pointless work.
    pub fn save(&self, cache: &Path) -> std::io::Result<()> {
        if let Some(parent) = cache_file(cache).parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string(&File {
            version: VERSION,
            tempos: self.known.clone(),
            asked: self.asked.clone(),
        })
        .map_err(std::io::Error::other)?;
        // The write goes to a temporary file and is renamed over the real
        // one, so a launch cut short mid-write leaves the previous cache
        // intact rather than a half-written file that reads as empty.
        let path = cache_file(cache);
        let temporary = path.with_extension("json.new");
        std::fs::write(&temporary, json)?;
        std::fs::rename(&temporary, &path)
    }

    /// How many recordings are recorded, tempos and absences together.
    pub fn len(&self) -> usize {
        self.known.len()
    }

    /// Whether anything is recorded yet. A fresh profile has none.
    pub fn is_empty(&self) -> bool {
        self.known.is_empty()
    }

    /// Every recording and its answer, so a launch can fill a map from the
    /// cache without re-deriving it.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Option<f32>)> {
        self.known.iter()
    }
}

/// Whether a Deezer answer is an error that says nothing about the song.
///
/// Deezer answers HTTP 200 even when it refuses, with
/// `{"error":{"type":"Exception","message":"Quota limit exceeded","code":4}}`.
/// That has no `bpm` in it, so reading it as "this song has no tempo" wrote
/// the refusal down as a fact: a playlist drawn in one burst got fifty
/// answers back in a second, the quota ran out, and nearly every row was
/// remembered as having no tempo. Only code 800, "no data", means Deezer
/// does not know the recording; every other error is a reason to wait and
/// ask again.
pub fn is_transient_error(body: &str) -> bool {
    let Some(rest) = body.split("\"error\":").nth(1) else {
        return false;
    };
    let code: String = rest
        .split("\"code\":")
        .nth(1)
        .map(|after| {
            after
                .trim_start()
                .chars()
                .take_while(char::is_ascii_digit)
                .collect()
        })
        .unwrap_or_default();
    code != "800"
}

/// The least time between two requests to Deezer, whichever songs they are
/// for. Its quota is fifty requests in five seconds; one lookup is up to two
/// requests, so a burst of rows must be spread out rather than sent at once.
const GAP: std::time::Duration = std::time::Duration::from_millis(160);

/// When the next request to Deezer may start.
static NEXT_SLOT: std::sync::Mutex<Option<tokio::time::Instant>> = std::sync::Mutex::new(None);

/// Waits for this request's turn. Every caller reserves the slot after the
/// last one reserved, so a hundred rows asking in the same frame line up
/// instead of arriving together.
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

/// One GET to Deezer, paced, and tried again when Deezer says to wait.
/// `None` means no real answer could be had, which is not an answer about
/// the song and must not be remembered as one.
async fn get(client: &reqwest::Client, url: &str) -> Option<String> {
    for attempt in 0..4u32 {
        wait_for_slot().await;
        let response = client.get(url).send().await.ok()?;
        let status = response.status();
        let busy = status.as_u16() == 429 || status.is_server_error();
        if !busy && !status.is_success() {
            return None;
        }
        if !busy {
            let body = response.text().await.ok()?;
            if !is_transient_error(&body) {
                return Some(body);
            }
        }
        // Two, four, eight, sixteen seconds: long enough for the quota's
        // five-second window to empty, short enough to give up in half a
        // minute rather than hold a row's place for ever.
        tokio::time::sleep(std::time::Duration::from_secs(2u64 << attempt)).await;
    }
    None
}

/// Looks one recording up on Deezer: `Some(Some(tempo))` for a tempo,
/// `Some(None)` when Deezer genuinely has none, and `None` when it could not
/// be asked, which the caller should try again later.
pub async fn lookup(client: &reqwest::Client, isrc: &str) -> Option<Option<f32>> {
    let body = get(client, &url(isrc)).await?;
    if let Some(tempo) = parse(&body) {
        return Some(Some(tempo));
    }
    // No id either: Deezer has no record of the recording at all.
    let Some(id) = track_id(&body) else {
        return Some(None);
    };
    let second = get(client, &track_url(id)).await?;
    Some(parse(&second))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deezer's refusal is HTTP 200 with an error object. It is a reason to
    /// wait, never a fact about the song; only "no data" is one.
    #[test]
    fn a_quota_refusal_is_not_an_absence() {
        let quota = r#"{"error":{"type":"Exception","message":"Quota limit exceeded","code":4}}"#;
        assert!(is_transient_error(quota));
        assert_eq!(parse(quota), None);
        let unknown = r#"{"error":{"type":"DataException","message":"no data","code":800}}"#;
        assert!(!is_transient_error(unknown), "no data is an answer");
        let track = r#"{"id":3135556,"bpm":123.4}"#;
        assert!(!is_transient_error(track));
    }

    #[test]
    fn a_tempo_is_read_from_a_deezer_track() {
        let body = r#"{"id":3135556,"title_short":"Harder, Better, Faster, Stronger","duration":"226","bpm":123.4,"gain":-7.1}"#;
        assert_eq!(parse(body), Some(123.4));
    }

    /// The ISRC lookup carries no tempo. It names the Deezer track, and the
    /// tempo is on that track's own page — so an answer without a `bpm` is
    /// not "no tempo", it is "ask again by id".
    #[test]
    fn an_isrc_answer_hands_over_the_id_the_tempo_is_read_from() {
        // A real body from `track/isrc:GBARL9300135`, tempo and gain left out.
        let body = r#"{"id":15646529,"readable":false,"title":"Never Gonna Give You Up","duration":211,"rank":1620}"#;
        assert_eq!(parse(body), None, "the ISRC answer holds no tempo at all");
        assert_eq!(track_id(body), Some(15646529));
        assert_eq!(track_url(15646529), "https://api.deezer.com/track/15646529");
    }

    /// Deezer reports `0` for a recording nobody measured. Treating that
    /// as a tempo would put a row reading "0" in a column of tempos.
    #[test]
    fn an_unmeasured_recording_has_no_tempo() {
        assert_eq!(parse(r#"{"id":1,"title_short":"No.3","bpm":0}"#), None);
    }

    /// An ISRC Deezer does not know answers with an error object, not a
    /// track. Reading past the end of that must not panic or invent one.
    #[test]
    fn an_unknown_recording_has_no_tempo() {
        let body = r#"{"error":{"type":"DataException","message":"no data","code":800}}"#;
        assert_eq!(parse(body), None);
    }

    /// A tempo outside any real range is a bad reading, not a song.
    #[test]
    fn an_impossible_tempo_is_rejected() {
        assert_eq!(parse(r#"{"bpm":0.4}"#), None);
        assert_eq!(parse(r#"{"bpm":90000}"#), None);
    }

    /// A tempo carries one decimal from the source, so a cached value and
    /// a freshly fetched one are the same number and do not fight.
    #[test]
    fn a_tempo_is_kept_at_the_precision_it_arrives_with() {
        assert_eq!(parse(r#"{"bpm":113.3333333}"#), Some(113.3));
    }

    #[test]
    fn an_isrc_is_looked_up_regardless_of_case_or_surrounding_space() {
        let mut store = Store::default();
        store.remember("  gbarL9300135  ", Some(113.3));
        assert!(!store.needs_fetch("GBARL9300135"));
        assert!(!store.needs_fetch("  gbarL9300135  "));
        assert_eq!(store.tempo("gbarL9300135"), Some(113.3));
        // One recording is one entry however it was written.
        assert_eq!(store.len(), 1);
    }

    /// The whole point of the cache: an absence is remembered, so an
    /// untempoed recording is asked about exactly once ever.
    #[test]
    fn a_recording_with_no_tempo_is_never_asked_about_twice() {
        let mut store = Store::default();
        assert!(store.needs_fetch("GBARL9300135"));
        store.remember("GBARL9300135", None);
        assert!(!store.needs_fetch("GBARL9300135"));
        assert_eq!(store.get("GBARL9300135"), Some(None));
        assert_eq!(store.tempo("GBARL9300135"), None);
    }

    #[test]
    fn a_cache_round_trips_through_the_disk() {
        let root = std::env::temp_dir().join(format!("chanceify-bpm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut store = Store::default();
        store.remember("GBARL9300135", Some(113.3));
        store.remember("NOISRC0000001", None);
        store.save(&root).expect("save");

        let loaded = Store::load(&root);
        assert_eq!(loaded.tempo("GBARL9300135"), Some(113.3));
        assert_eq!(loaded.get("NOISRC0000001"), Some(None));
        assert!(loaded.needs_fetch("NEVER00000001"));
        assert_eq!(loaded.len(), 2);
        assert!(!loaded.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A corrupt cache costs tempos, not the app.
    #[test]
    fn an_unreadable_cache_reads_as_empty() {
        let root = std::env::temp_dir().join(format!("chanceify-bpm-bad-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("mkdir");
        std::fs::write(cache_file(&root), "{ not json at all").expect("write");
        let store = Store::load(&root);
        assert_eq!(store.len(), 0);
        assert!(store.needs_fetch("GBARL9300135"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
