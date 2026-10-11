//! Last.fm: signing in, telling it what is playing, and scrobbling.
//!
//! Everything that talks to Last.fm happens on one thread of its own, so a
//! slow or missing network never holds up the window. The window sends jobs
//! down a channel and reads a small snapshot ([`View`]) back.
//!
//! Scrobbles are written to a file before they are sent and taken out of it
//! once Last.fm has them, so a song played with no network, or with the
//! program closed a moment later, is still counted next time.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

const API_URL: &str = "https://ws.audioscrobbler.com/2.0/";

/// A Cloudflare Worker of your own (see `worker/lastfm-proxy.js`). It holds
/// the Last.fm key and secret, so they are never inside this program or on
/// GitHub. The address is read at build time from the untracked
/// `lastfm-proxy.txt`; the source ships with none. Empty means no proxy: the
/// key from `lastfm-keys.txt` is used, or Settings asks for one of your own.
pub const PROXY_URL: &str = match option_env!("CHANCEIFY_LASTFM_PROXY") {
    Some(address) => address,
    None => "",
};

/// Stands in for the key and secret while the proxy does the signing.
pub const VIA_PROXY: &str = "via-proxy";

/// What every copy of chanceify™ signs in with: the proxy if there is one,
/// else a key and secret built in from the untracked `lastfm-keys.txt`.
pub const DEFAULT_API_KEY: &str = if !PROXY_URL.is_empty() {
    VIA_PROXY
} else {
    match option_env!("CHANCEIFY_LASTFM_KEY") {
        Some(key) => key,
        None => "",
    }
};
pub const DEFAULT_SECRET: &str = if !PROXY_URL.is_empty() {
    VIA_PROXY
} else {
    match option_env!("CHANCEIFY_LASTFM_SECRET") {
        Some(secret) => secret,
        None => "",
    }
};

/// A song worth telling Last.fm about.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Track {
    pub artist: String,
    pub title: String,
    #[serde(default)]
    pub album: String,
    #[serde(default)]
    pub duration_s: u32,
}

/// A song played, waiting to be counted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Pending {
    track: Track,
    /// When it started, in seconds since 1970.
    at: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    SignedOut,
    /// The browser is open on Last.fm's page; waiting for "Allow".
    WaitingForBrowser,
    SignedIn,
    Failed(String),
}

/// What the window can see of Last.fm.
#[derive(Clone, Debug, Default)]
pub struct View {
    pub status: Status,
    pub user: String,
    /// How many songs the account has scrobbled, once asked.
    pub scrobbles: Option<u64>,
    /// Songs saved to send when the network is back.
    pub queued: usize,
    /// The page the person is asked to allow chanceify™ on, while waiting.
    pub auth_url: Option<String>,
}

#[derive(Default)]
struct Shared {
    view: View,
    /// A new sign-in: the user name and session key to keep.
    new_session: Option<(String, String)>,
    notices: Vec<String>,
}

enum Job {
    Configure {
        key: String,
        secret: String,
        session: String,
        user: String,
        queue_file: PathBuf,
    },
    SignIn,
    CancelSignIn,
    NowPlaying(Track),
    Scrobble(Track, i64),
    Love(Track, bool),
    Refresh,
    Ping,
    Quit,
}

/// The handle the window keeps. The thread starts the first time it is
/// needed, so a program that never uses Last.fm never runs it.
#[derive(Default)]
pub struct LastFm {
    shared: Arc<Mutex<Shared>>,
    tx: Mutex<Option<Sender<Job>>>,
}

impl LastFm {
    fn send(&self, job: Job) {
        let mut tx = self.tx.lock().unwrap_or_else(PoisonError::into_inner);
        if tx.is_none() {
            let (sender, receiver) = mpsc::channel();
            let shared = Arc::clone(&self.shared);
            let started = std::thread::Builder::new()
                .name("lastfm".into())
                .spawn(move || Worker::new(receiver, shared).run());
            if started.is_err() {
                log::error!("could not start the Last.fm thread");
                return;
            }
            *tx = Some(sender);
        }
        if let Some(sender) = tx.as_ref() {
            let _ = sender.send(job);
        }
    }

