//! Where Spotifast keeps its files.
//!
//! Configuration, durable non-secret state, and disposable caches live in the
//! platform's conventional directories. Spotify grants use the platform store;
//! the token paths below are retained only for migration and sign-out cleanup.

use std::path::PathBuf;

use directories::ProjectDirs;

#[derive(Clone, Debug)]
pub struct AppDirs {
    pub config: PathBuf,
    pub state: PathBuf,
    pub cache: PathBuf,
}

impl AppDirs {
    pub fn discover() -> Self {
        Self::for_name("spotifast")
    }

    /// Where an earlier build kept its files, in the user's profile folders.
    /// Read only to bring them next to the program once.
    pub(crate) fn old_profile() -> Self {
        Self::profile_dirs("spotifast")
    }

    pub(crate) fn legacy() -> Self {
        Self::for_name("fastpotify")
    }

    /// An updater trial launch must leave the old profile available to rollback.
    pub fn for_launch(update_trial: bool) -> Self {
        Self::select_profile(Self::discover(), Self::legacy(), update_trial)
    }

    fn select_profile(current: Self, legacy: Self, update_trial: bool) -> Self {
        if update_trial
            && !current.config.exists()
            && !current.state.exists()
            && (legacy.config.exists() || legacy.state.exists())
        {
            legacy
        } else {
            current
        }
    }

    pub(crate) fn is_legacy_profile(&self) -> bool {
        self.state == Self::legacy().state
    }

