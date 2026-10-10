//! Discord Rich Presence: shows the song playing on the reader's Discord
//! profile.
//!
//! Discord's desktop app listens on a local pipe (`discord-ipc-0` and up).
//! A program says who it is by sending the *Application ID* of an app made
//! at <https://discord.com/developers/applications> (no bot, no token, no
//! server: only the ID), then sends the activity to show. Nothing leaves the
//! computer except through Discord's own app.
//!
//! The pipe is spoken to by one background thread, so a closed Discord, a
//! slow pipe or an odd answer never holds up drawing. The thread keeps the
//! newest activity and tries again every few seconds until Discord is there.

use std::io::{Read, Write};
use std::sync::{Mutex, mpsc};
use std::time::Duration;

use serde_json::{Value, json};

/// What to show.
#[derive(Clone, Debug, PartialEq)]
pub struct Activity {
    /// First line: the song.
    pub details: String,
    /// Second line: the artist.
    pub state: String,
    /// Make the song, and the artist, links on the profile.
    pub details_url: Option<String>,
    pub state_url: Option<String>,
    /// What the line under the name in the member list says: 0 the app's
    /// name, 1 the second line (the artist), 2 the first line (the song).
    pub status_display: u8,
    /// A web address, or the key of a picture uploaded to the Discord app.
    pub large_image: Option<String>,
    /// Shown when the picture is hovered: the album.
    pub large_text: String,
    /// The small picture in the corner of the big one: chanceify's badge.
    pub small_image: Option<String>,
    pub small_text: String,
    /// At most two buttons under the activity: a label and a web address.
    pub buttons: Vec<(String, String)>,
    /// Seconds since 1970 the song (would have) started, and ends. Both
    /// `None` while paused.
    pub start: Option<i64>,
    pub end: Option<i64>,
    /// What a friend's Discord hands back when they press Join: the song and
    /// when it started (see [`join_secret`]). `None` unless listen-along is on.
    pub join: Option<String>,
}

impl Activity {
    /// The same line and picture, and times within a few seconds: a progress
    /// bar that drifts by the width of a frame is not a new activity.
    fn same_as(&self, other: &Self) -> bool {
        let near = |a: Option<i64>, b: Option<i64>| match (a, b) {
            (Some(a), Some(b)) => (a - b).abs() <= 3,
            (None, None) => true,
            _ => false,
        };
        self.details == other.details
            && self.state == other.state
            && self.details_url == other.details_url
            && self.state_url == other.state_url
            && self.status_display == other.status_display
            && self.large_image == other.large_image
            && self.large_text == other.large_text
            && self.small_image == other.small_image
            && self.small_text == other.small_text
            && self.buttons == other.buttons
            && near(self.start, other.start)
            && near(self.end, other.end)
    }
}

/// The key of the chanceify picture uploaded to the Discord application's
/// Rich Presence assets (a lower-case name, set when it was uploaded).
pub const BADGE_KEY: &str = "chanceify";

/// Where the swirl pictures live: the chanceify site, from the repository's docs folder.
const SWIRL_BASE: &str = "https://madlygeeked.github.io/chanceify/swirl/";

/// The swirl picture nearest a cover's colour: twelve hues round the wheel,
/// and a grey one for covers with hardly any colour.
pub fn swirl_url(red: u8, green: u8, blue: u8) -> String {
    let (r, g, b) = (f32::from(red) / 255.0, f32::from(green) / 255.0, f32::from(blue) / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    if max < 0.12 || delta / max.max(0.001) < 0.18 {
        return format!("{SWIRL_BASE}swirl-gray.png");
    }
    let hue = if max == r {
        ((g - b) / delta).rem_euclid(6.0)
    } else if max == g {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    } * 60.0;
    let index = ((hue / 30.0).round() as usize) % 12;
    format!("{SWIRL_BASE}swirl-{index}.png")
}

/// How the reader wants their profile to look (Settings > Discord).
#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    /// 0 the song, 1 the artist, 2 chanceify: what the status line says.
    pub status_line: u8,
    pub cover: bool,
    pub badge: bool,
    /// A swirl picture (web address) in the cover's colour, shown small on the cover.
    pub swirl: Option<String>,
    /// The playlist the song plays from, when the reader wants it said.
    pub playlist: Option<String>,
    /// The reader's Spotify profile address, when they want a button for it.
    pub profile_url: Option<String>,
    pub buttons: bool,
    /// A button to the playlist the song plays from.
    pub playlist_url: Option<String>,
    /// The song button leads to the chanceify song page, not Spotify.
    pub song_page: bool,
    /// 0 album big, swirl small; 1 swirl big, album small; 2 badge only.
    pub look: u8,
    pub links: bool,
    pub hide_paused: bool,
    pub files: bool,
    /// Let friends join in and hear the same song.
    pub listen_along: bool,
    /// Where "Get chanceify" leads, when there is a real address to give.
    pub app_url: Option<String>,
}

