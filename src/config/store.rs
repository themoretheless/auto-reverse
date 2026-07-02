use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::{AppError, AppResult};

use super::schema::AppConfig;

#[cfg(target_os = "macos")]
const APP_DIR_NAME: &str = "Auto Reverse";
const CONFIG_FILE_NAME: &str = "config.toml";

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
        let contents = fs::read_to_string(&self.path)
            .map_err(|source| AppError::io("read config", &self.path, source))?;
        let config: AppConfig =
            toml::from_str(&contents).map_err(|source| AppError::ConfigParse {
                path: self.path.clone(),
                source,
            })?;
        config.validate()?;
        Ok(config)
    }

    pub fn load_or_create(&self) -> AppResult<AppConfig> {
        if self.exists() {
            return self.load();
        }

        let config = AppConfig::default();
        self.save(&config)?;
        Ok(config)
    }

    pub fn save(&self, config: &AppConfig) -> AppResult<()> {
        config.validate()?;

        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .map_err(|source| AppError::io("create config directory", parent, source))?;
        }

        let contents = toml::to_string_pretty(config).map_err(AppError::ConfigSerialize)?;
        // Unique per call (process id + a call counter), not a fixed name -
        // a fixed "config.toml.tmp" lets two concurrent saves (e.g. two CLI
        // invocations racing) clobber each other's temp file, silently
        // discarding whichever one loses the race.
        let tmp_path =
            self.path
                .with_extension(format!("toml.{}.{}.tmp", process::id(), next_save_id()));
        fs::write(&tmp_path, contents).map_err(|source| {
            let _ = fs::remove_file(&tmp_path);
            AppError::io("write temporary config", &tmp_path, source)
        })?;
        fs::rename(&tmp_path, &self.path).map_err(|source| {
            let _ = fs::remove_file(&tmp_path);
            AppError::io("replace config", &self.path, source)
        })?;
        Ok(())
    }
}

fn next_save_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn test_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        env::temp_dir().join(format!("auto-reverse-{name}-{nanos}.toml"))
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

        store.save(&config).unwrap();

        assert_eq!(store.load().unwrap(), config);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn save_can_be_called_twice_without_leaking_temp_files() {
        let path = test_path("no-leftover-tmp");
        let store = ConfigStore::new(&path);

        store.save(&AppConfig::default()).unwrap();
        store.save(&AppConfig::default()).unwrap();

        let leftover_tmp_files: Vec<_> = fs::read_dir(env::temp_dir())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(path.file_stem().unwrap().to_string_lossy().as_ref())
            })
            .filter(|entry| entry.path() != path)
            .collect();

        assert!(
            leftover_tmp_files.is_empty(),
            "found leftover temp files: {leftover_tmp_files:?}"
        );
        let _ = fs::remove_file(path);
    }
}
