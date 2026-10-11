//! The guided tour: a short list of steps, each pointing at a part of the
//! window. The list lives in `index/tour.json`, so it can be edited in
//! Settings (the tour builder) or by hand, and moves with the index folder.

use serde::{Deserialize, Serialize};

/// Parts of the window a step can point at.
pub const ANCHORS: &[(&str, &str)] = &[
    ("none", "Nothing (centre card)"),
    ("top", "Top bar"),
    ("sidebar", "Library sidebar"),
    ("account", "Your account button"),
    ("player", "Player bar controls"),
    ("controls", "Play buttons"),
    ("seek", "Song-length bar"),
    ("volume", "Volume"),
    ("visual", "Visualizer"),
    ("bottom", "Whole bottom strip"),
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub title: String,
    pub text: String,
    #[serde(default = "none")]
    pub anchor: String,
    /// What a right click there offers. The tour shows a pretend cursor
    /// right-clicking and stepping through these, so the menu can be seen
    /// without anyone having to open it.
    #[serde(default)]
    pub menu: Vec<String>,
}

fn none() -> String {
    "none".into()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tour {
    /// Bumped when the built-in tour is rewritten, so an older saved copy is
    /// replaced by the new one instead of shadowing it.
    #[serde(default)]
    pub version: u32,
    pub steps: Vec<Step>,
}

/// The version of the built-in tour.
const VERSION: u32 = 2;

impl Default for Tour {
    fn default() -> Self {
        let step = |title: &str, text: &str, anchor: &str, menu: &[&str]| Step {
            title: title.into(),
            text: text.into(),
            anchor: anchor.into(),
            menu: menu.iter().map(|item| (*item).to_string()).collect(),
        };
        Self {
            version: VERSION,
            steps: vec![
                step(
                    "welcome to chanceify™",
                    "music reimagined\nwatch what a right click does",
                    "none",
                    &[],
                ),
                step(
                    "library",
                    "right click the library .",
                    "sidebar",
                    &[
                        "playlists",
                        "albums",
                        "artists",
                        "podcasts",
                        "sort personal",
                        "sort recent",
                        "sort alphabetical",
                        "grid or list",
                    ],
                ),
                step(
                    "songs",
                    "right click any song .",
                    "none",
                    &[
                        "play next",
                        "add to queue",
                        "add to playlist",
                        "go to artist",
                        "go to album",
                        "save to index",
                    ],
                ),
                step(
                    "player bar",
                    "right click the empty space .",
                    "player",
                    &["move the buttons", "song bar length", "time style", "bar height"],
                ),
                step(
                    "volume",
                    "right click the volume .",
                    "volume",
                    &["speed", "equalizer curves"],
                ),
                step(
                    "visualizer",
                    "right click it , or push g .",
                    "visual",
                    &["bars", "flow", "swirl", "colours", "looks", "full-screen"],
                ),
                step(
                    "lyrics",
                    "right click the lyrics page .",
                    "none",
                    &[
                        "album art",
                        "floating art",
                        "timestamps",
                        "countdown",
                        "left or right",
                        "focus bottom",
                        "focus middle",
                        "focus top",
                    ],
                ),
                step(
                    "keys",
                    "z x c switch views .\npush t for every key .",
                    "none",
                    &[],
                ),
            ],
        }
    }
}

impl Tour {
    pub fn load(path: &std::path::Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Tour>(&bytes).ok())
            .filter(|tour| !tour.steps.is_empty() && tour.version >= VERSION)
            .unwrap_or_default()
    }

    pub fn save(&self, path: &std::path::Path) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        if let Ok(bytes) = serde_json::to_vec_pretty(self) {
            let temporary = path.with_extension("json.tmp");
            if std::fs::write(&temporary, bytes).is_ok() {
                std::fs::rename(&temporary, path).ok();
            }
        }
    }
}

pub fn file() -> std::path::PathBuf {
    crate::paths::AppDirs::discover()
        .index_dir()
        .join("tour.json")
}