/// The web address of a Spotify song or episode, from its URI.
fn web_url(uri: &str) -> Option<String> {
    let mut parts = uri.split(':');
    if parts.next()? != "spotify" {
        return None;
    }
    let kind = parts.next()?;
    let id = parts.next()?;
    let clean = !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric());
    match kind {
        "track" | "episode" if clean => Some(format!("https://open.spotify.com/{kind}/{id}")),
        _ => None,
    }
}

/// What to show on the profile for what is playing, or `None` for nothing.
/// The music is the headline: the song and artist, the cover, a progress bar;
/// chanceify is the small badge, the hover text and a button.
pub fn activity_for(
    now: &crate::app::NowPlaying,
    style: &Style,
    now_seconds: i64,
) -> Option<Activity> {
    if now.resuming || now.uri.is_empty() || now.title.is_empty() {
        return None;
    }
    let is_file = crate::file_deck::is_file_uri(&now.uri);
    if (is_file && !style.files) || (!now.playing && style.hide_paused) {
        return None;
    }
    let timed = now.playing && now.duration_ms > 0;
    let start = now_seconds - i64::from(now.position_ms / 1000);
    let song_url = if is_file { None } else { web_url(&now.uri) };
    let artist_url = if is_file {
        None
    } else {
        now.artists
            .first()
            .and_then(|artist| artist.id.as_deref())
            .filter(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric()))
            .map(|id| format!("https://open.spotify.com/artist/{id}"))
    };
    // A cover only this program can show (a file's) cannot be fetched by Discord.
    let cover = if style.cover {
        now.art_url
            .clone()
            .or_else(|| now.art_small.clone())
            .filter(|url| url.starts_with("https://") && !url.contains(".invalid/"))
    } else {
        None
    };
    let (large_image, small_image) = match (cover, style.look) {
        // The swirl big, the album in the corner.
        (Some(url), 1) if style.swirl.is_some() => (style.swirl.clone(), Some(url)),
        // Only the chanceify badge.
        (_, 2) => (Some(BADGE_KEY.to_string()), None),
        (Some(url), _) => (
            Some(url),
            style
                .swirl
                .clone()
                .or_else(|| style.badge.then(|| BADGE_KEY.to_string())),
        ),
        (None, _) => (style.badge.then(|| BADGE_KEY.to_string()), None),
    };
    let mut buttons: Vec<(String, String)> = Vec::new();
    if style.buttons {
        let page = if style.song_page && !is_file {
            song_link(&now.uri, &now.title, &now.subtitle)
        } else {
            None
        };
        if let Some(url) = page.as_ref().or(song_url.as_ref()) {
            buttons.push((
                if page.is_some() { "Open the song" } else { "Listen on Spotify" }.to_string(),
                url.clone(),
            ));
        }
        if let Some(url) = &style.playlist_url {
            buttons.push(("Open the playlist".to_string(), url.clone()));
        }
        // Discord shows two at most: the song, then the profile or the app.
        if let Some(url) = &style.profile_url {
            buttons.push(("My Spotify profile".to_string(), url.clone()));
        }
        if buttons.len() < 2
            && let Some(url) = &style.app_url
        {
            buttons.push((format!("Get {}", crate::build_info::DISPLAY_NAME), url.clone()));
        }
    }
    buttons.truncate(2);
    Some(Activity {
        details: now.title.clone(),
        state: if now.playing || now.subtitle.is_empty() {
            now.subtitle.clone()
        } else {
            format!("{} (paused)", now.subtitle)
        },
        details_url: if style.links { song_url } else { None },
        state_url: if style.links { artist_url } else { None },
        status_display: match style.status_line {
            1 => 1,
            2 => 0,
            _ => 2,
        },
        large_image,
        large_text: {
            let album = if now.album_name.is_empty() {
                crate::build_info::DISPLAY_NAME.to_string()
            } else {
                now.album_name.clone()
            };
            match &style.playlist {
                Some(playlist) => format!("{album} - from {playlist}"),
                None => album,
            }
        },
        small_image,
        small_text: match &style.playlist {
            Some(playlist) => format!("Playing from {playlist}"),
            None => crate::build_info::DISPLAY_NAME.to_string(),
        },
        buttons,
        start: timed.then_some(start),
        end: timed.then_some(start + i64::from(now.duration_ms / 1000)),
        join: (style.listen_along && timed && !is_file && !now.is_episode)
            .then(|| join_secret(&now.uri, start))
            .flatten(),
    })
}

