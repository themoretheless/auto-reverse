use serde::{Deserialize, Serialize};

use crate::device::DeviceKind;
use crate::error::{AppError, AppResult};

pub const CONFIG_VERSION: u32 = 1;

// deny_unknown_fields is deliberately NOT used here: it would make
// toml::from_str hard-fail with a generic "unknown field" parse error on
// any config written by a newer version that added a field, before
// validate()'s config_version check ever gets a chance to run and produce
// the intended, actionable "unsupported config_version" message instead.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn invalid_step_size_is_rejected() {
        let config = AppConfig {
            discrete_scroll_step_size: 99,
            ..AppConfig::default()
        };

        assert!(config.validate().is_err());
    }

    #[test]
    fn unknown_fields_are_ignored_instead_of_hard_failing() {
        // Regression test: a config written by a newer version with an
        // extra field must still load, so config_version can drive
        // compatibility decisions instead of a generic parse error.
        let config: AppConfig =
            toml::from_str("config_version = 1\nenabled = true\na_field_from_the_future = true\n")
                .unwrap();

        assert!(config.enabled);
    }
}
