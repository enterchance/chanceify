//! Spotify links as they arrive from outside: the desktop's URL handler,
//! the command line, and a second launch handing one to the running
//! instance.
//!
//! Every shape Spotify hands out comes back as the one canonical URI,
//! `spotify:<kind>:<id>` or `spotify:search:<encoded query>`, or nothing
//! when it is not something the app can open.

/// The song a `chanceify://` link asked to play, until the app takes it.
fn play_request() -> &'static std::sync::Mutex<Option<String>> {
    static REQUEST: std::sync::OnceLock<std::sync::Mutex<Option<String>>> = std::sync::OnceLock::new();
    REQUEST.get_or_init(|| std::sync::Mutex::new(None))
}

/// True once, if `uri` is the song a `chanceify://` link asked to play.
pub fn take_play_request(uri: &str) -> bool {
    let mut request = play_request().lock().unwrap_or_else(|e| e.into_inner());
    if request.as_deref() == Some(uri) {
        *request = None;
        true
    } else {
        false
    }
}

/// A song asked for by name instead of by Spotify id: what a page such as
/// Last.fm hands over with `chanceify://play?artist=..&track=..` (or
/// `queue`). The canonical form carries it whole, so it survives being passed
/// from the launch that read the link to the app already running:
/// `spotify:lookup:<play|queue>:<artist>:<track>`, both names percent-encoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lookup {
    pub queue: bool,
    pub artist: String,
    pub track: String,
}

const LOOKUP_PREFIX: &str = "spotify:lookup:";
const LOOKUP_LONGEST: usize = 200;

fn clean_name(text: &str) -> Option<String> {
    let text = text.trim();
    (text.chars().count() <= LOOKUP_LONGEST && !text.chars().any(char::is_control)).then(|| text.to_string())
}

fn decode_part(text: &str) -> Option<String> {
    let text = text.replace('+', " ");
    let decoded = percent_encoding::percent_decode_str(&text).decode_utf8().ok()?;
    clean_name(&decoded)
}

/// The song behind a canonical `spotify:lookup:` link.
pub fn lookup(text: &str) -> Option<Lookup> {
    let rest = text.trim().strip_prefix(LOOKUP_PREFIX)?;
    let mut parts = rest.split(':');
    let (mode, artist, track) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    let queue = match mode {
        "play" => false,
        "queue" => true,
        _ => return None,
    };
    let track = decode_part(track)?;
    (!track.is_empty()).then(|| Lookup {
        queue,
        artist: decode_part(artist).unwrap_or_default(),
        track,
    })
}

fn canonical_lookup(found: &Lookup) -> String {
    let encode = |text: &str| percent_encoding::utf8_percent_encode(text, percent_encoding::NON_ALPHANUMERIC).to_string();
    format!(
        "{LOOKUP_PREFIX}{}:{}:{}",
        if found.queue { "queue" } else { "play" },
        encode(&found.artist),
        encode(&found.track)
    )
}

/// `play?artist=..&track=..` or `queue?..`, the part after `chanceify://`.
fn lookup_from_link(rest: &str) -> Option<Lookup> {
    let rest = rest.split('#').next().unwrap_or(rest);
    let (verb, query) = rest.split_once('?')?;
    let queue = match verb.trim_matches('/') {
        "play" => false,
        "queue" => true,
        _ => return None,
    };
    let (mut artist, mut track) = (String::new(), String::new());
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        match key {
            "artist" => artist = decode_part(value)?,
            "track" => track = decode_part(value)?,
            _ => {}
        }
    }
    (!track.is_empty()).then_some(Lookup { queue, artist, track })
}

/// Resource pages. Search links carry text instead of a resource id.
const KINDS: [&str; 6] = ["track", "album", "artist", "playlist", "show", "episode"];

/// The canonical form of a context URI Spotify reports as playing.
/// Personalized playlists report their context with the owner embedded,
/// `spotify:user:NAME:playlist:ID`, while the app's models hold the plain
/// `spotify:playlist:ID`; strict comparisons need the one shape.
pub fn canonical_context_uri(uri: &str) -> String {
    if let Some(rest) = uri.strip_prefix("spotify:user:") {
        const PLAYLIST: &str = ":playlist:";
        if let Some(at) = rest.find(PLAYLIST) {
            let id = &rest[at + PLAYLIST.len()..];
            if !id.is_empty() && !id.contains(':') {
                return format!("spotify:playlist:{id}");
            }
        }
    }
    uri.to_owned()
}

