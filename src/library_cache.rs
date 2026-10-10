//! Which of an account's songs are in its library, remembered between runs.
//!
//! `/me/library/contains` is the request this exists to stop repeating. It
//! was asked for every row of every list the interface drew, on every
//! launch, for an answer that changes only when the reader presses the heart
//! — and this program's shared Spotify app has a quota that every other
//! person running it is spending too. The rate-limit log was full of it.
//!
//! So an answer is written down and read back, and a row that has an answer
//! is never asked about again. What is written is deliberately only a
//! floor: a song recorded as *not* saved is re-asked after a while, because
//! the reader may have liked it on their phone, while a song recorded as
//! saved is trusted until this program itself changes it.

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::paths::AppDirs;

/// How long a "not in the library" answer is believed before it is asked
/// about again. Long enough that a normal session never re-asks, short
/// enough that a like made elsewhere turns up on its own. A day: an hour
/// meant a thousand-song library was asked about again every launch.
pub const UNSAVED_REASK: Duration = Duration::from_secs(24 * 60 * 60);

/// How often a changed file is written, so a scroll through a long list
/// writes once rather than once per row.
const WRITE_AFTER: Duration = Duration::from_secs(5);

#[derive(Serialize, Deserialize, Default)]
struct Stored {
    #[serde(default)]
    version: u32,
    /// Track URI to whether the account's library holds it.
    #[serde(default)]
    saved: HashMap<String, bool>,
    /// When each negative answer was written, so it can be re-asked later.
    #[serde(default)]
    asked_at: HashMap<String, u64>,
}

#[derive(Default)]
pub struct Membership {
    /// The account this file belongs to. A different account starts over
    /// rather than inheriting someone else's library.
    account: String,
    saved: HashMap<String, bool>,
    asked_at: HashMap<String, Instant>,
    dirty: bool,
    last_write: Option<Instant>,
}

impl Membership {
    /// Reads the file for `account`, if there is one.
    pub fn load(dirs: &AppDirs, account: Option<&str>) -> Self {
        let Some(account) = account.filter(|id| !id.is_empty()) else {
            return Self::default();
        };
        let path = dirs.library_membership_file(account);
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self {
                account: account.to_string(),
                ..Self::default()
            };
        };
        let Ok(stored) = serde_json::from_str::<Stored>(&text) else {
            log::debug!("could not read the library cache; asking again");
            return Self {
                account: account.to_string(),
                ..Self::default()
            };
        };
        if stored.version != 1 {
            return Self {
                account: account.to_string(),
                ..Self::default()
            };
        }
        let now = Instant::now();
        Self {
            account: account.to_string(),
            saved: stored.saved,
            asked_at: stored
                .asked_at
                .into_iter()
                // `checked_sub`: an age longer than this computer has been
                // switched on cannot be taken off the clock, and a plain
                // subtraction panics there. Such an answer simply starts its
                // window again.
                .map(|(uri, seconds)| {
                    (uri, now.checked_sub(Duration::from_secs(seconds)).unwrap_or(now))
                })
                .collect(),
            dirty: false,
            last_write: Some(now),
        }
    }

    /// Forgets everything. Used on sign-out and on an account switch.
    pub fn clear(&mut self) {
        self.account.clear();
        self.saved.clear();
        self.asked_at.clear();
        self.dirty = false;
    }

    /// Whether this URI already has an answer worth drawing on.
    ///
    /// A positive answer is trusted indefinitely. A negative one is only
    /// believed until it is [`UNSAVED_REASK`] old, because the reader may
    /// have liked the song somewhere this program cannot see.
    pub fn knows(&self, uri: &str) -> Option<bool> {
        match self.saved.get(uri) {
            Some(true) => Some(true),
            Some(false) => match self.asked_at.get(uri) {
                Some(at) if at.elapsed() < UNSAVED_REASK => Some(false),
                _ => None,
            },
            None => None,
        }
    }

    /// Records what `/me/library/contains` answered.
    pub fn remember<I>(&mut self, answers: I)
    where
        I: IntoIterator<Item = (String, bool)>,
    {
        if self.account.is_empty() {
            return;
        }
        let now = Instant::now();
        for (uri, saved) in answers {
            if uri.is_empty() {
                continue;
            }
            self.saved.insert(uri.clone(), saved);
            self.asked_at.insert(uri, now);
            self.dirty = true;
        }
    }

    /// Records one song's state, whether or not Spotify has been asked.
    ///
    /// This is what a press of the heart goes through, so the next launch
    /// draws the right heart without spending a request to find out.
    pub fn set(&mut self, uri: &str, saved: bool) {
        if uri.is_empty() || self.account.is_empty() {
            return;
        }
        self.saved.insert(uri.to_string(), saved);
        self.asked_at.insert(uri.to_string(), Instant::now());
        self.dirty = true;
    }

    /// Writes the file if anything changed and enough time has passed.
    pub fn checkpoint(&mut self, dirs: &AppDirs) {
        if !self.dirty || self.account.is_empty() {
            return;
        }
        if self.last_write.is_some_and(|at| at.elapsed() < WRITE_AFTER) {
            return;
        }
        self.write(dirs);
    }

    /// Writes the file now, if anything changed.
    pub fn save(&mut self, dirs: &AppDirs) {
        if self.dirty && !self.account.is_empty() {
            self.write(dirs);
        }
    }

    fn write(&mut self, dirs: &AppDirs) {
        let path = dirs.library_membership_file(&self.account);
        let now = Instant::now();
        let stored = Stored {
            version: 1,
            saved: self.saved.clone(),
            asked_at: self
                .asked_at
                .iter()
                .map(|(uri, at)| (uri.clone(), at.elapsed().as_secs()))
                .collect(),
        };
        if write_atomic(&path, &stored).is_ok() {
            self.dirty = false;
            self.last_write = Some(now);
        } else {
            log::debug!("could not write the library cache");
        }
    }

    /// How many answers are held, for the tests and the settings page.
    pub fn len(&self) -> usize {
        self.saved.len()
    }

    pub fn is_empty(&self) -> bool {
        self.saved.is_empty()
    }
}

