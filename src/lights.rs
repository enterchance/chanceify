//! The optional "lights feed": a tiny JSON file with the colours the
//! visualizer is showing right now, for other programs (a keyboard or
//! microphone light app) to copy. Off by default. Nothing goes over the
//! network: it is one file, `lights.json`, in Chanceify's data folder,
//! replaced whole (written aside, then renamed) so a reader never sees half
//! of it.

use std::path::PathBuf;
use std::sync::mpsc::{Sender, channel};
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct Feed {
    tx: Option<Sender<String>>,
    path: Option<PathBuf>,
    last_body: String,
    last_sent: Option<Instant>,
    on: bool,
}

impl Feed {
    pub fn path_for(state: &std::path::Path) -> PathBuf {
        state.join("lights.json")
    }

    /// Sends `body` (the JSON without its stamp) when it changed, at most
    /// twenty times a second, and at least every five seconds as a sign of life.
    pub fn publish(&mut self, state: &std::path::Path, body: serde_json::Value) {
        let path = Self::path_for(state);
        let tx = self.tx.get_or_insert_with(|| {
            let (tx, rx) = channel::<String>();
            let target = path.clone();
            std::thread::spawn(move || {
                while let Ok(mut text) = rx.recv() {
                    // Only the newest matters.
                    while let Ok(newer) = rx.try_recv() {
                        text = newer;
                    }
                    let temp = target.with_extension("json.tmp");
                    if let Some(parent) = target.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    if std::fs::write(&temp, text).is_ok() {
                        let _ = std::fs::rename(&temp, &target);
                    }
                }
            });
            tx
        });
        self.path = Some(path);
        self.on = true;
        let key = body.to_string();
        let due = self.last_sent.is_none_or(|at| at.elapsed() >= Duration::from_millis(50));
        let alive = self.last_sent.is_none_or(|at| at.elapsed() >= Duration::from_secs(5));
        if !((key != self.last_body && due) || alive) {
            return;
        }
        let mut body = body;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64);
        if let Some(map) = body.as_object_mut() {
            map.insert("ts".into(), stamp.into());
        }
        self.last_body = key;
        self.last_sent = Some(Instant::now());
        let _ = tx.send(body.to_string());
    }

    /// The feed was switched off: the file goes, so a reader sees it stop.
    pub fn stop(&mut self) {
        if self.on {
            self.on = false;
            if let Some(path) = &self.path {
                let _ = std::fs::remove_file(path);
            }
            self.last_body.clear();
            self.last_sent = None;
        }
    }
}

pub fn hex(colour: egui::Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", colour.r(), colour.g(), colour.b())
}