    pub fn view(&self) -> View {
        self.shared
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .view
            .clone()
    }

    /// The user name and session key of a sign-in that just finished, once.
    pub fn take_new_session(&self) -> Option<(String, String)> {
        self.shared
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .new_session
            .take()
    }

    /// Messages for the person (a song loved, a sign-in that ran out), once.
    pub fn take_notices(&self) -> Vec<String> {
        std::mem::take(
            &mut self
                .shared
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .notices,
        )
    }

    pub fn configure(
        &self,
        key: &str,
        secret: &str,
        session: &str,
        user: &str,
        queue_file: PathBuf,
    ) {
        self.send(Job::Configure {
            key: key.trim().to_string(),
            secret: secret.trim().to_string(),
            session: session.to_string(),
            user: user.to_string(),
            queue_file,
        });
    }

    /// Asks the worker (or Last.fm) a harmless question and says how it went.
    pub fn ping(&self) {
        self.send(Job::Ping);
    }

    pub fn sign_in(&self) {
        self.send(Job::SignIn);
    }

    pub fn cancel_sign_in(&self) {
        self.send(Job::CancelSignIn);
    }

    pub fn now_playing(&self, track: Track) {
        self.send(Job::NowPlaying(track));
    }

    pub fn scrobble(&self, track: Track, started_at: i64) {
        self.send(Job::Scrobble(track, started_at));
    }

    pub fn love(&self, track: Track, love: bool) {
        self.send(Job::Love(track, love));
    }

    pub fn refresh(&self) {
        self.send(Job::Refresh);
    }
}

impl Drop for LastFm {
    fn drop(&mut self) {
        let tx = self.tx.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(sender) = tx.as_ref() {
            let _ = sender.send(Job::Quit);
        }
    }
}

// ---- signing -------------------------------------------------------------