    pub fn window_profile(&self) -> &'static str {
        if self.is_legacy_profile() {
            "fastpotify"
        } else {
            "spotifast"
        }
    }

    /// Every file the program keeps goes in one folder beside the program
    /// (`index\data`), so it can be moved or deleted as one. Only when that
    /// folder cannot be written to (the program is under Program Files) does
    /// it use a folder in the profile named for the program.
    fn for_name(name: &str) -> Self {
        if name == "spotifast"
            && let Some(root) = portable_root()
        {
            return Self {
                config: root.join("config"),
                state: root.join("state"),
                cache: root.join("cache"),
            };
        }
        if name == "spotifast" {
            return Self::profile_dirs("chanceify");
        }
        Self::profile_dirs(name)
    }

    fn profile_dirs(name: &str) -> Self {
        let project = ProjectDirs::from("me", "paolino", name);
        match project {
            Some(project) => Self {
                config: project.config_dir().to_path_buf(),
                state: project
                    .state_dir()
                    .map(|path| path.to_path_buf())
                    .unwrap_or_else(|| project.data_local_dir().to_path_buf()),
                cache: project.cache_dir().to_path_buf(),
            },
            None => {
                let fallback = std::env::current_dir().unwrap_or_default();
                Self {
                    config: fallback.join(format!("{name}-config")),
                    state: fallback.join(format!("{name}-state")),
                    cache: fallback.join(format!("{name}-cache")),
                }
            }
        }
    }

    /// Run only after acquiring the single-instance guard, before loading state.
    pub fn migrate_legacy(&self) -> std::io::Result<()> {
        self.migrate_from(&Self::legacy())?;
        // Files an earlier build left in the user's profile come next to
        // the program. A failure here never stops the program starting.
        let old = Self::old_profile();
        if old.state != self.state
            && let Err(error) = self.migrate_from(&old)
        {
            log::warn!("could not bring the old files along: {error}");
        }
        Ok(())
    }

    pub(crate) fn migrate_from(&self, old: &Self) -> std::io::Result<()> {
        use sha2::{Digest, Sha256};
        use std::io::Write;
        // Credential accounts were scoped to the original state path. Preserve
        // that non-secret identity before moving any directory (on macOS config
        // and state share a directory). Never copy a grant into this file.
        if old.state.is_dir() && !self.state.exists() {
            let profile = old.state.join("credential-profile");
            if !profile.exists() {
                let value = format!(
                    "{:x}",
                    Sha256::digest(old.state.to_string_lossy().as_bytes())
                );
                let temporary = old.state.join("credential-profile.tmp");
                let mut file = std::fs::File::create(&temporary)?;
                file.write_all(value.as_bytes())?;
                file.sync_all()?;
                std::fs::rename(temporary, profile)?;
            }
        }
        for (source, destination) in [
            (&old.config, &self.config),
            (&old.state, &self.state),
            (&old.cache, &self.cache),
        ] {
            migrate_directory(source, destination)?;
        }
        Ok(())
    }

    /// The Chanceify index: one folder that holds everything the listener
    /// builds up (settings files, songs, playlist copies). It sits next to
    /// the program, so the whole thing can be moved or copied as one folder,
    /// and falls back to the profile when that folder cannot be written to
    /// (the program is under Program Files, say).
    pub fn index_dir(&self) -> PathBuf {
        static INDEX: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
        INDEX
            .get_or_init(|| {
                if let Ok(exe) = std::env::current_exe()
                    && let Some(dir) = exe.parent()
                {
                    let index = dir.join("index");
                    let probe = index.join("settings");
                    if std::fs::create_dir_all(&probe).is_ok() {
                        return index;
                    }
                }
                self.state.join("index")
            })
            .clone()
    }

    /// Where settings files are saved to and read from.
    pub fn index_settings_dir(&self) -> PathBuf {
        self.index_dir().join("settings")
    }

    pub fn settings_file(&self) -> PathBuf {
        self.config.join("settings.json")
    }

    /// Winamp skins the listener has added, as `.wsz` files or folders.
    pub fn skins_dir(&self) -> PathBuf {
        self.config.join("skins")
    }

    /// MilkDrop presets, as `.milk` files, in folders or not, with any
    /// textures they use in a `textures` folder inside.
    pub fn milkdrop_dir(&self) -> PathBuf {
        self.config.join("milkdrop")
    }

    pub fn session_file(&self) -> PathBuf {
        self.state.join("session.json")
    }

    /// What was played here, which Spotify never hears about and so
    /// cannot tell us later. See [`crate::history`].
    pub fn history_file(&self) -> PathBuf {
        self.state.join("history.json")
    }

    /// Every song that counted as played, with the time: the listening
    /// statistics. It sits in the index folder, beside the songs.
    pub fn stats_file(&self) -> PathBuf {
        self.index_dir().join("stats.json")
    }

    pub fn shared_web_token_file(&self) -> PathBuf {
        self.state.join("shared_web_api_token.json")
    }

    pub fn personal_web_token_file(&self) -> PathBuf {
        self.state.join("personal_web_api_token.json")
    }

    pub fn legacy_web_token_file(&self) -> PathBuf {
        self.state.join("web_api_token.json")
    }

    /// The log of the current run, replaced at every start.
    pub fn log_file(&self) -> PathBuf {
        self.state.join("spotifast.log")
    }

    /// Where a panic is recorded before the process dies of it.
    pub fn panic_log(&self) -> PathBuf {
        self.state.join("panic.log")
    }

    pub fn credentials_dir(&self) -> PathBuf {
        self.state.join("credentials")
    }

    /// Optional proxy password, owner-only, never written to settings.json.
    pub fn proxy_secret_file(&self) -> PathBuf {
        self.state.join("proxy_password")
    }

    pub fn volume_dir(&self) -> PathBuf {
        self.state.join("volume")
    }

    pub fn audio_cache_dir(&self) -> PathBuf {
        self.cache.join("audio")
    }

    pub fn art_cache_dir(&self) -> PathBuf {
        self.cache.join("art")
    }

    pub fn lyrics_cache_dir(&self) -> PathBuf {
        self.cache.join("lyrics")
    }

    pub fn playlist_cache_dir(&self) -> PathBuf {
        self.cache.join("playlists")
    }

    pub fn account_playlist_cache_dir(&self, account_id: &str) -> PathBuf {
        self.playlist_cache_dir().join(account_id)
    }

    pub fn liked_songs_cache_file(&self, account_id: &str) -> PathBuf {
        // Hex encoding also keeps unusual account IDs within the cache root.
        let account: String = account_id
            .bytes()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        self.cache
            .join("liked-songs")
            .join(format!("{account}.json"))
    }

    /// Which of this account's songs are in its library, kept between runs.
    ///
    /// A like state almost never changes on its own, and asking Spotify
    /// again for every row of every list on every launch is what filled the
    /// log with `/me/library/contains` until the shared app's quota ran
    /// out. The file is only a floor: an unlike here is re-asked later, and
    /// a like made in this session is written back over it.
    pub fn library_membership_file(&self, account_id: &str) -> PathBuf {
        let account: String = account_id
            .bytes()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        self.cache.join("library").join(format!("{account}.json"))
    }

    pub fn ensure(&self) -> std::io::Result<()> {
        for dir in [&self.config, &self.state, &self.cache] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}