/// The text a friend's Discord sends back when they press Join: the Spotify
/// song and the second (since 1970) it started, e.g. `spotify:track:abc@1700000000`.
/// Only a clean track address is ever put in, and only one is ever taken out.
pub fn join_secret(uri: &str, start: i64) -> Option<String> {
    let mut parts = uri.split(':');
    let (a, kind, id) = (parts.next()?, parts.next()?, parts.next()?);
    let clean = !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric());
    (a == "spotify" && kind == "track" && clean && parts.next().is_none())
        .then(|| format!("spotify:track:{id}@{start}"))
}

/// The song and start time inside a join secret, if it is one of ours.
pub fn parse_join(secret: &str) -> Option<(String, i64)> {
    let (uri, start) = secret.split_once('@')?;
    let start: i64 = start.parse().ok()?;
    let again = join_secret(uri, start)?;
    (again == secret).then(|| (uri.to_string(), start))
}

static LISTEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static JOINS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Whether to listen for friends' Join presses (Settings > Discord).
pub fn set_listen(on: bool) {
    LISTEN.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// A join secret a friend's press delivered, oldest first.
pub fn take_join() -> Option<String> {
    let mut joins = JOINS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    (!joins.is_empty()).then(|| joins.remove(0))
}

/// Keeps what an event frame carries: a press of Join.
fn note_event(frame: &Value) {
    if frame.get("evt").and_then(Value::as_str) != Some("ACTIVITY_JOIN") {
        return;
    }
    if let Some(secret) = frame.pointer("/data/secret").and_then(Value::as_str) {
        let mut joins = JOINS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if joins.len() < 4 && parse_join(secret).is_some() {
            joins.push(secret.to_string());
        }
    }
}

fn next_nonce() -> String {
    static NONCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NONCE
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        .to_string()
}

/// Reads until the answer to `nonce`, keeping any event met on the way.
fn read_answer(pipe: &mut dyn Pipe, nonce: &str) -> std::io::Result<Value> {
    for _ in 0..32 {
        let (opcode, frame) = read_frame(pipe)?;
        if opcode == 2 {
            return Err(std::io::Error::other("discord closed the pipe"));
        }
        if frame.get("nonce").and_then(Value::as_str) == Some(nonce) {
            return Ok(frame);
        }
        note_event(&frame);
    }
    Err(std::io::Error::other("no answer to the request"))
}

/// Asks Discord to tell us when a friend presses Join. Asked again every few
/// seconds it doubles as the moment the pipe is read for such news.
fn subscribe(pipe: &mut dyn Pipe) -> std::io::Result<()> {
    let nonce = next_nonce();
    let payload = json!({ "cmd": "SUBSCRIBE", "evt": "ACTIVITY_JOIN", "args": {}, "nonce": nonce });
    write_frame(pipe, 1, &payload)?;
    read_answer(pipe, &nonce)?;
    Ok(())
}

/// chanceify's own Discord application, so Rich Presence works with nothing to
/// set up. (An Application ID is public, not a secret.) A listener can still
/// put their own in Settings.
pub const DEFAULT_APPLICATION_ID: &str = "1557925208906661998";

/// How the link to the Discord app stands: 0 not found (or not tried yet),
/// 1 found, 2 found and the song was sent. For the status line in Settings.
static LINK: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// The chanceify song page (worker/embed.js). Empty until it is deployed.
pub const EMBED_URL: &str = "https://chanceify-embed.chance-a10.workers.dev";

/// The link "Copy the song for Discord" gives out for a Spotify track uri.
pub fn song_link(uri: &str, title: &str, artist: &str) -> Option<String> {
    let id = uri.strip_prefix("spotify:track:")?;
    if EMBED_URL.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    let enc = |text: &str| -> String {
        text.bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
                _ => format!("%{b:02X}"),
            })
            .collect()
    };
    Some(format!("{}/t/{id}?t={}&a={}", EMBED_URL.trim_end_matches('/'), enc(title), enc(artist)))
}

