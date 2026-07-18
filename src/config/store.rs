use std::env;
use std::ffi::OsString;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigRepairOutcome {
    Unchanged {
        config: AppConfig,
    },
    Created {
        config: AppConfig,
    },
    Repaired {
        config: AppConfig,
        backup_path: PathBuf,
    },
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

    /// Reads and validates an existing config without creating its parent,
    /// config file, or lock file. Atomic writers make either the old or new
    /// complete revision visible to this read-only diagnostic path.
    pub fn inspect_existing(&self) -> AppResult<Option<AppConfig>> {
        let contents = match self.read_utf8("read config") {
            Ok(contents) => contents,
            Err(AppError::Io { source, .. }) if source.kind() == ErrorKind::NotFound => {
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        self.parse_contents(&contents).map(Some)
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

    /// Explicitly repairs a missing or invalid regular config. Invalid bytes
    /// are moved intact to a unique sibling before defaults are written; if
    /// replacement fails, the original path is restored before returning.
    pub fn repair_with_defaults(&self) -> AppResult<ConfigRepairOutcome> {
        self.repair_with_writer(|store, config| store.write_unlocked(config))
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
        let current = match self.read_utf8("read config") {
            Ok(contents) => Some(ConfigRevision(Arc::from(contents))),
            Err(AppError::Io { source, .. }) if source.kind() == ErrorKind::NotFound => None,
            Err(error) => return Err(error),
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
        let contents = self.read_utf8("read config")?;
        let config = self.parse_contents(&contents)?;
        Ok(ConfigSnapshot {
            config,
            revision: ConfigRevision(Arc::from(contents)),
        })
    }

    fn parse_contents(&self, contents: &str) -> AppResult<AppConfig> {
        let config: AppConfig =
            toml::from_str(contents).map_err(|source| AppError::ConfigParse {
                path: self.path.clone(),
                source,
            })?;
        config.validate()?;
        Ok(config)
    }

    fn read_utf8(&self, action: &'static str) -> AppResult<String> {
        match fs::read_to_string(&self.path) {
            Ok(contents) => Ok(contents),
            Err(source) if source.kind() == ErrorKind::InvalidData => Err(AppError::InvalidConfig(
                format!("`{}` must be valid UTF-8", self.path.display()),
            )),
            Err(source) => Err(AppError::io(action, &self.path, source)),
        }
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
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut temporary = options
            .open(&tmp_path)
            .map_err(|source| AppError::io("create temporary config", &tmp_path, source))?;
        temporary.write_all(contents.as_bytes()).map_err(|source| {
            let _ = fs::remove_file(&tmp_path);
            AppError::io("write temporary config", &tmp_path, source)
        })?;
        temporary.sync_all().map_err(|source| {
            let _ = fs::remove_file(&tmp_path);
            AppError::io("sync temporary config", &tmp_path, source)
        })?;
        drop(temporary);
        fs::rename(&tmp_path, &self.path).map_err(|source| {
            let _ = fs::remove_file(&tmp_path);
            AppError::io("replace config", &self.path, source)
        })?;
        self.sync_parent_directory()?;
        Ok(ConfigRevision(Arc::from(contents)))
    }

    fn repair_with_writer(
        &self,
        write_replacement: impl FnOnce(&Self, &AppConfig) -> AppResult<ConfigRevision>,
    ) -> AppResult<ConfigRepairOutcome> {
        self.ensure_parent_directory()?;
        let _lock = self.acquire_lock()?;
        let defaults = AppConfig::default();
        let metadata = match fs::symlink_metadata(&self.path) {
            Ok(metadata) => metadata,
            Err(source) if source.kind() == ErrorKind::NotFound => {
                write_replacement(self, &defaults)?;
                return Ok(ConfigRepairOutcome::Created { config: defaults });
            }
            Err(source) => return Err(AppError::io("inspect config", &self.path, source)),
        };
        if !metadata.file_type().is_file() {
            return Err(AppError::InvalidConfig(format!(
                "refusing to repair non-regular config `{}`",
                self.path.display()
            )));
        }

        let original = fs::read(&self.path)
            .map_err(|source| AppError::io("read config for repair", &self.path, source))?;
        if let Ok(contents) = std::str::from_utf8(&original)
            && let Ok(config) = self.parse_contents(contents)
        {
            return Ok(ConfigRepairOutcome::Unchanged { config });
        }

        let backup_path = self.quarantine_invalid_config()?;
        if let Err(error) = self.sync_parent_directory() {
            return self.restore_after_failed_repair(&backup_path, error);
        }

        match write_replacement(self, &defaults) {
            Ok(_) => Ok(ConfigRepairOutcome::Repaired {
                config: defaults,
                backup_path,
            }),
            Err(error) => self.restore_after_failed_repair(&backup_path, error),
        }
    }

    fn restore_after_failed_repair<T>(
        &self,
        backup_path: &Path,
        original_error: AppError,
    ) -> AppResult<T> {
        match fs::remove_file(&self.path) {
            Ok(()) => {}
            Err(source) if source.kind() == ErrorKind::NotFound => {}
            Err(source) => {
                return Err(AppError::Platform(format!(
                    "config repair failed ({original_error}); removing the incomplete replacement also failed: {source}"
                )));
            }
        }
        if let Err(source) = fs::rename(backup_path, &self.path) {
            return Err(AppError::Platform(format!(
                "config repair failed ({original_error}); restoring `{}` also failed: {source}",
                self.path.display()
            )));
        }
        if let Err(rollback_error) = self.sync_parent_directory() {
            return Err(AppError::Platform(format!(
                "config repair failed ({original_error}); original bytes were restored but directory sync failed ({rollback_error})"
            )));
        }
        Err(original_error)
    }

    fn quarantine_invalid_config(&self) -> AppResult<PathBuf> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let stem = self
            .path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("config");
        let extension = self
            .path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("toml");
        for _ in 0..1_024 {
            let candidate = self.path.with_file_name(format!(
                "{stem}.broken.{timestamp}.{}.{}.{extension}",
                process::id(),
                next_save_id()
            ));
            match fs::hard_link(&self.path, &candidate) {
                Ok(()) => {
                    if let Err(source) = File::open(&candidate).and_then(|backup| backup.sync_all())
                    {
                        let _ = fs::remove_file(&candidate);
                        return Err(AppError::io(
                            "sync invalid config backup",
                            &candidate,
                            source,
                        ));
                    }
                    if let Err(source) = fs::remove_file(&self.path) {
                        if let Err(cleanup_error) = fs::remove_file(&candidate) {
                            return Err(AppError::Platform(format!(
                                "could not quarantine invalid config ({source}); removing the duplicate backup `{}` also failed: {cleanup_error}",
                                candidate.display()
                            )));
                        }
                        return Err(AppError::io(
                            "remove invalid config after backup",
                            &self.path,
                            source,
                        ));
                    }
                    return Ok(candidate);
                }
                Err(source) if source.kind() == ErrorKind::AlreadyExists => continue,
                Err(source) => {
                    return Err(AppError::io(
                        "create invalid config backup",
                        &candidate,
                        source,
                    ));
                }
            }
        }
        Err(AppError::Platform(
            "could not allocate a unique broken-config backup path".to_string(),
        ))
    }

    fn sync_parent_directory(&self) -> AppResult<()> {
        let Some(parent) = self.path.parent() else {
            return Ok(());
        };
        if parent.as_os_str().is_empty() {
            return Ok(());
        }
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|source| AppError::io("sync config directory", parent, source))
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
    #[cfg(unix)]
    use std::os::unix::fs::{PermissionsExt, symlink};
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

    fn broken_siblings(store: &ConfigStore) -> Vec<PathBuf> {
        let parent = store.path().parent().unwrap_or_else(|| Path::new("."));
        let prefix = format!(
            "{}.broken.",
            store
                .path()
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("config")
        );
        fs::read_dir(parent)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|value| value.to_str())
                    .is_some_and(|name| name.starts_with(&prefix))
            })
            .collect()
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
    fn read_only_inspection_does_not_create_parent_config_or_lock() {
        let root = env::temp_dir().join(format!(
            "auto-reverse-inspect-{}-{}",
            process::id(),
            next_save_id()
        ));
        let store = ConfigStore::new(root.join("nested/config.toml"));

        assert_eq!(store.inspect_existing().unwrap(), None);
        assert!(!root.exists());
        assert!(!store.path().exists());
        assert!(!store.lock_path().exists());
    }

    #[cfg(unix)]
    #[test]
    fn committed_config_is_private_to_the_user() {
        let path = test_path("private-mode");
        let store = ConfigStore::new(&path);

        store.load_or_create().unwrap();

        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        cleanup(&store);
    }

    #[test]
    fn explicit_repair_preserves_invalid_bytes_and_writes_defaults() {
        let path = test_path("repair-invalid");
        let store = ConfigStore::new(&path);
        let invalid = b"enabled = maybe\n\xff";
        fs::write(&path, invalid).unwrap();

        let outcome = store.repair_with_defaults().unwrap();
        let ConfigRepairOutcome::Repaired {
            config,
            backup_path,
        } = outcome
        else {
            panic!("invalid config should be repaired");
        };

        assert_eq!(config, AppConfig::default());
        assert_eq!(fs::read(&backup_path).unwrap(), invalid);
        assert_eq!(store.load().unwrap(), AppConfig::default());
        fs::remove_file(backup_path).unwrap();
        cleanup(&store);
    }

    #[test]
    fn repair_leaves_a_valid_config_byte_for_byte_unchanged() {
        let path = test_path("repair-valid");
        let store = ConfigStore::new(&path);
        let original = "# keep this comment\nenabled = false\n";
        fs::write(&path, original).unwrap();

        let outcome = store.repair_with_defaults().unwrap();

        assert!(matches!(outcome, ConfigRepairOutcome::Unchanged { .. }));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        assert!(broken_siblings(&store).is_empty());
        cleanup(&store);
    }

    #[test]
    fn failed_repair_restores_the_original_path_and_bytes() {
        let path = test_path("repair-rollback");
        let store = ConfigStore::new(&path);
        let invalid = b"not valid = [";
        fs::write(&path, invalid).unwrap();

        let error = store
            .repair_with_writer(|_, _| {
                Err(AppError::Platform(
                    "injected replacement failure".to_string(),
                ))
            })
            .unwrap_err();

        assert_eq!(error.code(), "E_PLATFORM");
        assert_eq!(fs::read(&path).unwrap(), invalid);
        assert!(broken_siblings(&store).is_empty());
        cleanup(&store);
    }

    #[test]
    fn repair_creates_defaults_when_config_is_missing() {
        let path = test_path("repair-missing");
        let store = ConfigStore::new(&path);

        let outcome = store.repair_with_defaults().unwrap();

        assert!(matches!(outcome, ConfigRepairOutcome::Created { .. }));
        assert_eq!(store.load().unwrap(), AppConfig::default());
        cleanup(&store);
    }

    #[cfg(unix)]
    #[test]
    fn repair_refuses_to_follow_a_config_symlink() {
        let target_path = test_path("repair-symlink-target");
        let link_path = test_path("repair-symlink-link");
        let store = ConfigStore::new(&link_path);
        let invalid = b"not valid = [";
        fs::write(&target_path, invalid).unwrap();
        symlink(&target_path, &link_path).unwrap();

        let error = store.repair_with_defaults().unwrap_err();

        assert_eq!(error.code(), "E_CONFIG_INVALID");
        assert!(
            fs::symlink_metadata(&link_path)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read(&target_path).unwrap(), invalid);
        assert!(broken_siblings(&store).is_empty());
        cleanup(&store);
        fs::remove_file(target_path).unwrap();
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
