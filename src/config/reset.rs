//! Pure, explicitly scoped configuration reset operations.

use crate::device::DeviceIdentity;

use super::schema::AppConfig;

pub fn without_device_profile(config: &AppConfig, identity: &DeviceIdentity) -> AppConfig {
    let mut reset = config.clone();
    reset
        .device_rules
        .retain(|rule| !rule.is_preferred_for(identity));
    reset
}

pub fn with_dynamics_defaults(config: &AppConfig) -> AppConfig {
    let defaults = AppConfig::default();
    let mut reset = config.clone();
    reset.discrete_scroll_step_size = defaults.discrete_scroll_step_size;
    reset.smooth_preset = defaults.smooth_preset;
    reset.show_discrete_scroll_options = defaults.show_discrete_scroll_options;

    for rule in &mut reset.device_rules {
        rule.step_size = None;
        rule.smooth_preset = None;
    }
    reset
        .device_rules
        .retain(super::schema::DeviceRule::has_profile_overrides);
    reset
}

/// Emergency rollback for a release/runtime dynamics gate. Unlike the user
/// facing dynamics reset, this clears only smooth-preset selection and leaves
/// wheel step size plus its per-device overrides untouched.
pub fn with_dynamics_rollback(config: &AppConfig) -> AppConfig {
    let mut rollback = config.clone();
    rollback.smooth_preset = crate::scroll_dynamics::SmoothPreset::Off;
    for rule in &mut rollback.device_rules {
        rule.smooth_preset = None;
    }
    rollback
        .device_rules
        .retain(super::schema::DeviceRule::has_profile_overrides);
    rollback
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::config::DeviceRule;
    use crate::device::HardwareId;
    use crate::scroll_dynamics::SmoothPreset;

    use super::*;

    fn identity(serial: &str) -> DeviceIdentity {
        DeviceIdentity::new(
            HardwareId {
                vendor_id: 1,
                product_id: 2,
            },
            Some(Arc::from(serial)),
            None,
        )
    }

    #[test]
    fn device_reset_removes_only_the_exact_profile() {
        let first = identity("first");
        let second = identity("second");
        let config = AppConfig {
            device_rules: vec![
                DeviceRule::for_identity(&first, None, true),
                DeviceRule::for_identity(&second, None, false),
            ],
            ..AppConfig::default()
        };

        let reset = without_device_profile(&config, &first);
        assert!(reset.preferred_device_rule(&first).is_none());
        assert!(reset.preferred_device_rule(&second).is_some());
    }

    #[test]
    fn dynamics_reset_preserves_direction_alias_and_unrelated_settings() {
        let target = identity("mouse");
        let config = AppConfig {
            enabled: false,
            reverse_horizontal: true,
            discrete_scroll_step_size: 9,
            smooth_preset: SmoothPreset::Fast,
            show_discrete_scroll_options: true,
            device_rules: vec![DeviceRule {
                alias: Some("Desk".to_string()),
                reverse: Some(false),
                step_size: Some(8),
                smooth_preset: Some(SmoothPreset::Balanced),
                ..DeviceRule::inheriting_for_identity(&target, None)
            }],
            ..AppConfig::default()
        };

        let reset = with_dynamics_defaults(&config);
        assert!(!reset.enabled);
        assert!(reset.reverse_horizontal);
        assert_eq!(reset.discrete_scroll_step_size, 3);
        assert_eq!(reset.smooth_preset, SmoothPreset::Off);
        assert!(!reset.show_discrete_scroll_options);
        assert_eq!(reset.device_rules[0].alias.as_deref(), Some("Desk"));
        assert_eq!(reset.device_rules[0].reverse, Some(false));
        assert_eq!(reset.device_rules[0].step_size, None);
        assert_eq!(reset.device_rules[0].smooth_preset, None);
    }

    #[test]
    fn dynamics_only_device_profile_is_removed_after_reset() {
        let target = identity("mouse");
        let config = AppConfig {
            device_rules: vec![DeviceRule {
                step_size: Some(8),
                smooth_preset: Some(SmoothPreset::Precise),
                ..DeviceRule::inheriting_for_identity(&target, None)
            }],
            ..AppConfig::default()
        };

        assert!(with_dynamics_defaults(&config).device_rules.is_empty());
    }

    #[test]
    fn release_rollback_clears_only_smooth_presets() {
        let target = identity("mouse");
        let config = AppConfig {
            discrete_scroll_step_size: 9,
            smooth_preset: SmoothPreset::Fast,
            device_rules: vec![DeviceRule {
                alias: Some("Desk".to_string()),
                reverse: Some(false),
                step_size: Some(8),
                smooth_preset: Some(SmoothPreset::Balanced),
                ..DeviceRule::inheriting_for_identity(&target, None)
            }],
            ..AppConfig::default()
        };

        let rollback = with_dynamics_rollback(&config);
        assert_eq!(rollback.smooth_preset, SmoothPreset::Off);
        assert_eq!(rollback.discrete_scroll_step_size, 9);
        assert_eq!(rollback.device_rules[0].alias.as_deref(), Some("Desk"));
        assert_eq!(rollback.device_rules[0].reverse, Some(false));
        assert_eq!(rollback.device_rules[0].step_size, Some(8));
        assert_eq!(rollback.device_rules[0].smooth_preset, None);
    }
}