/// The MD5 of `data`, as 32 lowercase hex digits. Last.fm signs every
/// request with it. Written out here because nothing else needs it.
pub fn md5_hex(data: &[u8]) -> String {
    const SHIFTS: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    let constants: Vec<u32> = (0..64u32)
        .map(|i| ((f64::from(i) + 1.0).sin().abs() * 4_294_967_296.0) as u32)
        .collect();
    let (mut a0, mut b0, mut c0, mut d0) = (
        0x6745_2301u32,
        0xefcd_ab89u32,
        0x98ba_dcfeu32,
        0x1032_5476u32,
    );
    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&(data.len() as u64).wrapping_mul(8).to_le_bytes());
    for chunk in message.chunks_exact(64) {
        let words: Vec<u32> = chunk
            .chunks_exact(4)
            .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
            .collect();
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64usize {
            let (mut f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            f = f
                .wrapping_add(a)
                .wrapping_add(constants[i])
                .wrapping_add(words[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(SHIFTS[i]));
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }
    let mut out = String::with_capacity(32);
    for word in [a0, b0, c0, d0] {
        for byte in word.to_le_bytes() {
            use std::fmt::Write;
            let _ = write!(out, "{byte:02x}");
        }
    }
    out
}

/// A request's signature: the parameters in name order, name and value run
/// together, the secret on the end, all through MD5. `format` and `callback`
/// are never part of it.
pub fn signature(params: &[(String, String)], secret: &str) -> String {
    let mut sorted: Vec<&(String, String)> = params
        .iter()
        .filter(|(name, _)| name != "format" && name != "callback")
        .collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut text = String::new();
    for (name, value) in sorted {
        text.push_str(name);
        text.push_str(value);
    }
    text.push_str(secret);
    md5_hex(text.as_bytes())
}

// ---- the thread ----------------------------------------------------------

/// What came back from Last.fm.
enum Reply {
    Ok(serde_json::Value),
    /// Last.fm answered with an error code and message.
    Api(i64, String),
    /// No answer: the network, or a reply that was not Last.fm's.
    Network(String),
}

struct Worker {
    rx: Receiver<Job>,
    shared: Arc<Mutex<Shared>>,
    http: Option<reqwest::blocking::Client>,
    key: String,
    secret: String,
    session: String,
    user: String,
    queue_file: PathBuf,
    queue: Vec<Pending>,
    /// While signing in: the token and when to give up.
    token: Option<(String, Instant)>,
    last_poll: Instant,
    last_flush: Instant,
}

/// How long a sign-in waits for "Allow" before it gives up.
const SIGN_IN_PATIENCE: Duration = Duration::from_secs(5 * 60);

/// Songs sent in one request. Last.fm takes up to 50.
const BATCH: usize = 50;

impl Worker {
    fn new(rx: Receiver<Job>, shared: Arc<Mutex<Shared>>) -> Self {
        Self {
            rx,
            shared,
            http: None,
            key: String::new(),
            secret: String::new(),
            session: String::new(),
            user: String::new(),
            queue_file: PathBuf::new(),
            queue: Vec::new(),
            token: None,
            last_poll: Instant::now(),
            last_flush: Instant::now(),
        }
    }

    fn run(mut self) {
        loop {
            match self.rx.recv_timeout(Duration::from_secs(2)) {
                Ok(Job::Quit) | Err(RecvTimeoutError::Disconnected) => return,
                Ok(job) => self.handle(job),
                Err(RecvTimeoutError::Timeout) => {}
            }
            self.poll_sign_in();
            if !self.queue.is_empty()
                && !self.session.is_empty()
                && self.last_flush.elapsed() >= Duration::from_secs(60)
            {
                self.flush();
            }
        }
    }

    fn with_view(&self, change: impl FnOnce(&mut View)) {
        let mut shared = self.shared.lock().unwrap_or_else(PoisonError::into_inner);
        change(&mut shared.view);
    }

    fn notice(&self, text: impl Into<String>) {
        self.shared
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .notices
            .push(text.into());
    }

    fn configured(&self) -> bool {
        !self.key.is_empty() && !self.secret.is_empty()
    }

    fn handle(&mut self, job: Job) {
        match job {
            Job::Quit => {}
            Job::Configure {
                key,
                secret,
                session,
                user,
                queue_file,
            } => {
                let first = self.queue_file != queue_file;
                self.key = key;
                self.secret = secret;
                self.session = session;
                self.user = user;
                if first {
                    self.queue_file = queue_file;
                    self.queue = std::fs::read(&self.queue_file)
                        .ok()
                        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                        .unwrap_or_default();
                }
                let signed_in = !self.session.is_empty() && self.configured();
                let waiting = self.token.is_some();
                let user = self.user.clone();
                let queued = self.queue.len();
                self.with_view(|view| {
                    view.queued = queued;
                    if signed_in {
                        view.status = Status::SignedIn;
                        view.user = user;
                    } else if !waiting {
                        view.status = Status::SignedOut;
                        view.user.clear();
                        view.scrobbles = None;
                    }
                });
                if signed_in && !self.queue.is_empty() {
                    self.flush();
                }
            }
            Job::SignIn => self.sign_in(),
            Job::CancelSignIn => {
                self.token = None;
                self.with_view(|view| {
                    view.status = Status::SignedOut;
                    view.auth_url = None;
                });
            }
            Job::NowPlaying(track) => self.now_playing(&track),
            Job::Scrobble(track, at) => {
                self.queue.push(Pending { track, at });
                self.save_queue();
                self.flush();
            }
            Job::Love(track, love) => self.love(&track, love),
            Job::Refresh => self.refresh(),
            Job::Ping => self.ping(),
        }
    }

    fn client(&mut self) -> Option<&reqwest::blocking::Client> {
        if self.http.is_none() {
            self.http = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(20))
                .user_agent(concat!("chanceify/", env!("CARGO_PKG_VERSION")))
                .build()
                .ok();
        }
        self.http.as_ref()
    }

    /// Sends one signed request.
    fn call(&mut self, method: &str, extra: &[(&str, String)], signed_in: bool) -> Reply {
        let proxied = self.key == VIA_PROXY && !PROXY_URL.is_empty();
        let mut params: Vec<(String, String)> = vec![("method".into(), method.to_string())];
        if !proxied {
            params.push(("api_key".into(), self.key.clone()));
        }
        for (name, value) in extra {
            params.push(((*name).to_string(), value.clone()));
        }
        if signed_in {
            params.push(("sk".into(), self.session.clone()));
        }
        if !proxied {
            let api_sig = signature(&params, &self.secret);
            params.push(("api_sig".into(), api_sig));
        }
        params.push(("format".into(), "json".into()));
        let url = if proxied { PROXY_URL } else { API_URL };
        let Some(client) = self.client() else {
            return Reply::Network("could not set up the connection".into());
        };
        let response = match client.post(url).form(&params).send() {
            Ok(response) => response,
            Err(error) => return Reply::Network(error.to_string()),
        };
        let status = response.status();
        let body: serde_json::Value = match response.json() {
            Ok(body) => body,
            Err(error) => {
                return Reply::Network(format!("unreadable answer ({status}): {error}"));
            }
        };
        if let Some(code) = body.get("error").and_then(serde_json::Value::as_i64) {
            let message = body
                .get("message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("Last.fm said no")
                .to_string();
            return Reply::Api(code, message);
        }
        Reply::Ok(body)
    }

    /// The session has stopped working: sign out and say so.
    fn expired(&mut self) {
        self.session.clear();
        self.with_view(|view| {
            view.status = Status::Failed("Last.fm sign-in ran out. Sign in again.".into());
            view.user.clear();
        });
        self.notice("Last.fm sign-in ran out. Sign in again in Settings.");
        self.shared
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .new_session = Some((String::new(), String::new()));
    }

    fn sign_in(&mut self) {
        if !self.configured() {
            self.with_view(|view| {
                view.status = Status::Failed("Enter a Last.fm API key and secret first.".into());
            });
            return;
        }
        match self.call("auth.getToken", &[], false) {
            Reply::Ok(body) => {
                let Some(token) = body
                    .get("token")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
                else {
                    self.with_view(|view| {
                        view.status = Status::Failed("Last.fm gave no sign-in token.".into());
                    });
                    return;
                };
                // Through the proxy the public key comes back with the token.
                let public_key = body
                    .get("api_key")
                    .and_then(serde_json::Value::as_str)
                    .map_or_else(|| self.key.clone(), str::to_string);
                let url = format!(
                    "https://www.last.fm/api/auth/?api_key={public_key}&token={token}"
                );
                if let Err(error) = open::that(&url) {
                    log::warn!("could not open the browser: {error}");
                }
                self.token = Some((token, Instant::now()));
                self.last_poll = Instant::now();
                self.with_view(|view| {
                    view.status = Status::WaitingForBrowser;
                    view.auth_url = Some(url);
                });
            }
            Reply::Api(_, message) => self.with_view(|view| {
                view.status = Status::Failed(format!("Last.fm: {message}"));
            }),
            Reply::Network(message) => self.with_view(|view| {
                view.status = Status::Failed(format!("Could not reach Last.fm ({message})"));
            }),
        }
    }

    /// While the browser is open on Last.fm, asks every few seconds whether
    /// "Allow" has been pressed.
    fn poll_sign_in(&mut self) {
        let Some((token, since)) = self.token.clone() else {
            return;
        };
        if since.elapsed() > SIGN_IN_PATIENCE {
            self.token = None;
            self.with_view(|view| {
                view.status = Status::Failed("Sign-in timed out. Try again.".into());
                view.auth_url = None;
            });
            return;
        }
        if self.last_poll.elapsed() < Duration::from_secs(3) {
            return;
        }
        self.last_poll = Instant::now();
        match self.call("auth.getSession", &[("token", token)], false) {
            Reply::Ok(body) => {
                let session = body.get("session");
                let name = session
                    .and_then(|s| s.get("name"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let key = session
                    .and_then(|s| s.get("key"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string();
                self.token = None;
                if key.is_empty() {
                    self.with_view(|view| {
                        view.status = Status::Failed("Last.fm gave no session.".into());
                        view.auth_url = None;
                    });
                    return;
                }
                self.session = key.clone();
                self.user = name.clone();
                {
                    let mut shared = self.shared.lock().unwrap_or_else(PoisonError::into_inner);
                    shared.new_session = Some((name.clone(), key));
                    shared.view.status = Status::SignedIn;
                    shared.view.user = name.clone();
                    shared.view.auth_url = None;
                }
                self.notice(format!("Signed in to Last.fm as {name}"));
                self.refresh();
                if !self.queue.is_empty() {
                    self.flush();
                }
            }
            // 14: not allowed yet. Keep waiting.
            Reply::Api(14, _) => {}
            Reply::Api(15, _) => {
                self.token = None;
                self.with_view(|view| {
                    view.status = Status::Failed("Sign-in timed out. Try again.".into());
                    view.auth_url = None;
                });
            }
            Reply::Api(_, message) => {
                self.token = None;
                self.with_view(|view| {
                    view.status = Status::Failed(format!("Last.fm: {message}"));
                    view.auth_url = None;
                });
            }
            // No network for a moment: try again on the next round.
            Reply::Network(_) => {}
        }
    }

    fn refresh(&mut self) {
        if self.session.is_empty() || !self.configured() {
            return;
        }
        let user = self.user.clone();
        if let Reply::Ok(body) = self.call("user.getInfo", &[("user", user)], false) {
            let count = body
                .get("user")
                .and_then(|u| u.get("playcount"))
                .and_then(serde_json::Value::as_str)
                .and_then(|text| text.parse::<u64>().ok());
            self.with_view(|view| view.scrobbles = count);
        }
    }

    fn ping(&mut self) {
        if !self.configured() {
            self.notice("Last.fm has no key to use yet.");
            return;
        }
        let proxied = self.key == VIA_PROXY && !PROXY_URL.is_empty();
        let who = if proxied { "The chanceify worker" } else { "Last.fm" };
        match self.call("auth.getToken", &[], false) {
            Reply::Ok(_) => self.notice(format!("{who} answered. The Last.fm connection works.")),
            Reply::Api(code, message) => self.notice(format!(
                "{who} answered, but Last.fm said no ({code}: {message}). Check the key and secret."
            )),
            Reply::Network(message) => {
                self.notice(format!("Could not reach {who} ({message}). Check your internet."))
            }
        }
    }

    fn track_params(track: &Track) -> Vec<(&'static str, String)> {
        let mut params = vec![
            ("artist", track.artist.clone()),
            ("track", track.title.clone()),
        ];
        if !track.album.is_empty() {
            params.push(("album", track.album.clone()));
        }
        if track.duration_s > 0 {
            params.push(("duration", track.duration_s.to_string()));
        }
        params
    }

    fn now_playing(&mut self, track: &Track) {
        if self.session.is_empty() || !self.configured() {
            return;
        }
        let params = Self::track_params(track);
        if let Reply::Api(code, _) = self.call("track.updateNowPlaying", &params, true) {
            if code == 9 {
                self.expired();
            }
        }
    }

    fn love(&mut self, track: &Track, love: bool) {
        if self.session.is_empty() || !self.configured() {
            self.notice("Sign in to Last.fm in Settings first");
            return;
        }
        let params = vec![
            ("artist", track.artist.clone()),
            ("track", track.title.clone()),
        ];
        let method = if love { "track.love" } else { "track.unlove" };
        match self.call(method, &params, true) {
            Reply::Ok(_) => self.notice(if love {
                format!("Loved {} on Last.fm", track.title)
            } else {
                format!("Removed {} from your Last.fm loves", track.title)
            }),
            Reply::Api(9, _) => self.expired(),
            Reply::Api(_, message) => self.notice(format!("Last.fm: {message}")),
            Reply::Network(_) => self.notice("Could not reach Last.fm"),
        }
    }

    fn save_queue(&self) {
        if self.queue_file.as_os_str().is_empty() {
            return;
        }
        if let Some(parent) = self.queue_file.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        if let Ok(bytes) = serde_json::to_vec(&self.queue) {
            let temporary = self.queue_file.with_extension("json.tmp");
            if std::fs::write(&temporary, bytes).is_ok() {
                std::fs::rename(&temporary, &self.queue_file).ok();
            }
        }
        let queued = self.queue.len();
        self.with_view(|view| view.queued = queued);
    }

    /// Sends the songs waiting, in batches, until one fails.
    fn flush(&mut self) {
        self.last_flush = Instant::now();
        if self.session.is_empty() || !self.configured() {
            return;
        }
        while !self.queue.is_empty() {
            let count = self.queue.len().min(BATCH);
            let mut params: Vec<(String, String)> = Vec::new();
            for (i, pending) in self.queue[..count].iter().enumerate() {
                params.push((format!("artist[{i}]"), pending.track.artist.clone()));
                params.push((format!("track[{i}]"), pending.track.title.clone()));
                params.push((format!("timestamp[{i}]"), pending.at.to_string()));
                if !pending.track.album.is_empty() {
                    params.push((format!("album[{i}]"), pending.track.album.clone()));
                }
                if pending.track.duration_s > 0 {
                    params.push((
                        format!("duration[{i}]"),
                        pending.track.duration_s.to_string(),
                    ));
                }
            }
            let borrowed: Vec<(&str, String)> = params
                .iter()
                .map(|(name, value)| (name.as_str(), value.clone()))
                .collect();
            match self.call("track.scrobble", &borrowed, true) {
                Reply::Ok(_) => {
                    self.queue.drain(..count);
                    self.save_queue();
                    if let Some(total) = self.shared.lock().ok().and_then(|s| s.view.scrobbles) {
                        let total = total + count as u64;
                        self.with_view(|view| view.scrobbles = Some(total));
                    }
                }
                Reply::Api(9, _) => {
                    self.expired();
                    return;
                }
                // Busy or mistaken for a moment: keep them and try later.
                Reply::Api(11 | 16, _) | Reply::Network(_) => return,
                // Anything else is a request Last.fm will never take, so
                // dropping it keeps one bad song from blocking the rest.
                Reply::Api(code, message) => {
                    log::warn!("Last.fm refused {count} scrobbles ({code}): {message}");
                    self.queue.drain(..count);
                    self.save_queue();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md5_matches_the_published_answers() {
        assert_eq!(md5_hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(
            md5_hex(b"The quick brown fox jumps over the lazy dog"),
            "9e107d9d372bb6826bd81d3542a419d6"
        );
        assert_eq!(
            md5_hex(b"12345678901234567890123456789012345678901234567890123456789012345678901234567890"),
            "57edf4a22be3c955ac49da2e2107b67a"
        );
    }

    #[test]
    fn a_signature_sorts_the_names_and_skips_format() {
        let params = vec![
            ("method".to_string(), "auth.getToken".to_string()),
            ("api_key".to_string(), "xxx".to_string()),
            ("format".to_string(), "json".to_string()),
        ];
        // api_keyxxxmethodauth.getTokenSECRET
        assert_eq!(
            signature(&params, "SECRET"),
            md5_hex(b"api_keyxxxmethodauth.getTokenSECRET")
        );
    }

    #[test]
    fn a_handle_that_is_never_used_starts_no_thread() {
        let lastfm = LastFm::default();
        assert_eq!(lastfm.view().status, Status::SignedOut);
        assert!(lastfm.take_new_session().is_none());
        assert!(lastfm.take_notices().is_empty());
    }
}
