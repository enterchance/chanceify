//! Spotify answers, fetched once and kept in the Chanceify index folder.
//!
//! A read such as an album, an artist or a playlist hardly ever changes, and
//! asking Spotify again every time it is opened is what runs the shared app's
//! quota out. So the first answer is saved under `index/answers/<account>/`
//! and used from then on until it is old. Things that are live by nature (the
//! player, the queue, devices, search, `/me`, recently played, like checks)
//! are never kept. Anything the reader changes (a playlist edit, a like, a
//! follow) throws away their saved library answers so the next look is fresh.
//! If Spotify is busy or the network is down, an old saved answer is better
//! than an error, so it is used then.
//!
//! Each account has its own folder, so a flash drive shared between people
//! never mixes their libraries. No tokens or secrets are ever stored.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use sha2::{Digest, Sha256};

/// Catalogue answers (albums, artists, tracks, shows) rarely change.
const CATALOGUE_TTL: Duration = Duration::from_secs(60 * 24 * 3600);
/// The reader's own library can change on another device, so it is asked
/// again once a week, or at once when the reader reloads a page. (It was an
/// hour, which meant every launch the next day pulled everything again.)
const LIBRARY_TTL: Duration = Duration::from_secs(7 * 24 * 3600);

struct State {
    account: Option<String>,
}

static STATE: Mutex<State> = Mutex::new(State { account: None });

fn lock() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|p| p.into_inner())
}

/// Called when a verified account becomes the active one.
pub fn set_account(account: &str) {
    lock().account = Some(account.to_string());
}

/// Called on sign-out: nothing is read or saved until the next sign-in.
pub fn clear_account() {
    lock().account = None;
}

fn root() -> PathBuf {
    crate::paths::AppDirs::discover()
        .index_dir()
        .join("answers")
}

fn account_dir() -> Option<PathBuf> {
    // Tests talk to fake servers and must never read or write real answers.
    if cfg!(test) {
        return None;
    }
    let account = lock().account.clone()?;
    let digest = Sha256::digest(account.as_bytes());
    let name: String = digest.iter().take(8).map(|b| format!("{b:02x}")).collect();
    Some(root().join(name))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Catalogue,
    Library,
}

fn kind_of(path: &str) -> Option<Kind> {
    let path = path.strip_prefix("https://api.spotify.com/v1").unwrap_or(path);
    if path.contains("/contains") {
        return None;
    }
    let first = |p: &str| path == p || path.starts_with(&format!("{p}/"));
    if first("/albums") || first("/artists") || first("/tracks") || first("/shows")
        || first("/episodes")
    {
        return Some(Kind::Catalogue);
    }
    if first("/playlists")
        || first("/me/playlists")
        || first("/me/tracks")
        || first("/me/albums")
        || first("/me/shows")
        || first("/me/episodes")
        || first("/me/following")
        || first("/me/top")
    {
        return Some(Kind::Library);
    }
    None
}

fn file_for(dir: &std::path::Path, kind: Kind, path: &str, query: &[(&str, String)]) -> PathBuf {
    let mut key = String::from(path);
    let mut pairs: Vec<_> = query.iter().map(|(k, v)| format!("{k}={v}")).collect();
    pairs.sort();
    key.push('?');
    key.push_str(&pairs.join("&"));
    let digest = Sha256::digest(key.as_bytes());
    let name: String = digest.iter().take(12).map(|b| format!("{b:02x}")).collect();
    let prefix = match kind {
        Kind::Catalogue => "c",
        Kind::Library => "l",
    };
    dir.join(format!("{prefix}-{name}.json"))
}

pub struct Saved {
    pub body: String,
    pub fresh: bool,
}

/// A saved answer for this request, if there is one. `fresh` says whether it
/// is young enough to use without asking Spotify.
pub fn lookup(path: &str, query: &[(&str, String)]) -> Option<Saved> {
    let kind = kind_of(path)?;
    let dir = account_dir()?;
    let file = file_for(&dir, kind, path, query);
    let body = std::fs::read_to_string(&file).ok()?;
    let age = std::fs::metadata(&file)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .unwrap_or(Duration::MAX);
    let ttl = match kind {
        Kind::Catalogue => CATALOGUE_TTL,
        Kind::Library => LIBRARY_TTL,
    };
    Some(Saved {
        body,
        fresh: age < ttl,
    })
}

/// Saves a good answer. Failures are ignored: a cache that cannot be written
/// just means asking again next time.
pub fn store(path: &str, query: &[(&str, String)], body: &str) {
    let Some(kind) = kind_of(path) else { return };
    if body.trim().is_empty() {
        return;
    }
    let Some(dir) = account_dir() else { return };
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let file = file_for(&dir, kind, path, query);
    let tmp = file.with_extension("tmp");
    if std::fs::write(&tmp, body).is_ok() {
        let _ = std::fs::rename(&tmp, &file);
    }
}

/// The reader changed something of their own: forget the saved library
/// answers (catalogue answers stay, they are not affected).
pub fn note_write(path: &str) {
    let path = path.strip_prefix("https://api.spotify.com/v1").unwrap_or(path);
    let touches_library = path.starts_with("/playlists")
        || path.starts_with("/me/playlists")
        || path.starts_with("/me/tracks")
        || path.starts_with("/me/albums")
        || path.starts_with("/me/shows")
        || path.starts_with("/me/episodes")
        || path.starts_with("/me/following")
        || path.starts_with("/me/library");
    if !touches_library {
        return;
    }
    forget_library();
}

/// Throws away saved library answers (used by writes and by refresh).
pub fn forget_library() {
    let Some(dir) = account_dir() else { return };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with("l-") {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_things_are_never_kept() {
        for path in [
            "/me",
            "/me/player",
            "/me/player/queue",
            "/me/player/devices",
            "/me/player/recently-played",
            "/me/library/contains",
            "/search",
        ] {
            assert!(kind_of(path).is_none(), "{path}");
        }
        assert!(kind_of("/albums/abc").is_some());
        assert!(kind_of("/me/tracks").is_some());
    }
}
