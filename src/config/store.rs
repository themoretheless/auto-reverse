use std::env;
use std::ffi::OsString;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::{AppError, AppResult};

use super::schema::AppConfig;

#[cfg(target_os = "macos")]
const APP_DIR_NAME: &str = "Auto Reverse";
const CONFIG_FILE_NAME: &str = "config.toml";

/// Exact serialized contents observed when a config snapshot was loaded.
/// Keeping the full small TOML document instead of a probabilistic hash makes
/// stale-write detection collision-free.
#[derive(Clone, PartialEq, Eq)]
pub struct ConfigRevision(Arc<str>);

impl fmt::Debug for ConfigRevision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConfigRevision")
            .field("bytes", &self.0.len())
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigSnapshot {
    pub config: AppConfig,
    pub revision: ConfigRevision,
}

struct ConfigFileLock {
    _file: File,
}

#[derive(Debug, Clone)]
pub struct ConfigStore {
    path: PathBuf,
}

impl Default for ConfigStore {
    fn default() -> Self {
        Self::new(Self::default_path())
    }
}

impl ConfigStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn default_path() -> PathBuf {
        if let Some(path) = env::var_os("AUTO_REVERSE_CONFIG") {
            return PathBuf::from(path);
        }

        #[cfg(target_os = "macos")]
        {
            if let Some(home) = env::var_os("HOME") {
                return PathBuf::from(home)
                    .join("Library")
                    .join("Application Support")
                    .join(APP_DIR_NAME)
                    .join(CONFIG_FILE_NAME);
            }
        }

