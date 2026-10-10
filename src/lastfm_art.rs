//! Which pictures Last.fm is missing.
//!
//! Last.fm fills its pages from what people upload. For each album and artist
//! that plays, this asks Last.fm (a plain public lookup, one a second)
//! whether it has a real picture, or only the grey placeholder. Pictures that
//! are missing are saved, nicely named, in a folder to upload by hand; every
//! cover that plays is kept too. Nothing is sent to Last.fm from here.

use std::collections::HashMap;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

const API_URL: &str = "https://ws.audioscrobbler.com/2.0/";

/// The picture Last.fm shows when it has none.
const PLACEHOLDER: &str = "2a96cbd8b46e442fc41c2b86b821562f";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Album,
    Artist,
}

#[derive(Clone, Debug)]
pub struct Job {
    pub kind: Kind,
    pub key: String,
    pub artist: String,
    pub album: String,
    pub api_key: String,
}

/// Last.fm's answer for one thing; `None` is "could not tell, ask again later".
#[derive(Clone, Debug, PartialEq)]
pub struct Answer {
    pub key: String,
    pub missing: Option<bool>,
}

/// What is known so far, kept between runs.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Store {
    /// For each album or artist key: true when Last.fm has no picture.
    #[serde(default)]
    pub checked: HashMap<String, bool>,
    /// When each was last asked about, in seconds since 1970. A picture
    /// Last.fm lacks is asked about again after a week.
    #[serde(default)]
    pub at: HashMap<String, u64>,
    /// Pictures the reader took off the list after uploading them
    /// ("albums/Artist - Album"); they come back only if a later check
    /// finds Last.fm still lacks them.
    #[serde(default)]
    pub removed: std::collections::HashSet<String>,
}

/// A week, in seconds.
pub const RECHECK_AFTER: u64 = 7 * 24 * 3600;

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
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

pub fn artist_key(artist: &str) -> String {
    format!("artist:{}", artist.trim().to_lowercase())
}

pub fn album_key(artist: &str, album: &str) -> String {
    format!(
        "album:{}|{}",
        artist.trim().to_lowercase(),
        album.trim().to_lowercase()
    )
}

/// A name that is safe on Windows, macOS and Linux: no characters they
/// forbid, no trailing dots or spaces, not too long, nothing reserved.
pub fn clean_name(text: &str) -> String {
    let mut name: String = text
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut name: String = name.chars().take(110).collect();
    while name.ends_with('.') || name.ends_with(' ') {
        name.pop();
    }
    let upper = name.to_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&upper.as_str())
        || (upper.len() == 4
            && (upper.starts_with("COM") || upper.starts_with("LPT"))
            && upper.ends_with(|c: char| c.is_ascii_digit()));
    if name.is_empty() || reserved {
        name.insert(0, '_');
    }
    name
}

/// The file name (without its ending) a picture is saved under:
/// `Artist - Album` for an album, `Artist` for an artist.
pub fn file_name(kind: Kind, artist: &str, album: &str) -> String {
    match kind {
        Kind::Artist => clean_name(artist),
        Kind::Album => clean_name(&format!("{artist} - {album}")),
    }
}

/// Whether Last.fm's answer says there is no real picture.
pub fn missing_from(body: &Value, kind: Kind) -> Option<bool> {
    if let Some(code) = body.get("error").and_then(Value::as_i64) {
        // 6: Last.fm has never heard of it, so it has no picture either.
        return (code == 6).then_some(true);
    }
    let images = match kind {
        Kind::Album => body.pointer("/album/image"),
        Kind::Artist => body.pointer("/artist/image"),
    }
    .and_then(Value::as_array)?;
    let real = images.iter().any(|image| {
        image
            .get("#text")
            .and_then(Value::as_str)
            .is_some_and(|url| !url.is_empty() && !url.contains(PLACEHOLDER))
    });
    Some(!real)
}