/// What Discord last said when it refused the song, or why the pipe failed,
/// for the Settings page (empty when all is well).
static LAST_ERROR: Mutex<String> = Mutex::new(String::new());

fn note_error(text: String) {
    *LAST_ERROR.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = text;
}

pub fn last_error() -> String {
    LAST_ERROR.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
}

pub fn link_state() -> u8 {
    LINK.load(std::sync::atomic::Ordering::Relaxed)
}

enum Message {
    Set(String, Option<Activity>),
}

static WORKER: Mutex<Option<mpsc::Sender<Message>>> = Mutex::new(None);
static LAST: Mutex<Option<(String, Option<Activity>)>> = Mutex::new(None);

/// Shows `activity` (or clears it when `None`) for the Discord application
/// `client_id`. An empty ID disconnects. Asking for what is already shown
/// does nothing, so this is cheap to call every frame.
pub fn update(client_id: &str, activity: Option<Activity>) {
    let client_id = client_id.trim();
    {
        let mut last = LAST.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let unchanged = match last.as_ref() {
            Some((id, held)) if id == client_id => match (held, &activity) {
                (Some(a), Some(b)) => a.same_as(b),
                (None, None) => true,
                _ => false,
            },
            // Nothing ever sent, and nothing to say.
            None => client_id.is_empty() && activity.is_none(),
            _ => false,
        };
        if unchanged {
            return;
        }
        *last = Some((client_id.to_string(), activity.clone()));
    }
    let mut worker = WORKER.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if worker.is_none() {
        let (tx, rx) = mpsc::channel();
        if std::thread::Builder::new()
            .name("discord-presence".into())
            .spawn(move || run(rx))
            .is_err()
        {
            return;
        }
        *worker = Some(tx);
    }
    if let Some(tx) = worker.as_ref() {
        let _ = tx.send(Message::Set(client_id.to_string(), activity));
    }
}

trait Pipe: Read + Write + Send {}
impl<T: Read + Write + Send> Pipe for T {}

