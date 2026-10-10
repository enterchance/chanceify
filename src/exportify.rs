//! Reading a playlist saved by Exportify (or any tool that writes the same
//! kind of CSV), so a library can be browsed with no internet at all.
//!
//! An export has one row per song with headers such as `Track URI`,
//! `Track Name`, `Album Name`, `Artist Name(s)`, `Duration (ms)`, `Genres`,
//! and, in older exports, `Tempo`, `Key` and `Mode`. Columns are found by
//! their heading, not their position, so a different order or a missing
//! column is fine. The songs themselves are NOT in the file, only the list;
//! hearing one offline needs the audio file in the local songs folder.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct OfflineTrack {
    #[serde(default)]
    pub uri: String,
    pub name: String,
    #[serde(default)]
    pub album: String,
    #[serde(default)]
    pub artists: String,
    #[serde(default)]
    pub duration_ms: u64,
    #[serde(default)]
    pub release_date: String,
    #[serde(default)]
    pub genres: Vec<String>,
    #[serde(default)]
    pub tempo: Option<f32>,
    /// 0 is C ... 11 is B, as Spotify numbered it.
    #[serde(default)]
    pub key: Option<u8>,
    #[serde(default)]
    pub minor: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct OfflinePlaylist {
    pub name: String,
    pub tracks: Vec<OfflineTrack>,
}

/// Splits CSV text into rows of fields. Quotes may hold commas, doubled
/// quotes and line breaks.
pub fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut rows = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    quoted = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' => quoted = true,
            ',' => row.push(std::mem::take(&mut field)),
            '\r' => {}
            '\n' => {
                row.push(std::mem::take(&mut field));
                if row.iter().any(|f| !f.is_empty()) {
                    rows.push(std::mem::take(&mut row));
                } else {
                    row.clear();
                }
            }
            _ => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        if row.iter().any(|f| !f.is_empty()) {
            rows.push(row);
        }
    }
    rows
}

fn find(header: &[String], names: &[&str]) -> Option<usize> {
    header.iter().position(|h| {
        let h = h.trim().to_lowercase();
        names.iter().any(|n| h == *n)
    })
}

/// Reads one export. `name` is the playlist's name (the file's name).
pub fn parse_playlist(name: &str, text: &str) -> Result<OfflinePlaylist, String> {
    let rows = parse_csv(text);
    let Some((header, body)) = rows.split_first() else {
        return Err("the file is empty".into());
    };
    let Some(title) = find(header, &["track name", "name", "title", "song"]) else {
        return Err("no \"Track Name\" column - is this an Exportify file?".into());
    };
    let uri = find(header, &["track uri", "uri", "spotify uri"]);
    let album = find(header, &["album name", "album"]);
    let artists = find(header, &["artist name(s)", "artist names", "artist name", "artists", "artist"]);
    let duration = find(header, &["duration (ms)", "duration_ms", "duration ms"]);
    let release = find(header, &["release date", "album release date"]);
    let genres = find(header, &["genres", "genre"]);
    let tempo = find(header, &["tempo", "bpm"]);
    let key = find(header, &["key"]);
    let mode = find(header, &["mode"]);
    let get = |row: &Vec<String>, index: Option<usize>| -> String {
        index
            .and_then(|i| row.get(i))
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    };
    let mut tracks = Vec::with_capacity(body.len());
    for row in body {
        let name = get(row, Some(title));
        if name.is_empty() {
            continue;
        }
        let genre_text = get(row, genres);
        tracks.push(OfflineTrack {
            uri: get(row, uri),
            name,
            album: get(row, album),
            artists: get(row, artists).replace(';', ", "),
            duration_ms: get(row, duration).parse().unwrap_or(0),
            release_date: get(row, release),
            genres: genre_text
                .split([',', ';'])
                .map(|g| g.trim().to_string())
                .filter(|g| !g.is_empty())
                .collect(),
            tempo: get(row, tempo).parse::<f32>().ok().filter(|t| *t > 20.0 && *t < 300.0),
            // Spotify used -1 for "no key".
            key: get(row, key).parse::<i32>().ok().filter(|k| (0..12).contains(k)).map(|k| k as u8),
            minor: get(row, mode).parse::<i32>().ok().map(|m| m == 0),
        });
    }
    Ok(OfflinePlaylist {
        name: name.to_string(),
        tracks,
    })
}

/// `index/exportify`: drop the CSV files here, then press Import.
pub fn import_dir(index: &std::path::Path) -> std::path::PathBuf {
    index.join("exportify")
}

/// `index/offline`: what has been imported, one file per playlist.
pub fn offline_dir(index: &std::path::Path) -> std::path::PathBuf {
    index.join("offline")
}

fn file_safe(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let cleaned = cleaned.trim().to_string();
    if cleaned.is_empty() { "playlist".into() } else { cleaned }
}