        if let Some(config_home) = env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(config_home)
                .join("auto-reverse")
                .join(CONFIG_FILE_NAME);
        }

        if let Some(home) = env::var_os("HOME") {
            return PathBuf::from(home)
                .join(".config")
                .join("auto-reverse")
                .join(CONFIG_FILE_NAME);
        }

        PathBuf::from(CONFIG_FILE_NAME)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn exists(&self) -> bool {
        self.path.exists()
    }

    pub fn load(&self) -> AppResult<AppConfig> {
        Ok(self.load_snapshot()?.config)
    }

    pub fn load_or_create(&self) -> AppResult<AppConfig> {
        Ok(self.load_or_create_snapshot()?.config)
    }

    pub fn load_snapshot(&self) -> AppResult<ConfigSnapshot> {
        self.ensure_parent_directory()?;
        let _lock = self.acquire_lock()?;
        self.read_snapshot_unlocked()
    }

    pub fn load_or_create_snapshot(&self) -> AppResult<ConfigSnapshot> {
        self.ensure_parent_directory()?;
        let _lock = self.acquire_lock()?;

        match self.read_snapshot_unlocked() {
            Ok(snapshot) => Ok(snapshot),
            Err(AppError::Io { source, .. }) if source.kind() == ErrorKind::NotFound => {
                let config = AppConfig::default();
                let revision = self.write_unlocked(&config)?;
                Ok(ConfigSnapshot { config, revision })
            }
            Err(error) => Err(error),
        }
    }

    /// Saves only if the file is byte-for-byte identical to the snapshot the
    /// caller edited. The check and atomic rename happen under one
    /// cross-process lock, so cooperating GUI/CLI writers cannot race between
    /// compare and replace.
    pub fn save_if_unchanged(
        &self,
        config: &AppConfig,
        expected: &ConfigRevision,
    ) -> AppResult<ConfigRevision> {
        self.ensure_parent_directory()?;
        let _lock = self.acquire_lock()?;
        let current = match fs::read_to_string(&self.path) {
            Ok(contents) => Some(ConfigRevision(Arc::from(contents))),
            Err(source) if source.kind() == ErrorKind::NotFound => None,
            Err(source) => return Err(AppError::io("read config", &self.path, source)),
        };

        if current.as_ref() != Some(expected) {
            return Err(AppError::ConfigChanged {
                path: self.path.clone(),
            });
        }

        self.write_unlocked(config)
    }

    /// Runs one complete read-modify-write transaction under the config lock.
    /// This is the CLI mutation primitive: two concurrent commands each see
    /// the prior command's committed fields instead of both saving stale
    /// snapshots with last-writer-wins data loss.
    pub fn update(&self, mutate: impl FnOnce(&mut AppConfig)) -> AppResult<ConfigSnapshot> {
        self.ensure_parent_directory()?;
        let _lock = self.acquire_lock()?;
        let mut config = match self.read_snapshot_unlocked() {
            Ok(snapshot) => snapshot.config,
            Err(AppError::Io { source, .. }) if source.kind() == ErrorKind::NotFound => {
                AppConfig::default()
            }
            Err(error) => return Err(error),
        };
        mutate(&mut config);
        let revision = self.write_unlocked(&config)?;
        Ok(ConfigSnapshot { config, revision })
    }

    fn read_snapshot_unlocked(&self) -> AppResult<ConfigSnapshot> {
        let contents = fs::read_to_string(&self.path)
            .map_err(|source| AppError::io("read config", &self.path, source))?;
        let config: AppConfig =
            toml::from_str(&contents).map_err(|source| AppError::ConfigParse {
                path: self.path.clone(),
                source,
            })?;
        config.validate()?;
        Ok(ConfigSnapshot {
            config,
            revision: ConfigRevision(Arc::from(contents)),
        })
    }

    fn write_unlocked(&self, config: &AppConfig) -> AppResult<ConfigRevision> {
        config.validate()?;
        let contents = toml::to_string_pretty(config).map_err(AppError::ConfigSerialize)?;
        if let Some(parent) = self.path.parent() {
            debug_assert!(parent.as_os_str().is_empty() || parent.exists());
        }
        // Unique per call (process id + a call counter), not a fixed name -
        // a fixed "config.toml.tmp" lets two concurrent saves (e.g. two CLI
        // invocations racing) clobber each other's temp file, silently
        // discarding whichever one loses the race.
        let tmp_path =
            self.path
                .with_extension(format!("toml.{}.{}.tmp", process::id(), next_save_id()));
        fs::write(&tmp_path, contents.as_bytes()).map_err(|source| {
            let _ = fs::remove_file(&tmp_path);
            AppError::io("write temporary config", &tmp_path, source)
        })?;
        fs::rename(&tmp_path, &self.path).map_err(|source| {
            let _ = fs::remove_file(&tmp_path);
            AppError::io("replace config", &self.path, source)
        })?;
        Ok(ConfigRevision(Arc::from(contents)))
    }

    fn ensure_parent_directory(&self) -> AppResult<()> {
        if let Some(parent) = self.path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)
                .map_err(|source| AppError::io("create config directory", parent, source))?;
        }
        Ok(())
    }

    fn acquire_lock(&self) -> AppResult<ConfigFileLock> {
        let lock_path = self.lock_path();
        // This file is intentionally persistent. Removing it after unlock can
        // let one process keep the old locked inode while another creates and
        // locks a new inode at the same path, defeating serialization.
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(|source| AppError::io("open config lock", &lock_path, source))?;
        file.lock()
            .map_err(|source| AppError::io("lock config", &lock_path, source))?;
        Ok(ConfigFileLock { _file: file })
    }

    fn lock_path(&self) -> PathBuf {
        let mut file_name = self
            .path
            .file_name()
            .map(OsString::from)
            .unwrap_or_else(|| OsString::from(CONFIG_FILE_NAME));
        file_name.push(".lock");
        self.path.with_file_name(file_name)
    }
}

