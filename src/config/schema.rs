use serde::{Deserialize, Serialize};

use crate::device::{DeviceKind, HardwareId};
use crate::error::{AppError, AppResult};

pub const CONFIG_VERSION: u32 = 1;

/// A per-physical-device override: matches one exact vendor/product pair
/// and pins its reversal on or off regardless of the per-kind flags.
/// TOML supports hex literals, so a rule reads naturally:
///
/// ```toml
/// [[device_rules]]
/// vendor_id = 0x046d
/// product_id = 0xc52b
/// name = "Logitech MX Master"  # optional, display only
/// reverse = true
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceRule {
    pub vendor_id: u32,
    pub product_id: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub reverse: bool,
}

impl DeviceRule {
    pub fn matches(&self, hardware: HardwareId) -> bool {
        self.vendor_id == hardware.vendor_id && self.product_id == hardware.product_id
    }
}

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
    pub device_rules: Vec<DeviceRule>,
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
            device_rules: Vec::new(),
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

        for (index, rule) in self.device_rules.iter().enumerate() {
            let duplicate = self.device_rules[..index]
                .iter()
                .any(|earlier| earlier.vendor_id == rule.vendor_id
                    && earlier.product_id == rule.product_id);
            if duplicate {
                return Err(AppError::InvalidConfig(format!(
                    "duplicate device_rules entry for vendor_id=0x{:04x} product_id=0x{:04x}",
                    rule.vendor_id, rule.product_id
                )));
            }
        }

        Ok(())
    }

    /// Full reversal policy: an exact per-device rule wins when the event's
    /// hardware is known; otherwise the per-kind flag decides.
    pub fn should_reverse(&self, device_kind: DeviceKind, hardware: Option<HardwareId>) -> bool {
        if let Some(hardware) = hardware
            && let Some(rule) = self.device_rules.iter().find(|rule| rule.matches(hardware))
        {
            return rule.reverse;
        }
        self.should_reverse_device(device_kind)
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
    fn device_rule_overrides_the_kind_flag_in_both_directions() {
        let logitech = HardwareId {
            vendor_id: 0x046d,
            product_id: 0xc52b,
        };
        let razer = HardwareId {
            vendor_id: 0x1532,
            product_id: 0x0067,
        };
        let config = AppConfig {
            reverse_mouse: true,
            device_rules: vec![DeviceRule {
                vendor_id: 0x046d,
                product_id: 0xc52b,
                name: None,
                reverse: false,
            }],
            ..AppConfig::default()
        };

        // The rule pins this exact device off even though mice reverse.
        assert!(!config.should_reverse(DeviceKind::Mouse, Some(logitech)));
        // A device without a rule falls back to the kind flag.
        assert!(config.should_reverse(DeviceKind::Mouse, Some(razer)));
        // Unknown hardware falls back to the kind flag too.
        assert!(config.should_reverse(DeviceKind::Mouse, None));
    }

    #[test]
    fn duplicate_device_rules_are_rejected() {
        let rule = DeviceRule {
            vendor_id: 1,
            product_id: 2,
            name: None,
            reverse: true,
        };
        let config = AppConfig {
            device_rules: vec![rule.clone(), rule],
            ..AppConfig::default()
        };

        assert!(config.validate().is_err());
    }

    #[test]
    fn device_rules_round_trip_through_toml_with_hex_ids() {
        let config: AppConfig = toml::from_str(
            "config_version = 1\n\
             [[device_rules]]\n\
             vendor_id = 0x046d\n\
             product_id = 0xc52b\n\
             name = \"Logitech MX Master\"\n\
             reverse = true\n",
        )
        .unwrap();

        assert_eq!(config.device_rules.len(), 1);
        assert_eq!(config.device_rules[0].vendor_id, 0x046d);
        assert!(config.device_rules[0].reverse);

        let serialized = toml::to_string_pretty(&config).unwrap();
        let reparsed: AppConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(reparsed, config);
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
