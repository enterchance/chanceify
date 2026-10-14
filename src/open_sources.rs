//! Public lookups that stand in for Spotify calls Spotify no longer allows
//! (or limits so hard that the Home page hung).
//!
//! - Songs like yours come from Last.fm (`track.getsimilar`).
//! - New releases are found on Deezer's open API (no sign-in, no key); only
//!   the few that turn up are then looked up on Spotify so they can play.

use std::time::Duration;

use serde_json::Value;

const LASTFM_URL: &str = "https://ws.audioscrobbler.com/2.0/";
const DEEZER_URL: &str = "https://api.deezer.com";

pub fn client() -> Option<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .user_agent(concat!("chanceify/", env!("CARGO_PKG_VERSION")))
        .build()
        .ok()
}

/// Songs that Last.fm says go with each of the `seeds` (artist, title),
/// the best match of each seed first, mixed so no seed fills the list.
pub async fn similar_tracks(
    client: &reqwest::Client,
    api_key: &str,
    seeds: &[(String, String)],
) -> Vec<(String, String)> {
    let proxied = api_key == crate::lastfm::VIA_PROXY && !crate::lastfm::PROXY_URL.is_empty();
    let mut per_seed: Vec<Vec<(String, String)>> = Vec::new();
    for (artist, title) in seeds {
        let mut query: Vec<(&str, &str)> = vec![
            ("method", "track.getsimilar"),
            ("artist", artist.as_str()),
            ("track", title.as_str()),
            ("limit", "8"),
            ("autocorrect", "1"),
            ("format", "json"),
        ];
        if !proxied {
            query.push(("api_key", api_key));
        }
        let url = if proxied { crate::lastfm::PROXY_URL } else { LASTFM_URL };
        let body: Option<Value> = async {
            client.get(url).query(&query).send().await.ok()?.json().await.ok()
        }
        .await;
        let list = body
            .as_ref()
            .and_then(|body| body.pointer("/similartracks/track"))
            .and_then(Value::as_array)
            .map(|tracks| {
                tracks
                    .iter()
                    .filter_map(|track| {
                        let name = track.get("name")?.as_str()?.to_string();
                        let artist = track.pointer("/artist/name")?.as_str()?.to_string();
                        Some((artist, name))
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        per_seed.push(list);
    }
    // Round-robin across the seeds, without repeats.
    let mut mixed: Vec<(String, String)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let longest = per_seed.iter().map(Vec::len).max().unwrap_or(0);
    for round in 0..longest {
        for list in &per_seed {
            if let Some(pair) = list.get(round) {
                let key = format!("{}|{}", pair.0.to_lowercase(), pair.1.to_lowercase());
                if seen.insert(key) {
                    mixed.push(pair.clone());
                }
            }
        }
    }
    mixed
}

/// A release found on Deezer.
#[derive(Clone, Debug, PartialEq)]
pub struct DeezerAlbum {
    pub title: String,
    pub date: String,
}

#[derive(Debug, PartialEq)]
pub enum DeezerError {
    /// Deezer asked us to slow down.
    Busy,
    /// No such artist there, or no answer; the caller may ask Spotify instead.
    Unknown,
}

fn deezer_failed(body: &Value) -> Option<DeezerError> {
    let code = body.pointer("/error/code").and_then(Value::as_i64)?;
    Some(if code == 4 { DeezerError::Busy } else { DeezerError::Unknown })
}

fn plain(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

async fn deezer_get(client: &reqwest::Client, path: &str, query: &[(&str, &str)]) -> Result<Value, DeezerError> {
    let response = client
        .get(format!("{DEEZER_URL}{path}"))
        .query(query)
        .send()
        .await
        .map_err(|_| DeezerError::Unknown)?;
    if response.status().as_u16() == 429 {
        return Err(DeezerError::Busy);
    }
    let body: Value = response.json().await.map_err(|_| DeezerError::Unknown)?;
    match deezer_failed(&body) {
        Some(error) => Err(error),
        None => Ok(body),
    }
}

/// The releases of `artist` dated on or after `cutoff` (`YYYY-MM-DD`).
pub async fn recent_releases(
    client: &reqwest::Client,
    artist: &str,
    cutoff: &str,
) -> Result<Vec<DeezerAlbum>, DeezerError> {
    let found = deezer_get(client, "/search/artist", &[("q", artist), ("limit", "5")]).await?;
    let wanted = plain(artist);
    let id = found
        .get("data")
        .and_then(Value::as_array)
        .and_then(|list| {
            list.iter().find(|entry| {
                entry.get("name").and_then(Value::as_str).is_some_and(|name| plain(name) == wanted)
            })
        })
        .and_then(|entry| entry.get("id").and_then(Value::as_i64))
        .ok_or(DeezerError::Unknown)?;
    let path = format!("/artist/{id}/albums");
    let first = deezer_get(client, &path, &[("limit", "100")]).await?;
    let total = first.get("total").and_then(Value::as_i64).unwrap_or(0);
    let mut lists = vec![first];
    // A long catalogue: also read the far end, whichever way Deezer orders it.
    if total > 100 {
        let index = (total - 100).to_string();
        if let Ok(last) = deezer_get(client, &path, &[("limit", "100"), ("index", index.as_str())]).await {
            lists.push(last);
        }
    }
    let mut out: Vec<DeezerAlbum> = Vec::new();
    for list in &lists {
        for album in list.get("data").and_then(Value::as_array).into_iter().flatten() {
            let (Some(title), Some(date)) = (
                album.get("title").and_then(Value::as_str),
                album.get("release_date").and_then(Value::as_str),
            ) else {
                continue;
            };
            if date.len() >= 10
                && date.get(..10).is_some_and(|day| day >= cutoff)
                && !out.iter().any(|seen| plain(&seen.title) == plain(title))
            {
                out.push(DeezerAlbum { title: title.to_string(), date: date.to_string() });
            }
        }
    }
    out.sort_by(|a, b| b.date.cmp(&a.date));
    out.truncate(4);
    Ok(out)
}

/// Whether a Spotify result's name is the one that was asked for.
pub fn same_name(found: &str, wanted: &str) -> bool {
    let (found, wanted) = (plain(found), plain(wanted));
    !wanted.is_empty() && (found == wanted || found.starts_with(&wanted) || wanted.starts_with(&found))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_loosely() {
        assert!(same_name("Blinding Lights", "blinding lights"));
        assert!(same_name("Song - Remastered", "Song"));
        assert!(!same_name("Other", "Song"));
        assert!(!same_name("Song", ""));
    }

    #[test]
    fn deezer_quota_is_busy_not_unknown() {
        assert_eq!(deezer_failed(&serde_json::json!({"error": {"code": 4}})), Some(DeezerError::Busy));
        assert_eq!(deezer_failed(&serde_json::json!({"error": {"code": 800}})), Some(DeezerError::Unknown));
        assert_eq!(deezer_failed(&serde_json::json!({"data": []})), None);
    }
}
