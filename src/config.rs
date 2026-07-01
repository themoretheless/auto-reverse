use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::device::DeviceKind;
use crate::error::{AppError, AppResult};

pub const CONFIG_VERSION: u32 = 1;
const APP_DIR_NAME: &str = "Auto Reverse";
const CONFIG_FILE_NAME: &str = "config.toml";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AppConfig {
    pub config_version: u32,
    pub enabled: bool,
    pub reverse_vertical: bool,
    pub reverse_horizontal: bool,
    pub reverse_mouse: bool,
    pub reverse_trackpad: bool,
    pub reverse_magic_mouse: bool,
    pub reverse_unknown: bool,
    pub discrete_scroll_step_size: i64,
    pub show_discrete_scroll_options: bool,
    pub start_at_login: bool,
    pub show_menu_bar_icon: bool,
    pub check_for_updates: bool,
    pub include_beta_updates: bool,
    pub reverse_only_raw_input: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            config_version: CONFIG_VERSION,
            enabled: true,
            reverse_vertical: true,
            reverse_horizontal: false,
            reverse_mouse: true,
            reverse_trackpad: false,
            reverse_magic_mouse: true,
            reverse_unknown: false,
            discrete_scroll_step_size: 3,
            show_discrete_scroll_options: false,
            start_at_login: false,
            show_menu_bar_icon: true,
            check_for_updates: false,
            include_beta_updates: false,
            reverse_only_raw_input: false,
        }
    }
}

impl AppConfig {
    pub fn validate(&self) -> AppResult<()> {
        if self.config_version != CONFIG_VERSION {
            return Err(AppError::InvalidConfig(format!(
                "unsupported config_version {}; expected {CONFIG_VERSION}",
                self.config_version
            )));
        }

        if !(0..=20).contains(&self.discrete_scroll_step_size) {
            return Err(AppError::InvalidConfig(
                "discrete_scroll_step_size must be between 0 and 20".to_string(),
            ));
        }

        Ok(())
    }

    pub fn should_reverse_device(&self, device_kind: DeviceKind) -> bool {
        match device_kind {
            DeviceKind::Mouse => self.reverse_mouse,
            DeviceKind::Trackpad => self.reverse_trackpad,
            DeviceKind::MagicMouse => self.reverse_magic_mouse,
            DeviceKind::Unknown => self.reverse_unknown,
        }
    }
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
        fs::write(&tmp_path, contents)
            .map_err(|source| AppError::io("write temporary config", &tmp_path, source))?;
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
    fn default_config_matches_mouse_reversal_mvp() {
        let config = AppConfig::default();

        assert!(config.enabled);
        assert!(config.reverse_vertical);
        assert!(!config.reverse_horizontal);
        assert!(config.reverse_mouse);
        assert!(!config.reverse_trackpad);
        assert_eq!(config.discrete_scroll_step_size, 3);
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
    fn invalid_step_size_is_rejected() {
        let config = AppConfig {
            discrete_scroll_step_size: 99,
            ..AppConfig::default()
        };

        assert!(config.validate().is_err());
    }
}