/// Asks Last.fm in the background, one thing a second.
#[derive(Default)]
pub struct Checker {
    sender: Mutex<Option<Sender<Job>>>,
    answers: Arc<Mutex<Vec<Answer>>>,
}

impl Checker {
    pub fn check(&self, job: Job) {
        let mut sender = self.sender.lock().unwrap_or_else(|p| p.into_inner());
        if sender.is_none() {
            let (tx, rx) = channel::<Job>();
            let answers = Arc::clone(&self.answers);
            let started = std::thread::Builder::new()
                .name("lastfm-art".into())
                .spawn(move || {
                    let client = reqwest::blocking::Client::builder()
                        .timeout(Duration::from_secs(20))
                        .user_agent(concat!("chanceify/", env!("CARGO_PKG_VERSION")))
                        .build()
                        .ok();
                    while let Ok(job) = rx.recv() {
                        let missing = client.as_ref().and_then(|client| ask(client, &job));
                        answers
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .push(Answer {
                                key: job.key,
                                missing,
                            });
                        std::thread::sleep(Duration::from_millis(1100));
                    }
                })
                .is_ok();
            if !started {
                return;
            }
            *sender = Some(tx);
        }
        if let Some(tx) = sender.as_ref() {
            let _ = tx.send(job);
        }
    }

    /// The answers that came in since last asked.
    pub fn take(&self) -> Vec<Answer> {
        std::mem::take(&mut *self.answers.lock().unwrap_or_else(|p| p.into_inner()))
    }
}

fn ask(client: &reqwest::blocking::Client, job: &Job) -> Option<bool> {
    let proxied = job.api_key == crate::lastfm::VIA_PROXY && !crate::lastfm::PROXY_URL.is_empty();
    let mut query: Vec<(&str, &str)> = vec![("format", "json")];
    if !proxied {
        query.push(("api_key", &job.api_key));
    }
    match job.kind {
        Kind::Album => {
            query.push(("method", "album.getinfo"));
            query.push(("artist", &job.artist));
            query.push(("album", &job.album));
        }
        Kind::Artist => {
            query.push(("method", "artist.getinfo"));
            query.push(("artist", &job.artist));
        }
    }
    let body: Value = client.get(if proxied { crate::lastfm::PROXY_URL } else { API_URL }).query(&query).send().ok()?.json().ok()?;
    missing_from(&body, job.kind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn names_are_safe_on_every_system() {
        assert_eq!(file_name(Kind::Album, "AC/DC", "Back: In Black?"), "AC_DC - Back_ In Black_");
        assert_eq!(file_name(Kind::Artist, "  Sigur  Rós. ", ""), "Sigur Rós");
        assert_eq!(clean_name("CON"), "_CON");
        assert_eq!(clean_name(""), "_");
        assert!(clean_name(&"x".repeat(300)).chars().count() <= 111);
    }

    #[test]
    fn keys_ignore_case_and_spaces() {
        assert_eq!(album_key(" Daft Punk", "Discovery "), album_key("daft punk", "discovery"));
        assert_ne!(artist_key("a"), album_key("a", ""));
    }

    #[test]
    fn a_placeholder_or_no_picture_is_missing() {
        let with = |url: &str| json!({"album": {"image": [{"#text": url, "size": "large"}]}});
        assert_eq!(missing_from(&with(""), Kind::Album), Some(true));
        assert_eq!(
            missing_from(&with(&format!("https://x/{PLACEHOLDER}.png")), Kind::Album),
            Some(true)
        );
        assert_eq!(
            missing_from(&with("https://lastfm.freetls.fastly.net/i/u/300x300/abc.jpg"), Kind::Album),
            Some(false)
        );
    }

    #[test]
    fn unknown_things_are_missing_and_failures_are_not_answers() {
        assert_eq!(missing_from(&json!({"error": 6, "message": "not found"}), Kind::Album), Some(true));
        assert_eq!(missing_from(&json!({"error": 29}), Kind::Album), None);
        assert_eq!(missing_from(&json!({}), Kind::Artist), None);
    }
}
