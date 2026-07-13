//! Field-by-field resolution over the existing `device_rules` collection.

use crate::device::{DeviceIdentity, DeviceKind};
use crate::scroll_dynamics::SmoothPreset;

use super::schema::{AppConfig, DeviceRule};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileSource {
    ExactSerial,
    ExactLocation,
    Hardware,
    DeviceKind(DeviceKind),
    GlobalDefault,
}

impl ProfileSource {
    pub const fn is_device_rule(self) -> bool {
        matches!(
            self,
            Self::ExactSerial | Self::ExactLocation | Self::Hardware
        )
    }

    pub const fn code(self) -> &'static str {
        match self {
            Self::ExactSerial => "exact_serial",
            Self::ExactLocation => "exact_location",
            Self::Hardware => "hardware",
            Self::DeviceKind(DeviceKind::Mouse) => "mouse_kind",
            Self::DeviceKind(DeviceKind::Trackpad) => "trackpad_kind",
            Self::DeviceKind(DeviceKind::MagicMouse) => "magic_mouse_kind",
            Self::DeviceKind(DeviceKind::Unknown) => "unknown_kind",
            Self::GlobalDefault => "global_default",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::ExactSerial => "exact serial rule",
            Self::ExactLocation => "port rule",
            Self::Hardware => "shared hardware rule",
            Self::DeviceKind(DeviceKind::Mouse) => "mouse setting",
            Self::DeviceKind(DeviceKind::Trackpad) => "trackpad setting",
            Self::DeviceKind(DeviceKind::MagicMouse) => "Magic Mouse setting",
            Self::DeviceKind(DeviceKind::Unknown) => "unknown-device setting",
            Self::GlobalDefault => "global default",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedProfileValue<T> {
    pub value: T,
    pub source: ProfileSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedDeviceProfile {
    pub reverse: ResolvedProfileValue<bool>,
    pub step_size: ResolvedProfileValue<i64>,
    pub smooth_preset: ResolvedProfileValue<SmoothPreset>,
}

impl AppConfig {
    pub fn device_alias<'a>(&'a self, identity: &DeviceIdentity) -> Option<&'a str> {
        self.device_rules
            .iter()
            .filter(|rule| rule.matches(identity))
            .filter_map(|rule| rule.alias.as_deref().map(|alias| (rule, alias)))
            .max_by_key(|(rule, _)| rule.specificity())
            .map(|(_, alias)| alias)
    }

    pub fn resolve_device_profile(
        &self,
        device_kind: DeviceKind,
        identity: Option<&DeviceIdentity>,
    ) -> ResolvedDeviceProfile {
        let reverse = resolve_rule_value(&self.device_rules, identity, |rule| rule.reverse)
            .unwrap_or(ResolvedProfileValue {
                value: self.should_reverse_device(device_kind),
                source: ProfileSource::DeviceKind(device_kind),
            });
        let step_size = resolve_rule_value(&self.device_rules, identity, |rule| rule.step_size)
            .unwrap_or(ResolvedProfileValue {
                value: self.discrete_scroll_step_size,
                source: ProfileSource::GlobalDefault,
            });
        let smooth_preset =
            resolve_rule_value(&self.device_rules, identity, |rule| rule.smooth_preset).unwrap_or(
                ResolvedProfileValue {
                    value: self.smooth_preset,
                    source: ProfileSource::GlobalDefault,
                },
            );

        ResolvedDeviceProfile {
            reverse,
            step_size,
            smooth_preset,
        }
    }
}

fn resolve_rule_value<T: Copy>(
    rules: &[DeviceRule],
    identity: Option<&DeviceIdentity>,
    value: impl Fn(&DeviceRule) -> Option<T>,
) -> Option<ResolvedProfileValue<T>> {
    let identity = identity?;
    rules
        .iter()
        .filter(|rule| rule.matches(identity))
        .filter_map(|rule| value(rule).map(|value| (rule, value)))
        .max_by_key(|(rule, _)| rule.specificity())
        .map(|(rule, value)| ResolvedProfileValue {
            value,
            source: source_for_rule(rule),
        })
}

fn source_for_rule(rule: &DeviceRule) -> ProfileSource {
    if rule.serial_number.is_some() {
        ProfileSource::ExactSerial
    } else if rule.location_id.is_some() {
        ProfileSource::ExactLocation
    } else {
        ProfileSource::Hardware
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::device::HardwareId;

    use super::*;

    fn identity() -> DeviceIdentity {
        DeviceIdentity::new(
            HardwareId {
                vendor_id: 0x046d,
                product_id: 0xc52b,
            },
            Some(Arc::from("mouse-a")),
            Some(42),
        )
    }

    #[test]
    fn fields_resolve_independently_through_fixed_specificity_order() {
        let identity = identity();
        let hardware = DeviceRule {
            step_size: Some(4),
            smooth_preset: Some(SmoothPreset::Precise),
            ..DeviceRule::for_hardware(identity.hardware, None, false)
        };
        let location = DeviceRule {
            location_id: Some(42),
            smooth_preset: Some(SmoothPreset::Balanced),
            ..DeviceRule::for_hardware(identity.hardware, None, true)
        };
        let serial = DeviceRule {
            step_size: Some(9),
            ..DeviceRule::for_identity(&identity, None, false)
        };
        let config = AppConfig {
            device_rules: vec![serial, hardware, location],
            ..AppConfig::default()
        };

        let resolved = config.resolve_device_profile(DeviceKind::Mouse, Some(&identity));

        assert_eq!(
            resolved.reverse,
            ResolvedProfileValue {
                value: false,
                source: ProfileSource::ExactSerial,
            }
        );
        assert_eq!(
            resolved.step_size,
            ResolvedProfileValue {
                value: 9,
                source: ProfileSource::ExactSerial,
            }
        );
        assert_eq!(
            resolved.smooth_preset,
            ResolvedProfileValue {
                value: SmoothPreset::Balanced,
                source: ProfileSource::ExactLocation,
            }
        );
    }

    #[test]
    fn kind_and_global_values_are_explicit_fallback_sources() {
        let config = AppConfig {
            reverse_trackpad: true,
            discrete_scroll_step_size: 7,
            smooth_preset: SmoothPreset::Fast,
            ..AppConfig::default()
        };

        let resolved = config.resolve_device_profile(DeviceKind::Trackpad, None);

        assert_eq!(
            resolved.reverse.source,
            ProfileSource::DeviceKind(DeviceKind::Trackpad)
        );
        assert!(resolved.reverse.value);
        assert_eq!(resolved.step_size.value, 7);
        assert_eq!(resolved.step_size.source, ProfileSource::GlobalDefault);
        assert_eq!(resolved.smooth_preset.value, SmoothPreset::Fast);
        assert_eq!(resolved.smooth_preset.source, ProfileSource::GlobalDefault);
    }

    #[test]
    fn config_order_cannot_change_resolution() {
        let identity = identity();
        let serial = DeviceRule {
            step_size: Some(8),
            ..DeviceRule::for_identity(&identity, None, true)
        };
        let hardware = DeviceRule {
            step_size: Some(2),
            ..DeviceRule::for_hardware(identity.hardware, None, false)
        };
        let mut first = AppConfig {
            device_rules: vec![serial.clone(), hardware.clone()],
            ..AppConfig::default()
        };
        let expected = first.resolve_device_profile(DeviceKind::Mouse, Some(&identity));
        first.device_rules = vec![hardware, serial];

        assert_eq!(
            first.resolve_device_profile(DeviceKind::Mouse, Some(&identity)),
            expected
        );
    }

    #[test]
    fn exact_rule_can_inherit_direction_while_overriding_other_fields() {
        let identity = identity();
        let hardware = DeviceRule::for_hardware(identity.hardware, None, false);
        let serial = DeviceRule {
            reverse: None,
            step_size: Some(11),
            ..DeviceRule::for_identity(&identity, None, true)
        };
        let config = AppConfig {
            device_rules: vec![serial, hardware],
            ..AppConfig::default()
        };

        let resolved = config.resolve_device_profile(DeviceKind::Mouse, Some(&identity));

        assert!(!resolved.reverse.value);
        assert_eq!(resolved.reverse.source, ProfileSource::Hardware);
        assert_eq!(resolved.step_size.value, 11);
        assert_eq!(resolved.step_size.source, ProfileSource::ExactSerial);
    }
}