fn next_save_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, mpsc};
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use super::*;

    fn test_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        env::temp_dir().join(format!("auto-reverse-{name}-{nanos}.toml"))
    }

    fn cleanup(store: &ConfigStore) {
        let _ = fs::remove_file(store.path());
        let _ = fs::remove_file(store.lock_path());
    }

    #[test]
    fn config_round_trips_through_toml() {
        let path = test_path("roundtrip");
        let store = ConfigStore::new(&path);
        let config = AppConfig {
            reverse_horizontal: true,
            reverse_trackpad: true,
            ..AppConfig::default()
        };

        let initial = store.load_or_create_snapshot().unwrap();
        store.save_if_unchanged(&config, &initial.revision).unwrap();

        assert_eq!(store.load().unwrap(), config);
        cleanup(&store);
    }

    #[test]
    fn save_can_be_called_twice_without_leaking_temp_files() {
        let path = test_path("no-leftover-tmp");
        let store = ConfigStore::new(&path);

        let initial = store.load_or_create_snapshot().unwrap();
        let next = store
            .save_if_unchanged(&AppConfig::default(), &initial.revision)
            .unwrap();
        store
            .save_if_unchanged(&AppConfig::default(), &next)
            .unwrap();

        let leftover_tmp_files: Vec<_> = fs::read_dir(env::temp_dir())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(path.file_stem().unwrap().to_string_lossy().as_ref())
            })
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "tmp"))
            .collect();

        assert!(
            leftover_tmp_files.is_empty(),
            "found leftover temp files: {leftover_tmp_files:?}"
        );
        cleanup(&store);
    }

    #[test]
    fn stale_snapshot_is_rejected_without_overwriting_external_changes() {
        let path = test_path("stale-revision");
        let store = ConfigStore::new(&path);
        let initial = store.load_or_create_snapshot().unwrap();

        let external = store
            .update(|config| config.reverse_horizontal = true)
            .unwrap();
        let mut stale = initial.config;
        stale.reverse_trackpad = true;

        let error = store
            .save_if_unchanged(&stale, &initial.revision)
            .unwrap_err();

        assert!(error.is_config_changed());
        let disk = store.load().unwrap();
        assert_eq!(disk, external.config);
        assert!(disk.reverse_horizontal);
        assert!(!disk.reverse_trackpad);
        cleanup(&store);
    }

    #[test]
    fn successful_compare_and_swap_returns_the_next_usable_revision() {
        let path = test_path("cas-chain");
        let store = ConfigStore::new(&path);
        let initial = store.load_or_create_snapshot().unwrap();
        let mut first = initial.config;
        first.reverse_horizontal = true;

        let first_revision = store.save_if_unchanged(&first, &initial.revision).unwrap();
        let mut second = first;
        second.reverse_trackpad = true;
        store.save_if_unchanged(&second, &first_revision).unwrap();

        assert_eq!(store.load().unwrap(), second);
        cleanup(&store);
    }

    #[test]
    fn concurrent_updates_are_serialized_and_preserve_both_mutations() {
        let path = test_path("serialized-updates");
        let store = Arc::new(ConfigStore::new(&path));
        store.load_or_create().unwrap();

        let (first_entered_tx, first_entered_rx) = mpsc::channel();
        let (release_first_tx, release_first_rx) = mpsc::channel();
        let first_store = Arc::clone(&store);
        let first = thread::spawn(move || {
            first_store
                .update(|config| {
                    config.reverse_horizontal = true;
                    first_entered_tx.send(()).unwrap();
                    release_first_rx.recv().unwrap();
                })
                .unwrap();
        });
        first_entered_rx.recv().unwrap();

        let (second_done_tx, second_done_rx) = mpsc::channel();
        let second_store = Arc::clone(&store);
        let second = thread::spawn(move || {
            second_store
                .update(|config| config.reverse_trackpad = true)
                .unwrap();
            second_done_tx.send(()).unwrap();
        });

        assert!(matches!(
            second_done_rx.recv_timeout(Duration::from_millis(100)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        release_first_tx.send(()).unwrap();
        first.join().unwrap();
        second.join().unwrap();

        let config = store.load().unwrap();
        assert!(config.reverse_horizontal);
        assert!(config.reverse_trackpad);
        cleanup(&store);
    }
}