/// The folder beside the program that holds everything it keeps, if it can
/// be written to.
fn portable_root() -> Option<PathBuf> {
    static ROOT: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    ROOT.get_or_init(|| {
        let exe = std::env::current_exe().ok()?;
        let root = exe.parent()?.join("index").join("data");
        std::fs::create_dir_all(&root).ok()?;
        let probe = root.join(".write-test");
        std::fs::write(&probe, b"x").ok()?;
        let _ = std::fs::remove_file(&probe);
        Some(root)
    })
    .clone()
}

/// Copies a folder and everything in it, for a move from one drive to another.
fn copy_tree(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// A same-filesystem rename preserves contents and permissions. An existing
/// destination, including a dangling symlink, always wins and is never merged.
pub fn migrate_directory(
    source: &std::path::Path,
    destination: &std::path::Path,
) -> std::io::Result<()> {
    match std::fs::symlink_metadata(destination) {
        Ok(_) => return Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        Err(error) => return Err(error),
    }
    match std::fs::metadata(source) {
        Ok(metadata) if metadata.is_dir() => (),
        Ok(_) => return Err(std::io::Error::other("The old profile is not a directory")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    }
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if std::fs::rename(source, destination).is_err() {
        // Another drive: copy, then remove the old folder.
        if let Err(error) = copy_tree(source, destination) {
            let _ = std::fs::remove_dir_all(destination);
            return Err(error);
        }
        let _ = std::fs::remove_dir_all(source);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_preserves_files_and_credential_identity_and_is_repeatable() {
        use sha2::{Digest, Sha256};
        for shared in [false, true] {
            let root =
                std::env::temp_dir().join(format!("spotifast-migration-{}", rand::random::<u64>()));
            let dirs = |name: &str| AppDirs {
                config: root
                    .join(name)
                    .join(if shared { "state" } else { "config" }),
                state: root.join(name).join("state"),
                cache: root.join(name).join("cache"),
            };
            let old = dirs("old");
            let new = dirs("new");
            old.ensure().unwrap();
            std::fs::write(old.settings_file(), b"preferences").unwrap();
            std::fs::write(old.history_file(), b"history").unwrap();
            std::fs::write(old.cache.join("artwork"), b"cover").unwrap();
            assert_eq!(
                AppDirs::select_profile(new.clone(), old.clone(), true).state,
                old.state
            );
            assert_eq!(
                AppDirs::select_profile(new.clone(), old.clone(), false).state,
                new.state
            );
            new.migrate_from(&old).unwrap();
            new.migrate_from(&old).unwrap();
            assert_eq!(
                AppDirs::select_profile(new.clone(), old.clone(), true).state,
                new.state
            );
            assert_eq!(std::fs::read(new.settings_file()).unwrap(), b"preferences");
            assert_eq!(std::fs::read(new.history_file()).unwrap(), b"history");
            assert_eq!(std::fs::read(new.cache.join("artwork")).unwrap(), b"cover");
            assert_eq!(
                std::fs::read_to_string(new.state.join("credential-profile")).unwrap(),
                format!(
                    "{:x}",
                    Sha256::digest(old.state.to_string_lossy().as_bytes())
                )
            );
            // A separately created old profile must not overwrite the new one.
            old.ensure().unwrap();
            std::fs::write(old.settings_file(), b"other preferences").unwrap();
            new.migrate_from(&old).unwrap();
            assert_eq!(std::fs::read(new.settings_file()).unwrap(), b"preferences");
            assert_eq!(
                std::fs::read(old.settings_file()).unwrap(),
                b"other preferences"
            );
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}