/// Reads every CSV in `index/exportify` and saves each as an offline
/// playlist. Returns how many worked and a line for each that did not.
pub fn import_all(index: &std::path::Path) -> (usize, Vec<String>) {
    let from = import_dir(index);
    let to = offline_dir(index);
    std::fs::create_dir_all(&from).ok();
    std::fs::create_dir_all(&to).ok();
    let mut done = 0;
    let mut problems = Vec::new();
    let Ok(entries) = std::fs::read_dir(&from) else {
        return (0, problems);
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() != Some("csv") {
            continue;
        }
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().replace('_', " "))
            .unwrap_or_default();
        let label = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let text = match std::fs::read(&path) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).to_string(),
            Err(error) => {
                problems.push(format!("{label}: {error}"));
                continue;
            }
        };
        match parse_playlist(&name, &text) {
            Ok(playlist) => {
                let target = to.join(format!("{}.json", file_safe(&playlist.name)));
                match serde_json::to_vec(&playlist) {
                    Ok(bytes) if std::fs::write(&target, &bytes).is_ok() => done += 1,
                    _ => problems.push(format!("{label}: could not be saved")),
                }
            }
            Err(error) => problems.push(format!("{label}: {error}")),
        }
    }
    (done, problems)
}

/// Saves a playlist taken from the open Spotify page into `index/offline`,
/// the same place imported lists go, so it can be browsed with no internet.
pub fn save_offline(
    index: &std::path::Path,
    playlist: &OfflinePlaylist,
) -> std::io::Result<std::path::PathBuf> {
    let folder = offline_dir(index);
    std::fs::create_dir_all(&folder)?;
    let target = folder.join(format!("{}.json", file_safe(&playlist.name)));
    let bytes = serde_json::to_vec(playlist).map_err(std::io::Error::other)?;
    let temporary = target.with_extension("json.new");
    std::fs::write(&temporary, bytes)?;
    std::fs::rename(&temporary, &target)?;
    Ok(target)
}

/// Every imported playlist, by name.
pub fn load_offline(index: &std::path::Path) -> Vec<OfflinePlaylist> {
    let mut all: Vec<OfflinePlaylist> = std::fs::read_dir(offline_dir(index))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("json"))
        .filter_map(|e| std::fs::read(e.path()).ok())
        .filter_map(|bytes| serde_json::from_slice::<OfflinePlaylist>(&bytes).ok())
        .collect();
    all.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    all
}

/// A song's name and artists, boiled down so the same song matches whatever
/// punctuation, case or "(Remastered)" tag each side used.
pub fn match_key(title: &str, artists: &str) -> String {
    fn clean(text: &str) -> String {
        let mut out = String::new();
        let mut depth = 0u32;
        for c in text.chars() {
            match c {
                '(' | '[' => depth += 1,
                ')' | ']' => depth = depth.saturating_sub(1),
                _ if depth > 0 => {}
                c if c.is_alphanumeric() => out.extend(c.to_lowercase()),
                _ => {}
            }
        }
        out
    }
    // "Title - Remastered 2011" and "Title - Radio Edit" lose the suffix.
    let title = title.split(" - ").next().unwrap_or(title);
    let first_artist = artists.split([',', ';', '&']).next().unwrap_or(artists);
    format!("{}|{}", clean(title), clean(first_artist))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_handles_quotes_commas_and_line_breaks() {
        let rows = parse_csv("a,b,c\r\n\"x, y\",\"he said \"\"hi\"\"\",\"two\nlines\"\n");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1], vec!["x, y", "he said \"hi\"", "two\nlines"]);
    }

    #[test]
    fn reads_an_exportify_file_by_heading() {
        let text = "\u{feff}Track URI,Track Name,Album Name,Artist Name(s),Release Date,Duration (ms),Genres,Key,Mode,Tempo\n\
spotify:track:1,Hold On,Wild,\"A;B\",2015-01-02,201000,\"indie pop,dance\",9,0,127.9\n\
spotify:track:2,No Key,Wild,C,2016,1000,,-1,1,0\n";
        let p = parse_playlist("Mix", text).unwrap();
        assert_eq!(p.tracks.len(), 2);
        let t = &p.tracks[0];
        assert_eq!(t.uri, "spotify:track:1");
        assert_eq!(t.artists, "A, B");
        assert_eq!(t.genres, vec!["indie pop", "dance"]);
        assert_eq!(t.key, Some(9));
        assert_eq!(t.minor, Some(true));
        assert_eq!(t.tempo, Some(127.9));
        assert_eq!(p.tracks[1].key, None);
        assert_eq!(p.tracks[1].tempo, None);
    }

    #[test]
    fn a_file_with_no_name_column_is_refused() {
        assert!(parse_playlist("x", "foo,bar\n1,2\n").is_err());
        assert!(parse_playlist("x", "").is_err());
    }

    #[test]
    fn the_same_song_matches_across_spellings() {
        assert_eq!(
            match_key("Hold On (Remastered 2011)", "Wilson Phillips, X"),
            match_key("hold on", "Wilson Phillips")
        );
        assert_eq!(
            match_key("Song - Radio Edit", "A & B"),
            match_key("Song", "A")
        );
        assert_ne!(match_key("Song", "A"), match_key("Song", "B"));
    }
}
