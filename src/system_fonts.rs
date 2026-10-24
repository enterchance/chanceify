//! Fonts installed on this computer, for the visualizer title and the app's
//! own text. The list is read once, on a thread of its own, from the
//! system's font folders; until it is ready only the built-in face is offered.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, PoisonError, RwLock};

/// One face that can be picked: its family name and its file (none for the
/// built-in Inter, which is always first).
#[derive(Clone, Debug)]
pub struct Font {
    pub name: String,
    pub path: Option<PathBuf>,
}

/// The name of the built-in face.
pub const DEFAULT_NAME: &str = "Inter (default)";

static LIST: RwLock<Option<Arc<Vec<Font>>>> = RwLock::new(None);
static STARTED: AtomicBool = AtomicBool::new(false);

/// Every face found so far, the built-in one first. Starts the scan the
/// first time it is asked for.
pub fn list() -> Arc<Vec<Font>> {
    if !STARTED.swap(true, Ordering::Relaxed) {
        let _ = std::thread::Builder::new()
            .name("fonts".into())
            .spawn(|| {
                let found = scan();
                *LIST.write().unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(found));
            });
    }
    LIST.read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
        .unwrap_or_else(|| Arc::new(vec![builtin()]))
}

fn builtin() -> Font {
    Font {
        name: DEFAULT_NAME.to_string(),
        path: None,
    }
}

/// Where a face of this name sits in the list (0, the built-in one, when it
/// is not installed or the list is still being read).
pub fn index_of(name: &str) -> usize {
    if name.is_empty() {
        return 0;
    }
    list().iter().position(|font| font.name == name).unwrap_or(0)
}

/// The file of face `index`, read, when it has one.
pub fn bytes(index: usize) -> Option<Vec<u8>> {
    let fonts = list();
    let path = fonts.get(index)?.path.clone()?;
    std::fs::read(path).ok()
}

/// The folders fonts are installed in on this system.
fn font_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if cfg!(windows) {
        let windows = std::env::var_os("WINDIR").map_or_else(|| PathBuf::from("C:\\Windows"), PathBuf::from);
        dirs.push(windows.join("Fonts"));
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(PathBuf::from(local).join("Microsoft").join("Windows").join("Fonts"));
        }
    } else if cfg!(target_os = "macos") {
        dirs.push(PathBuf::from("/System/Library/Fonts"));
        dirs.push(PathBuf::from("/Library/Fonts"));
        if let Some(home) = std::env::var_os("HOME") {
            dirs.push(PathBuf::from(home).join("Library/Fonts"));
        }
    } else {
        dirs.push(PathBuf::from("/usr/share/fonts"));
        dirs.push(PathBuf::from("/usr/local/share/fonts"));
        if let Some(home) = std::env::var_os("HOME") {
            dirs.push(PathBuf::from(&home).join(".local/share/fonts"));
            dirs.push(PathBuf::from(home).join(".fonts"));
        }
    }
    dirs
}

fn files_in(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if depth < 4 {
                files_in(&path, depth + 1, out);
            }
        } else if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("ttf") || ext.eq_ignore_ascii_case("otf"))
        {
            out.push(path);
        }
    }
}

/// A name the face gives itself, preferring English.
fn face_name(face: &ttf_parser::Face<'_>, ids: &[u16]) -> Option<String> {
    for id in ids {
        let mut fallback = None;
        for name in face.names() {
            if name.name_id != *id {
                continue;
            }
            let Some(text) = name.to_string() else {
                continue;
            };
            if text.trim().is_empty() {
                continue;
            }
            // 0x0409 is English (United States) on Windows.
            if name.language_id == 0x0409 {
                return Some(text.trim().to_string());
            }
            fallback.get_or_insert(text.trim().to_string());
        }
        if fallback.is_some() {
            return fallback;
        }
    }
    None
}

/// One regular face per family, for every font that can draw letters.
fn scan() -> Vec<Font> {
    let mut files = Vec::new();
    for dir in font_dirs() {
        files_in(&dir, 0, &mut files);
    }
    let mut found: Vec<Font> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for path in files {
        // Very large files are the CJK collections: slow to read, and the
        // app already draws those scripts.
        if std::fs::metadata(&path).map_or(true, |meta| meta.len() > 25_000_000) {
            continue;
        }
        let Ok(data) = std::fs::read(&path) else {
            continue;
        };
        let Ok(face) = ttf_parser::Face::parse(&data, 0) else {
            continue;
        };
        // Symbol faces (Wingdings and the like) have no plain letters.
        if face.glyph_index('A').is_none() || face.glyph_index('a').is_none() {
            continue;
        }
        let Some(family) = face_name(&face, &[ttf_parser::name_id::TYPOGRAPHIC_FAMILY, ttf_parser::name_id::FAMILY]) else {
            continue;
        };
        let style = face_name(&face, &[ttf_parser::name_id::TYPOGRAPHIC_SUBFAMILY, ttf_parser::name_id::SUBFAMILY])
            .unwrap_or_default()
            .to_lowercase();
        let regular = style.is_empty() || matches!(style.as_str(), "regular" | "normal" | "book" | "roman" | "medium");
        if !regular {
            continue;
        }
        if family.eq_ignore_ascii_case("inter") || !seen.insert(family.to_lowercase()) {
            continue;
        }
        found.push(Font {
            name: family,
            path: Some(path),
        });
    }
    found.sort_by_key(|font| font.name.to_lowercase());
    found.insert(0, builtin());
    found
}