/// The canonical `spotify:<kind>:<id>` behind `text`, or `None` when it is
/// not a link to a track, album, artist, playlist, show, episode, or search.
///
/// Accepted: `spotify:track:ID`, the old `spotify:user:NAME:playlist:ID`,
/// `spotify://track/ID`, and `https://open.spotify.com/track/ID` with or
/// without a locale segment (`/intl-de/`), a query string, or the old
/// `/user/NAME/playlist/ID` shape.
pub fn parse(text: &str) -> Option<String> {
    // A song asked for by name (from a web page): the canonical form keeps
    // the names, so the instance that is already running can look it up.
    if let Some(found) = lookup(text) {
        return Some(canonical_lookup(&found));
    }
    if let Some(found) = text.trim().strip_prefix("chanceify://").and_then(lookup_from_link) {
        return Some(canonical_lookup(&found));
    }
    // chanceify://track/ID is the same link, handed to this app, and a song
    // link of this kind plays when it opens.
    if let Some(rest) = text.trim().strip_prefix("chanceify://") {
        let uri = parse(&format!("spotify://{rest}"))?;
        if uri.starts_with("spotify:track:") {
            *play_request().lock().unwrap_or_else(|e| e.into_inner()) = Some(uri.clone());
        }
        return Some(uri);
    }
    if let Some(query) = search_query(text) {
        return Some(format!(
            "spotify:search:{}",
            percent_encoding::utf8_percent_encode(&query, percent_encoding::NON_ALPHANUMERIC)
        ));
    }
    let text = text.trim();
    let mut segments: Vec<&str> = if let Some(rest) = text.strip_prefix("spotify://") {
        // The URL shape of the URI: `spotify://track/ID`, or the web
        // address with its scheme swapped.
        let mut segments = path_segments(rest);
        if segments.first().is_some_and(|host| is_web_host(host)) {
            segments.remove(0);
        }
        segments
    } else if let Some(rest) = text.strip_prefix("spotify:") {
        rest.split(':').filter(|part| !part.is_empty()).collect()
    } else {
        let rest = text
            .strip_prefix("https://")
            .or_else(|| text.strip_prefix("http://"))?;
        let mut segments = path_segments(rest);
        if !segments.first().is_some_and(|host| is_web_host(host)) {
            return None;
        }
        segments.remove(0);
        segments
    };
    // Old playlist links carry the owner: spotify:user:NAME:playlist:ID.
    if segments.len() >= 4 && segments[0] == "user" && segments[2] == "playlist" {
        segments.drain(..2);
    }
    // The web address may start with a locale: open.spotify.com/intl-de/…
    if segments
        .first()
        .is_some_and(|first| first.starts_with("intl-"))
    {
        segments.remove(0);
    }
    let [kind, id, ..] = segments.as_slice() else {
        return None;
    };
    let kind = kind.to_ascii_lowercase();
    if !KINDS.contains(&kind.as_str()) || !is_id(id) {
        return None;
    }
    Some(format!("spotify:{kind}:{id}"))
}

/// Decodes a search link once. A path's `+` is literal, not a form-encoded
/// space. Canonical links encode the whole query so delimiters and Unicode
/// survive command-line, D-Bus, Apple Event and line-based socket delivery.
pub fn search_query(text: &str) -> Option<String> {
    let text = text.trim();
    let encoded = if text.starts_with("spotify:") && !text.starts_with("spotify://") {
        let (kind, query) = text.strip_prefix("spotify:")?.split_once(':')?;
        if !kind.eq_ignore_ascii_case("search") {
            return None;
        }
        query
    } else {
        let path = if let Some(rest) = text.strip_prefix("spotify://") {
            match rest.split_once('/') {
                Some((host, path)) if is_web_host(host) => path,
                _ => rest,
            }
        } else {
            let rest = text
                .strip_prefix("https://")
                .or_else(|| text.strip_prefix("http://"))?;
            let (host, path) = rest.split_once('/')?;
            if !is_web_host(host) {
                return None;
            }
            path
        };
        let path = &path[..path.find(['?', '#']).unwrap_or(path.len())];
        let path = if path.starts_with("intl-") {
            path.split_once('/')?.1
        } else {
            path
        };
        let (kind, query) = path.split_once('/').unwrap_or((path, ""));
        if !kind.eq_ignore_ascii_case("search") {
            return None;
        }
        query
    };
    let query = percent_encoding::percent_decode_str(encoded)
        .decode_utf8()
        .ok()?;
    (!query.chars().any(char::is_control)).then(|| query.into_owned())
}