fn run(rx: mpsc::Receiver<Message>) {
    let mut connection: Option<Box<dyn Pipe>> = None;
    let mut connected_as = String::new();
    let mut pending: Option<(String, Option<Activity>)> = None;
    loop {
        let listening = LISTEN.load(std::sync::atomic::Ordering::Relaxed);
        let wait = if listening && connection.is_some() { 4 } else if connection.is_none() { 5 } else { 12 };
        match rx.recv_timeout(Duration::from_secs(wait)) {
            Ok(Message::Set(id, activity)) => pending = Some((id, activity)),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        // Only the newest matters.
        while let Ok(Message::Set(id, activity)) = rx.try_recv() {
            pending = Some((id, activity));
        }
        let Some((id, activity)) = pending.clone() else {
            if listening && let Some(pipe) = connection.as_mut() {
                if let Err(error) = subscribe(pipe.as_mut()) {
                    log::debug!("discord listen-along: {error}");
                    connection = None;
                }
            }
            continue;
        };
        if id.is_empty() {
            connection = None;
            connected_as.clear();
            pending = None;
            continue;
        }
        if connection.is_none() || connected_as != id {
            connection = connect(&id);
            connected_as = id.clone();
            LINK.store(u8::from(connection.is_some()), std::sync::atomic::Ordering::Relaxed);
            if listening && let Some(pipe) = connection.as_mut() {
                if let Err(error) = subscribe(pipe.as_mut()) {
                    log::debug!("discord listen-along: {error}");
                    connection = None;
                }
            }
        }
        let Some(pipe) = connection.as_mut() else {
            // Discord is not open; the next timeout tries again.
            LINK.store(0, std::sync::atomic::Ordering::Relaxed);
            continue;
        };
        let sent = match set_activity(pipe.as_mut(), activity.as_ref(), false) {
            // Discord did not take the whole thing (an older Discord may not
            // know a newer field): say it again with only the basics.
            Ok(false) => set_activity(pipe.as_mut(), activity.as_ref(), true),
            other => other,
        };
        match sent {
            Ok(taken) => {
                pending = None;
                LINK.store(2, std::sync::atomic::Ordering::Relaxed);
                if taken {
                    note_error(String::new());
                }
            }
            Err(error) => {
                log::debug!("discord presence: {error}");
                note_error(format!("the link to Discord broke ({error}); trying again"));
                connection = None;
                LINK.store(0, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }
}

fn connect(client_id: &str) -> Option<Box<dyn Pipe>> {
    for number in 0..10 {
        let Some(mut pipe) = open_pipe(number) else {
            continue;
        };
        let hello = json!({ "v": 1, "client_id": client_id });
        if write_frame(pipe.as_mut(), 0, &hello).is_err() {
            continue;
        }
        match read_frame(pipe.as_mut()) {
            Ok((_, answer)) if answer.get("evt").and_then(Value::as_str) == Some("READY") => {
                return Some(pipe);
            }
            Ok((_, answer)) => {
                log::debug!("discord presence: not accepted: {answer}");
                note_error(format!("Discord did not accept the application ID: {answer}"));
                return None;
            }
            Err(_) => continue,
        }
    }
    None
}

#[cfg(windows)]
fn open_pipe(number: u32) -> Option<Box<dyn Pipe>> {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(format!(r"\\.\pipe\discord-ipc-{number}"))
        .ok()
        .map(|file| Box::new(file) as Box<dyn Pipe>)
}

#[cfg(unix)]
fn open_pipe(number: u32) -> Option<Box<dyn Pipe>> {
    let mut bases: Vec<std::path::PathBuf> = Vec::new();
    for key in ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"] {
        if let Some(value) = std::env::var_os(key) {
            bases.push(value.into());
        }
    }
    bases.push("/tmp".into());
    for base in bases {
        for path in [
            base.join(format!("discord-ipc-{number}")),
            base.join(format!("app/com.discordapp.Discord/discord-ipc-{number}")),
            base.join(format!("snap.discord/discord-ipc-{number}")),
        ] {
            if let Ok(stream) = std::os::unix::net::UnixStream::connect(&path) {
                return Some(Box::new(stream));
            }
        }
    }
    None
}

#[cfg(not(any(windows, unix)))]
fn open_pipe(_number: u32) -> Option<Box<dyn Pipe>> {
    None
}

fn write_frame(pipe: &mut dyn Pipe, opcode: u32, payload: &Value) -> std::io::Result<()> {
    let body = payload.to_string().into_bytes();
    let mut frame = Vec::with_capacity(8 + body.len());
    frame.extend_from_slice(&opcode.to_le_bytes());
    frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
    frame.extend_from_slice(&body);
    pipe.write_all(&frame)?;
    pipe.flush()
}

fn read_frame(pipe: &mut dyn Pipe) -> std::io::Result<(u32, Value)> {
    let mut header = [0u8; 8];
    pipe.read_exact(&mut header)?;
    let opcode = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
    let length = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
    if length > 64 * 1024 {
        return Err(std::io::Error::other("an answer that large is not Discord's"));
    }
    let mut body = vec![0u8; length];
    pipe.read_exact(&mut body)?;
    let value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    Ok((opcode, value))
}

/// Discord wants two to 128 characters on a line.
fn line(text: &str) -> String {
    let mut text: String = text.chars().take(120).collect();
    if text.chars().count() < 2 {
        text.push('\u{200b}');
        text.push('\u{200b}');
    }
    text
}

/// The activity as Discord's JSON. `plain` leaves out everything but the two
/// lines, the times and the big picture.
fn activity_json(activity: &Activity, plain: bool) -> Value {
    let mut value = json!({
        // 2 is "Listening to".
        "type": 2,
        "details": line(&activity.details),
        "state": line(&activity.state),
    });
    let mut stamps = serde_json::Map::new();
    if let Some(start) = activity.start {
        stamps.insert("start".into(), json!(start));
    }
    if let Some(end) = activity.end {
        stamps.insert("end".into(), json!(end));
    }
    if !stamps.is_empty() {
        value["timestamps"] = Value::Object(stamps);
    }
    let mut assets = serde_json::Map::new();
    if let Some(image) = &activity.large_image {
        assets.insert("large_image".into(), json!(image));
        assets.insert("large_text".into(), json!(line(&activity.large_text)));
    }
    if !plain && let Some(image) = &activity.small_image {
        assets.insert("small_image".into(), json!(image));
        assets.insert("small_text".into(), json!(line(&activity.small_text)));
    }
    if !assets.is_empty() {
        value["assets"] = Value::Object(assets);
    }
    if !plain {
        value["status_display_type"] = json!(activity.status_display.min(2));
        if let Some(url) = &activity.details_url {
            value["details_url"] = json!(url);
        }
        if let Some(url) = &activity.state_url {
            value["state_url"] = json!(url);
        }
        if let Some(secret) = &activity.join {
            value["party"] = json!({
                "id": format!("chanceify-{}", std::process::id()),
                "size": [1, 10],
            });
            value["secrets"] = json!({ "join": secret });
        }
        if !activity.buttons.is_empty() {
            let buttons: Vec<Value> = activity
                .buttons
                .iter()
                .take(2)
                .map(|(label, url)| {
                    json!({
                        "label": label.chars().take(32).collect::<String>(),
                        "url": url.chars().take(512).collect::<String>(),
                    })
                })
                .collect();
            value["buttons"] = Value::Array(buttons);
        }
    }
    value
}

/// Sends the activity. `Ok(true)` is Discord taking it, `Ok(false)` Discord
/// answering with an error.
fn set_activity(
    pipe: &mut dyn Pipe,
    activity: Option<&Activity>,
    plain: bool,
) -> std::io::Result<bool> {
    let nonce = next_nonce();
    let payload = json!({
        "cmd": "SET_ACTIVITY",
        "args": {
            "pid": std::process::id(),
            "activity": activity.map(|activity| activity_json(activity, plain)),
        },
        "nonce": nonce,
    });
    write_frame(pipe, 1, &payload)?;
    // Discord answers every request; reading it keeps the pipe from filling.
    let nonce = payload["nonce"].as_str().unwrap_or_default().to_string();
    let answer = read_answer(pipe, &nonce)?;
    let refused = answer.get("evt").and_then(Value::as_str) == Some("ERROR");
    if refused {
        log::debug!("discord presence: refused: {answer}");
        note_error(format!("Discord refused the song: {}", answer.get("data").and_then(|d| d.get("message")).and_then(Value::as_str).unwrap_or("no reason given")));
    }
    Ok(!refused)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song() -> Activity {
        Activity {
            details: "Song".into(),
            state: "Artist".into(),
            details_url: Some("https://open.spotify.com/track/abc".into()),
            state_url: Some("https://open.spotify.com/artist/def".into()),
            status_display: 2,
            large_image: Some("https://i.scdn.co/image/x".into()),
            large_text: "Album".into(),
            small_image: Some(BADGE_KEY.into()),
            small_text: "chanceify".into(),
            buttons: vec![("Listen on Spotify".into(), "https://open.spotify.com/track/abc".into())],
            start: Some(1_000),
            end: Some(1_200),
            join: None,
        }
    }

    fn playing() -> crate::app::NowPlaying {
        crate::app::NowPlaying {
            local: true,
            device_name: None,
            uri: "spotify:track:abc123".into(),
            id: Some("abc123".into()),
            title: "Song".into(),
            artists: vec![crate::api::models::ArtistRef {
                id: Some("def456".into()),
                name: "Artist".into(),
                ..Default::default()
            }],
            subtitle: "Artist".into(),
            album_name: "Album".into(),
            album_id: None,
            show_id: None,
            art_url: Some("https://i.scdn.co/image/x".into()),
            art_small: None,
            duration_ms: 200_000,
            position_ms: 50_000,
            playing: true,
            loading: false,
            shuffle: false,
            repeat: Default::default(),
            volume_percent: 50,
            can_control: true,
            can_set_volume: true,
            is_episode: false,
            resuming: false,
        }
    }

    fn style() -> Style {
        Style {
            status_line: 0,
            cover: true,
            badge: true,
            swirl: None,
            playlist: None,
            profile_url: None,
            playlist_url: None,
            song_page: false,
            look: 0,
            buttons: true,
            links: true,
            hide_paused: false,
            files: true,
            listen_along: false,
            app_url: Some("https://github.com/someone/chanceify".into()),
        }
    }

    #[test]
    fn playlist_and_profile_show_when_asked() {
        let mut chosen = style();
        chosen.playlist = Some("Road trip".into());
        chosen.profile_url = Some("https://open.spotify.com/user/abc".into());
        let activity = activity_for(&playing(), &chosen, 0).unwrap();
        assert!(activity.large_text.ends_with("from Road trip"));
        assert_eq!(activity.small_text, "Playing from Road trip");
        assert_eq!(activity.buttons.len(), 2);
        assert_eq!(activity.buttons[1].0, "My Spotify profile");
    }

    #[test]
    fn the_swirl_follows_the_colour_of_the_cover() {
        assert!(swirl_url(255, 0, 0).ends_with("swirl-0.png"));
        assert!(swirl_url(0, 255, 0).ends_with("swirl-4.png"));
        assert!(swirl_url(0, 0, 255).ends_with("swirl-8.png"));
        assert!(swirl_url(128, 128, 128).ends_with("swirl-gray.png"));
        assert!(swirl_url(5, 5, 5).ends_with("swirl-gray.png"));
    }

    #[test]
    fn the_activity_becomes_discords_json() {
        let value = activity_json(&song(), false);
        assert_eq!(value["type"], 2);
        assert_eq!(value["details"], "Song");
        assert_eq!(value["timestamps"]["end"], 1_200);
        assert_eq!(value["assets"]["large_text"], "Album");
        assert_eq!(value["assets"]["small_image"], BADGE_KEY);
        assert_eq!(value["status_display_type"], 2);
        assert_eq!(value["details_url"], "https://open.spotify.com/track/abc");
        assert_eq!(value["buttons"][0]["label"], "Listen on Spotify");
    }

    #[test]
    fn the_plain_version_has_only_the_basics() {
        let value = activity_json(&song(), true);
        assert_eq!(value["details"], "Song");
        assert!(value["assets"].get("small_image").is_none());
        assert!(value.get("buttons").is_none());
        assert!(value.get("status_display_type").is_none());
        assert!(value.get("details_url").is_none());
    }

    #[test]
    fn a_spotify_song_leads_with_the_music() {
        let activity = activity_for(&playing(), &style(), 10_000).unwrap();
        assert_eq!(activity.details, "Song");
        assert_eq!(activity.state, "Artist");
        assert_eq!(activity.status_display, 2);
        assert_eq!(activity.large_image.as_deref(), Some("https://i.scdn.co/image/x"));
        assert_eq!(activity.small_image.as_deref(), Some(BADGE_KEY));
        assert_eq!(activity.buttons.len(), 2);
        assert_eq!(activity.buttons[0].1, "https://open.spotify.com/track/abc123");
        assert_eq!(
            activity.state_url.as_deref(),
            Some("https://open.spotify.com/artist/def456")
        );
        assert_eq!(activity.start, Some(10_000 - 50));
        assert_eq!(activity.end, Some(10_000 - 50 + 200));
    }

    #[test]
    fn the_status_line_follows_the_choice() {
        let mut chosen = style();
        chosen.status_line = 1;
        assert_eq!(activity_for(&playing(), &chosen, 0).unwrap().status_display, 1);
        chosen.status_line = 2;
        assert_eq!(activity_for(&playing(), &chosen, 0).unwrap().status_display, 0);
    }

    #[test]
    fn a_file_has_no_spotify_links_and_no_fetchable_cover() {
        let mut file = playing();
        file.uri = "local:file:0123456789abcdef".into();
        file.art_url = Some("https://local.chanceify.invalid/cover/0123456789abcdef".into());
        let activity = activity_for(&file, &style(), 0).unwrap();
        assert_eq!(activity.details_url, None);
        assert_eq!(activity.state_url, None);
        // Only the app's own button, and the badge stands in for the cover.
        assert_eq!(activity.buttons.len(), 1);
        assert_eq!(activity.large_image.as_deref(), Some(BADGE_KEY));
        assert_eq!(activity.small_image, None);
        let mut hidden = style();
        hidden.files = false;
        assert!(activity_for(&file, &hidden, 0).is_none());
    }

    #[test]
    fn paused_can_be_hidden_and_nothing_resumed_is_shown() {
        let mut paused = playing();
        paused.playing = false;
        let shown = activity_for(&paused, &style(), 0).unwrap();
        assert_eq!(shown.state, "Artist (paused)");
        assert_eq!(shown.start, None);
        let mut hide = style();
        hide.hide_paused = true;
        assert!(activity_for(&paused, &hide, 0).is_none());
        let mut resumed = playing();
        resumed.resuming = true;
        assert!(activity_for(&resumed, &style(), 0).is_none());
    }

    #[test]
    fn web_addresses_come_only_from_clean_spotify_ids() {
        assert_eq!(
            web_url("spotify:episode:Ab12").as_deref(),
            Some("https://open.spotify.com/episode/Ab12")
        );
        assert_eq!(web_url("spotify:playlist:x"), None);
        assert_eq!(web_url("local:file:abc"), None);
        assert_eq!(web_url("spotify:track:a/b"), None);
    }

    #[test]
    fn a_join_secret_holds_only_a_clean_track_and_a_time() {
        assert_eq!(
            join_secret("spotify:track:abc123", 42).as_deref(),
            Some("spotify:track:abc123@42")
        );
        assert_eq!(join_secret("spotify:episode:abc", 1), None);
        assert_eq!(join_secret("local:file:abc", 1), None);
        assert_eq!(join_secret("spotify:track:a/b", 1), None);
        assert_eq!(
            parse_join("spotify:track:abc123@42"),
            Some(("spotify:track:abc123".to_string(), 42))
        );
        assert_eq!(parse_join("spotify:track:abc@x"), None);
        assert_eq!(parse_join("https://evil.example@1"), None);
        assert_eq!(parse_join("spotify:track:a:b@1"), None);
    }

    #[test]
    fn listen_along_adds_a_party_and_a_secret_only_when_asked() {
        let mut chosen = style();
        assert_eq!(activity_for(&playing(), &chosen, 10_000).unwrap().join, None);
        chosen.listen_along = true;
        let activity = activity_for(&playing(), &chosen, 10_000).unwrap();
        assert_eq!(activity.join.as_deref(), Some("spotify:track:abc123@9950"));
        let value = activity_json(&activity, false);
        assert_eq!(value["secrets"]["join"], "spotify:track:abc123@9950");
        assert_eq!(value["party"]["size"][0], 1);
        assert!(activity_json(&activity, true).get("secrets").is_none());
        let mut file = playing();
        file.uri = "local:file:0123456789abcdef".into();
        assert_eq!(activity_for(&file, &chosen, 0).unwrap().join, None);
    }

    #[test]
    fn short_lines_are_padded_and_long_ones_cut() {
        assert_eq!(line("a").chars().count(), 3);
        assert_eq!(line(&"x".repeat(300)).chars().count(), 120);
    }

    #[test]
    fn a_drifting_progress_bar_is_not_a_new_activity() {
        let mut moved = song();
        moved.start = Some(1_002);
        moved.end = Some(1_201);
        assert!(song().same_as(&moved));
        moved.start = Some(1_100);
        assert!(!song().same_as(&moved));
    }

    #[test]
    fn frames_are_eight_bytes_of_header_then_json() {
        struct Sink(Vec<u8>);
        impl Read for Sink {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Ok(0)
            }
        }
        impl Write for Sink {
            fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
                self.0.extend_from_slice(data);
                Ok(data.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut sink = Sink(Vec::new());
        write_frame(&mut sink, 0, &json!({"v": 1})).unwrap();
        assert_eq!(&sink.0[..4], &0u32.to_le_bytes());
        assert_eq!(&sink.0[4..8], &(sink.0.len() as u32 - 8).to_le_bytes());
        assert_eq!(&sink.0[8..], br#"{"v":1}"#);
    }
}
