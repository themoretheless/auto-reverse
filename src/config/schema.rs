use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::scroll_dynamics::SmoothPreset;

pub const CONFIG_VERSION: u32 = 1;

/// A per-device override. `serial_number` identifies one physical device when
/// available; `location_id` is the port-level fallback. Omitting both keeps
/// the legacy behavior and matches every device with this vendor/product pair.
/// TOML supports hex literals, so a rule reads naturally:
///
/// ```toml
/// [[device_rules]]
/// vendor_id = 0x046d
/// product_id = 0xc52b
/// serial_number = "ABC123"       # preferred when available
/// name = "Logitech MX Master"  # optional, display only
/// reverse = true
/// alias = "Desk mouse"          # optional user label
/// step_size = 5                  # optional; otherwise inherit
/// smooth_preset = "balanced"    # optional; otherwise inherit
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceRule {
    pub vendor_id: u32,
    pub product_id: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serial_number: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location_id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reverse: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_size: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smooth_preset: Option<SmoothPreset>,
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
    pub smooth_preset: SmoothPreset,
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
            smooth_preset: SmoothPreset::Off,
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
            if let Some(serial) = &rule.serial_number {
                if serial.trim().is_empty() {
                    return Err(AppError::InvalidConfig(format!(
                        "device_rules[{index}].serial_number must not be empty"
                    )));
                }
                if serial.trim() != serial {
                    return Err(AppError::InvalidConfig(format!(
                        "device_rules[{index}].serial_number must not have surrounding whitespace"
                    )));
                }
            }
            if rule.serial_number.is_some() && rule.location_id.is_some() {
                return Err(AppError::InvalidConfig(format!(
                    "device_rules[{index}] must use serial_number or location_id, not both"
                )));
            }
            if rule.location_id == Some(0) {
                return Err(AppError::InvalidConfig(format!(
                    "device_rules[{index}].location_id must be non-zero"
                )));
            }
            if let Some(alias) = &rule.alias {
                if alias.trim().is_empty() {
                    return Err(AppError::InvalidConfig(format!(
                        "device_rules[{index}].alias must not be empty"
                    )));
                }
                if alias.trim() != alias {
                    return Err(AppError::InvalidConfig(format!(
                        "device_rules[{index}].alias must not have surrounding whitespace"
                    )));
                }
                if alias.chars().count() > 64 || alias.chars().any(char::is_control) {
                    return Err(AppError::InvalidConfig(format!(
                        "device_rules[{index}].alias must be at most 64 visible characters"
                    )));
                }
            }
            if rule
                .step_size
                .is_some_and(|value| !(0..=20).contains(&value))
            {
                return Err(AppError::InvalidConfig(format!(
                    "device_rules[{index}].step_size must be between 0 and 20"
                )));
            }

            let duplicate = self.device_rules[..index]
                .iter()
                .any(|earlier| earlier.has_same_selector(rule));
            if duplicate {
                return Err(AppError::InvalidConfig(format!(
                    "duplicate device_rules selector: {}",
                    rule.selector_description()
                )));
            }
        }

        Ok(())
    }

    /// One plain-English sentence describing what this config actually does.
    /// Shared by `doctor` and the settings window so the two can never
    /// drift apart in how they explain the same state.
    pub fn plain_english_summary(&self) -> String {
        if !self.enabled {
            return "not reversing anything right now (disabled)".to_string();
        }

        let mut targets = Vec::new();
        if self.reverse_mouse {
            targets.push("a physical mouse wheel");
        }
        if self.reverse_trackpad {
            targets.push("trackpad scrolling");
        }
        if self.reverse_magic_mouse {
            targets.push("Magic Mouse scrolling");
        }
        if targets.is_empty() {
            // Per-device rules can turn a device ON even when every
            // per-kind flag is off - exactly the config the GUI builds when
            // the user unchecks Mouse/Trackpad and pins one device to
            // "Reverse". Without this branch both doctor and the GUI would
            // claim nothing is reversed while the tap reverses that device.
            let pinned_on = self
                .device_rules
                .iter()
                .filter(|rule| rule.reverse == Some(true))
                .count();
            if pinned_on > 0 {
                let noun = if pinned_on == 1 { "rule" } else { "rules" };
                return format!("reversing only devices enabled by {pinned_on} per-device {noun}");
            }
            return "enabled, but no device is currently set to reverse".to_string();
        }

        let axes = match (self.reverse_vertical, self.reverse_horizontal) {
            (true, true) => "vertical and horizontal",
            (true, false) => "vertical",
            (false, true) => "horizontal",
            (false, false) => "no axis - nothing will actually flip",
        };
        let target_summary = match targets.as_slice() {
            [only] => (*only).to_string(),
            [first, second] => format!("{first} and {second}"),
            [first, second, third] => format!("{first}, {second}, and {third}"),
            _ => unreachable!("the config has exactly three device-kind flags"),
        };
        let base = format!("reversing {axes} scroll for {target_summary}");
        if self.device_rules.is_empty() {
            base
        } else {
            // Rules can flip the outcome for specific devices, so the
            // summary must not claim the per-kind behavior is the whole
            // story.
            format!(
                "{base}, with {} per-device rule(s) overriding specific mice",
                self.device_rules.len()
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::device::{DeviceIdentity, DeviceKind, HardwareId};

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
        assert_eq!(config.smooth_preset, SmoothPreset::Off);
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
        let logitech = DeviceIdentity::hardware_only(HardwareId {
            vendor_id: 0x046d,
            product_id: 0xc52b,
        });
        let razer = DeviceIdentity::hardware_only(HardwareId {
            vendor_id: 0x1532,
            product_id: 0x0067,
        });
        let config = AppConfig {
            reverse_mouse: true,
            device_rules: vec![DeviceRule::for_hardware(logitech.hardware, None, false)],
            ..AppConfig::default()
        };

        // The legacy rule pins this hardware model off even though mice reverse.
        assert!(!config.should_reverse(DeviceKind::Mouse, Some(&logitech)));
        // A device without a rule falls back to the kind flag.
        assert!(config.should_reverse(DeviceKind::Mouse, Some(&razer)));
        // Unknown hardware falls back to the kind flag too.
        assert!(config.should_reverse(DeviceKind::Mouse, None));
    }

    #[test]
    fn summary_reports_rule_only_reversal_when_all_kind_flags_are_off() {
        // Regression test for a merge-created contradiction: with every
        // per-kind flag off but one rule pinning a device ON, the summary
        // used to say "no device is currently set to reverse" while the
        // tap genuinely reversed that device.
        let config = AppConfig {
            reverse_mouse: false,
            reverse_trackpad: false,
            reverse_magic_mouse: false,
            device_rules: vec![DeviceRule::for_hardware(
                HardwareId {
                    vendor_id: 0x046d,
                    product_id: 0xc52b,
                },
                None,
                true,
            )],
            ..AppConfig::default()
        };

        assert_eq!(
            config.plain_english_summary(),
            "reversing only devices enabled by 1 per-device rule"
        );

        // A do-not-reverse-only rule set really does reverse nothing, so
        // the old wording stays correct there.
        let exempt_only = AppConfig {
            reverse_mouse: false,
            reverse_trackpad: false,
            reverse_magic_mouse: false,
            device_rules: vec![DeviceRule::for_hardware(
                HardwareId {
                    vendor_id: 0x046d,
                    product_id: 0xc52b,
                },
                None,
                false,
            )],
            ..AppConfig::default()
        };
        assert_eq!(
            exempt_only.plain_english_summary(),
            "enabled, but no device is currently set to reverse"
        );
    }

    #[test]
    fn summary_names_trackpad_and_magic_mouse_as_separate_targets() {
        let config = AppConfig {
            reverse_mouse: false,
            reverse_trackpad: true,
            reverse_magic_mouse: true,
            ..AppConfig::default()
        };

        assert_eq!(
            config.plain_english_summary(),
            "reversing vertical scroll for trackpad scrolling and Magic Mouse scrolling"
        );
    }

    #[test]
    fn summary_punctuates_all_three_targets_readably() {
        let config = AppConfig {
            reverse_mouse: true,
            reverse_trackpad: true,
            reverse_magic_mouse: true,
            ..AppConfig::default()
        };

        assert_eq!(
            config.plain_english_summary(),
            "reversing vertical scroll for a physical mouse wheel, trackpad scrolling, and Magic Mouse scrolling"
        );
    }

    #[test]
    fn duplicate_device_rules_are_rejected() {
        let rule = DeviceRule::for_hardware(
            HardwareId {
                vendor_id: 1,
                product_id: 2,
            },
            None,
            true,
        );
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
             serial_number = \"ABC123\"\n\
             name = \"Logitech MX Master\"\n\
             reverse = true\n\
             step_size = 7\n\
             smooth_preset = \"balanced\"\n",
        )
        .unwrap();

        assert_eq!(config.device_rules.len(), 1);
        assert_eq!(config.device_rules[0].vendor_id, 0x046d);
        assert_eq!(
            config.device_rules[0].serial_number.as_deref(),
            Some("ABC123")
        );
        assert_eq!(config.device_rules[0].reverse, Some(true));
        assert_eq!(config.device_rules[0].step_size, Some(7));
        assert_eq!(
            config.device_rules[0].smooth_preset,
            Some(SmoothPreset::Balanced)
        );

        let serialized = toml::to_string_pretty(&config).unwrap();
        let reparsed: AppConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(reparsed, config);
    }

    #[test]
    fn legacy_hardware_wide_rule_still_loads_without_a_version_bump() {
        let config: AppConfig = toml::from_str(
            "config_version = 1\n\
             [[device_rules]]\n\
             vendor_id = 0x046d\n\
             product_id = 0xc52b\n\
             reverse = false\n",
        )
        .unwrap();

        assert!(config.validate().is_ok());
        assert!(config.device_rules[0].is_hardware_wide());
        assert_eq!(config.device_rules[0].serial_number, None);
        assert_eq!(config.device_rules[0].location_id, None);
        assert_eq!(config.device_rules[0].step_size, None);
        assert_eq!(config.device_rules[0].smooth_preset, None);
        assert_eq!(config.smooth_preset, SmoothPreset::Off);
    }

    #[test]
    fn invalid_per_device_step_size_is_rejected() {
        let config = AppConfig {
            device_rules: vec![DeviceRule {
                step_size: Some(21),
                ..DeviceRule::for_hardware(
                    HardwareId {
                        vendor_id: 1,
                        product_id: 2,
                    },
                    None,
                    true,
                )
            }],
            ..AppConfig::default()
        };

        assert!(config.validate().is_err());
    }

    #[test]
    fn direction_can_inherit_in_a_profile_with_other_overrides() {
        let config: AppConfig = toml::from_str(
            "config_version = 1\n\
             [[device_rules]]\n\
             vendor_id = 0x046d\n\
             product_id = 0xc52b\n\
             serial_number = \"mouse-a\"\n\
             alias = \"Desk mouse\"\n\
             step_size = 8\n",
        )
        .unwrap();

        config.validate().unwrap();
        assert_eq!(config.device_rules[0].reverse, None);
        assert_eq!(config.device_rules[0].alias.as_deref(), Some("Desk mouse"));
        assert_eq!(config.device_rules[0].step_size, Some(8));
    }

    #[test]
    fn aliases_are_trimmed_bounded_and_free_of_control_characters() {
        for alias in ["", " leading", "trailing ", "line\nbreak"] {
            let config = AppConfig {
                device_rules: vec![DeviceRule {
                    alias: Some(alias.to_string()),
                    ..DeviceRule::for_hardware(
                        HardwareId {
                            vendor_id: 1,
                            product_id: 2,
                        },
                        None,
                        true,
                    )
                }],
                ..AppConfig::default()
            };
            assert!(config.validate().is_err(), "alias {alias:?}");
        }

        let config = AppConfig {
            device_rules: vec![DeviceRule {
                alias: Some("x".repeat(65)),
                ..DeviceRule::for_hardware(
                    HardwareId {
                        vendor_id: 1,
                        product_id: 2,
                    },
                    None,
                    true,
                )
            }],
            ..AppConfig::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn different_serials_for_identical_hardware_are_valid() {
        let hardware = HardwareId {
            vendor_id: 1,
            product_id: 2,
        };
        let config = AppConfig {
            device_rules: vec![
                DeviceRule {
                    serial_number: Some("mouse-a".to_string()),
                    ..DeviceRule::for_hardware(hardware, None, true)
                },
                DeviceRule {
                    serial_number: Some("mouse-b".to_string()),
                    ..DeviceRule::for_hardware(hardware, None, false)
                },
            ],
            ..AppConfig::default()
        };

        assert!(config.validate().is_ok());
    }

    #[test]
    fn ambiguous_or_empty_identity_qualifiers_are_rejected() {
        let hardware = HardwareId {
            vendor_id: 1,
            product_id: 2,
        };
        let both = AppConfig {
            device_rules: vec![DeviceRule {
                serial_number: Some("mouse-a".to_string()),
                location_id: Some(7),
                ..DeviceRule::for_hardware(hardware, None, true)
            }],
            ..AppConfig::default()
        };
        assert!(both.validate().is_err());

        let blank = AppConfig {
            device_rules: vec![DeviceRule {
                serial_number: Some("  ".to_string()),
                ..DeviceRule::for_hardware(hardware, None, true)
            }],
            ..AppConfig::default()
        };
        assert!(blank.validate().is_err());
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