/// The path of a web address split at slashes, its query and fragment
/// dropped, empty segments (a trailing slash) with them.
fn path_segments(rest: &str) -> Vec<&str> {
    let end = rest.find(['?', '#']).unwrap_or(rest.len());
    rest[..end]
        .split('/')
        .filter(|part| !part.is_empty())
        .collect()
}

fn is_web_host(host: &str) -> bool {
    matches!(
        host.to_ascii_lowercase().as_str(),
        "open.spotify.com" | "play.spotify.com"
    )
}

/// Spotify ids are base62; anything else on a link is not one, whatever
/// hands it over.
fn is_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A page asks for a song by name; the canonical form carries the names.
    #[test]
    fn songs_asked_for_by_name() {
        let uri = parse("chanceify://play?artist=Daft+Punk&track=One%20More%20Time").unwrap();
        let wanted = Lookup { queue: false, artist: "Daft Punk".into(), track: "One More Time".into() };
        assert_eq!(lookup(&uri), Some(wanted));
        // Passed on to the running instance, the canonical form still parses.
        assert_eq!(parse(&uri), Some(uri.clone()));
        let queued = parse("chanceify://queue/?artist=A%26B&track=x").unwrap();
        assert_eq!(lookup(&queued).map(|found| (found.queue, found.artist)), Some((true, "A&B".to_string())));
        assert_eq!(parse("chanceify://play?artist=a"), None);
        assert_eq!(parse("chanceify://play?track=a%00b"), None);
    }

    /// Every shape Spotify's own apps and site hand out lands on the one
    /// URI the app navigates by.
    #[test]
    fn every_link_shape_becomes_the_one_uri() {
        // #given / #when / #then
        for (link, uri) in [
            (
                "spotify:track:4uLU6hMCjMI75M1A2tKUQC",
                "spotify:track:4uLU6hMCjMI75M1A2tKUQC",
            ),
            (
                "spotify:playlist:37i9dQZF1DXcBWIGoYBM5M",
                "spotify:playlist:37i9dQZF1DXcBWIGoYBM5M",
            ),
            (
                "spotify:user:carmine:playlist:37i9dQZF1DXcBWIGoYBM5M",
                "spotify:playlist:37i9dQZF1DXcBWIGoYBM5M",
            ),
            (
                "spotify://album/1DFixLWuPkv3KT3TnV35m3",
                "spotify:album:1DFixLWuPkv3KT3TnV35m3",
            ),
            (
                "spotify://open.spotify.com/artist/4Z8W4fKeB5YxbusRsdQVPb",
                "spotify:artist:4Z8W4fKeB5YxbusRsdQVPb",
            ),
            (
                "https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC",
                "spotify:track:4uLU6hMCjMI75M1A2tKUQC",
            ),
            (
                "https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC?si=abc123&nd=1",
                "spotify:track:4uLU6hMCjMI75M1A2tKUQC",
            ),
            (
                "https://open.spotify.com/intl-it/album/1DFixLWuPkv3KT3TnV35m3/",
                "spotify:album:1DFixLWuPkv3KT3TnV35m3",
            ),
            (
                "https://open.spotify.com/user/carmine/playlist/37i9dQZF1DXcBWIGoYBM5M",
                "spotify:playlist:37i9dQZF1DXcBWIGoYBM5M",
            ),
            (
                "http://play.spotify.com/show/4rOoJ6Egrf8K2IrywzwOMk",
                "spotify:show:4rOoJ6Egrf8K2IrywzwOMk",
            ),
            (
                "https://OPEN.SPOTIFY.COM/episode/512ojhOuo1ktJprKbVcKyQ#top",
                "spotify:episode:512ojhOuo1ktJprKbVcKyQ",
            ),
            (
                "  spotify:Artist:4Z8W4fKeB5YxbusRsdQVPb\n",
                "spotify:artist:4Z8W4fKeB5YxbusRsdQVPb",
            ),
        ] {
            assert_eq!(parse(link).as_deref(), Some(uri), "{link}");
        }
    }

    /// What is not a page here, or not Spotify's at all, is refused rather
    /// than guessed at.
    #[test]
    fn what_is_not_a_page_is_refused() {
        // #given / #when / #then
        for link in [
            "",
            "spotify:",
            "spotify:track",
            "spotify:track:",
            "spotify:user:carmine",
            "spotify:search:bad%0Aquery",
            "spotify:station:track:4uLU6hMCjMI75M1A2tKUQC",
            "spotify:local:Artist:Album:Song:180",
            "spotify:track:4uLU6hMCjMI75M1A2tKUQC/../etc",
            "spotify:track:a b",
            "https://example.com/track/4uLU6hMCjMI75M1A2tKUQC",
            "https://open.spotify.com/",
            "https://open.spotify.com/user/carmine",
            "https://open.spotify.com/intl-it/",
            "file:///etc/passwd",
            "4uLU6hMCjMI75M1A2tKUQC",
        ] {
            assert_eq!(parse(link), None, "{link:?}");
        }
        let long = format!("spotify:track:{}", "x".repeat(65));
        assert_eq!(parse(&long), None);
    }

    #[test]
    fn search_links_preserve_the_query_across_normalization_and_delivery() {
        for (link, query) in [
            (
                "https://open.spotify.com/search/here%20comes%20the%20sun",
                "here comes the sun",
            ),
            (
                "https://open.spotify.com/intl-de/search/artist%3ABj%C3%B6rk?si=share#top",
                "artist:Björk",
            ),
            (
                "spotify:search:artist:Radiohead year:1997",
                "artist:Radiohead year:1997",
            ),
            ("spotify://search/%E6%9D%B1%E4%BA%AC", "東京"),
            ("spotify://open.spotify.com/search/AC%2FDC", "AC/DC"),
            ("http://play.spotify.com/search/C%2B%2B+100%25", "C+++100%"),
            ("spotify:search:%2520", "%20"),
            ("https://open.spotify.com/search", ""),
            ("https://open.spotify.com/search/", ""),
            ("spotify:search:", ""),
        ] {
            assert_eq!(search_query(link).as_deref(), Some(query), "{link}");
            let canonical = parse(link).unwrap();
            assert_eq!(search_query(&canonical).as_deref(), Some(query));
            assert_eq!(parse(&canonical).as_ref(), Some(&canonical));
            assert!(canonical.is_ascii() && !canonical.contains(['\n', ' ']));
        }
        for invalid in [
            "https://example.com/search/song",
            "https://open.spotify.com.evil/search/song",
            "https://user@open.spotify.com/search/song",
            "file:///search/song",
            "spotify:search:bad%FFutf8",
            "https://open.spotify.com/search/line%0Abreak",
            "spotify:search:zero%00byte",
        ] {
            assert_eq!(parse(invalid), None, "{invalid}");
        }
    }
}

/// Tells Windows that `chanceify://` links open this program (the reader asked
/// for it; it is the only thing written outside the chanceify folder, and only
/// under the current user).
pub fn register_links() -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        let command = format!("\"{}\" \"%1\"", exe.display());
        let run = |args: &[&str]| -> Result<(), String> {
            let status = std::process::Command::new("reg")
                .args(args)
                .creation_flags(0x0800_0000)
                .status()
                .map_err(|error| error.to_string())?;
            status.success().then_some(()).ok_or_else(|| "Windows refused".to_string())
        };
        run(&["add", r"HKCU\Software\Classes\chanceify", "/ve", "/d", "URL:chanceify", "/f"])?;
        run(&["add", r"HKCU\Software\Classes\chanceify", "/v", "URL Protocol", "/d", "", "/f"])?;
        run(&["add", r"HKCU\Software\Classes\chanceify\shell\open\command", "/ve", "/d", &command, "/f"])
    }
    #[cfg(not(windows))]
    {
        Err("Only on Windows".to_string())
    }
}