/// Writes through a temporary file in the same directory, so a reader never
/// sees half a file and a crash mid-write leaves the old one intact.
fn write_atomic(path: &Path, stored: &Stored) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text =
        serde_json::to_vec(stored).map_err(|error| std::io::Error::other(error.to_string()))?;
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, &text)?;
    // Windows will not replace a file that is open elsewhere, and the old
    // name is the one a reader wants, so fall back to writing in place.
    match std::fs::rename(&temporary, path) {
        Ok(()) => Ok(()),
        Err(_) => {
            let _ = std::fs::remove_file(&temporary);
            std::fs::write(path, &text)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs(name: &str) -> (std::path::PathBuf, AppDirs) {
        let root =
            std::env::temp_dir().join(format!("chanceify-library-{name}-{}", std::process::id()));
        let dirs = AppDirs {
            config: root.join("config"),
            state: root.join("state"),
            cache: root.join("cache"),
        };
        (root, dirs)
    }

    #[test]
    fn an_answer_is_never_asked_for_twice() {
        let (root, dirs) = dirs("asked-once");
        let mut membership = Membership::load(&dirs, Some("reader"));
        assert_eq!(membership.knows("spotify:track:1"), None);
        membership.remember([("spotify:track:1".into(), true)]);
        membership.save(&dirs);

        // A fresh load stands in for the next launch.
        let reloaded = Membership::load(&dirs, Some("reader"));
        assert_eq!(reloaded.knows("spotify:track:1"), Some(true));
        assert_eq!(reloaded.knows("spotify:track:2"), None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn another_accounts_library_is_never_inherited() {
        let (root, dirs) = dirs("account");
        let mut membership = Membership::load(&dirs, Some("first"));
        membership.remember([("spotify:track:1".into(), true)]);
        membership.save(&dirs);

        let other = Membership::load(&dirs, Some("second"));
        assert_eq!(
            other.knows("spotify:track:1"),
            None,
            "a different account must start with nothing known"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn an_unlike_is_only_believed_for_an_hour() {
        let (root, dirs) = dirs("unask");
        let mut membership = Membership::load(&dirs, Some("reader"));
        membership.remember([("spotify:track:1".into(), false)]);
        assert_eq!(membership.knows("spotify:track:1"), Some(false));
        // Aged past the window by hand: the reader may have liked it
        // elsewhere, so it has to be asked about again.
        membership.asked_at.insert(
            "spotify:track:1".into(),
            Instant::now()
                .checked_sub(UNSAVED_REASK + Duration::from_secs(1))
                .unwrap_or_else(Instant::now),
        );
        // A computer switched on for less than the window cannot age an
        // answer past it; the check only means something where it can.
        if Instant::now()
            .checked_sub(UNSAVED_REASK + Duration::from_secs(1))
            .is_some()
        {
            assert_eq!(membership.knows("spotify:track:1"), None);
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn signing_out_forgets_everything() {
        let (root, dirs) = dirs("signout");
        let mut membership = Membership::load(&dirs, Some("reader"));
        membership.remember([("spotify:track:1".into(), true)]);
        membership.clear();
        assert!(membership.is_empty());
        // And nothing is written for an account with no id.
        membership.save(&dirs);
        assert!(Membership::load(&dirs, Some("reader")).is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_corrupt_file_is_asked_about_again_rather_than_trusted() {
        let (root, dirs) = dirs("corrupt");
        let path = dirs.library_membership_file("reader");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{not json").unwrap();
        let membership = Membership::load(&dirs, Some("reader"));
        assert!(membership.is_empty());
        let _ = std::fs::remove_dir_all(root);
    }
}
