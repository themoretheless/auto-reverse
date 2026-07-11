//! Pure device-rule mutation used by the native tray quick-pick menu.

use crate::config::{DeviceRule, preferred_device_rule, with_device_rule_selection};
use crate::platform::macos::hid::DeviceInfo;

/// Cycles one device through Default -> Reverse -> Default.
///
/// `Don't reverse` is intentionally left untouched because the tray's
/// binary checkmark cannot represent the settings window's third state.
pub(super) fn toggle_device_rules(
    current_rules: &[DeviceRule],
    device: &DeviceInfo,
) -> Option<Vec<DeviceRule>> {
    let current_rule =
        preferred_device_rule(current_rules, &device.identity).map(|rule| rule.reverse);
    if current_rule == Some(false) {
        return None;
    }

    Some(with_device_rule_selection(
        current_rules,
        &device.identity,
        device.name.as_deref(),
        if current_rule == Some(true) {
            None
        } else {
            Some(true)
        },
    ))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::device::{DeviceIdentity, HardwareId};

    fn device(vendor_id: u32, product_id: u32) -> DeviceInfo {
        DeviceInfo {
            identity: DeviceIdentity::hardware_only(HardwareId {
                vendor_id,
                product_id,
            }),
            name: Some("Test Device".to_string()),
            transport: None,
        }
    }

    #[test]
    fn default_device_becomes_reversed() {
        let updated = toggle_device_rules(&[], &device(0x1, 0x2)).expect("should mutate");
        assert_eq!(
            updated,
            vec![DeviceRule::for_hardware(
                HardwareId {
                    vendor_id: 0x1,
                    product_id: 0x2,
                },
                Some("Test Device".to_string()),
                true,
            )]
        );
    }

    #[test]
    fn reversed_device_cycles_back_to_default() {
        let rules = vec![DeviceRule::for_hardware(
            HardwareId {
                vendor_id: 0x1,
                product_id: 0x2,
            },
            None,
            true,
        )];
        let updated = toggle_device_rules(&rules, &device(0x1, 0x2)).expect("should mutate");
        assert!(updated.is_empty());
    }

    #[test]
    fn explicit_dont_reverse_rule_is_never_touched() {
        let rules = vec![DeviceRule::for_hardware(
            HardwareId {
                vendor_id: 0x1,
                product_id: 0x2,
            },
            None,
            false,
        )];
        assert_eq!(toggle_device_rules(&rules, &device(0x1, 0x2)), None);
    }

    #[test]
    fn unrelated_devices_rules_are_left_untouched() {
        let other = DeviceRule::for_hardware(
            HardwareId {
                vendor_id: 0x9,
                product_id: 0x9,
            },
            None,
            true,
        );
        let rules = vec![other.clone()];
        let updated = toggle_device_rules(&rules, &device(0x1, 0x2)).expect("should mutate");
        assert!(updated.contains(&other));
        assert!(updated.iter().any(|rule| rule.vendor_id == 0x1));
    }

    #[test]
    fn identical_models_get_independent_serial_rules() {
        let hardware = HardwareId {
            vendor_id: 0x1,
            product_id: 0x2,
        };
        let first = DeviceInfo {
            identity: DeviceIdentity::new(hardware, Some(Arc::from("mouse-a")), Some(10)),
            name: Some("Twin Mouse".to_string()),
            transport: None,
        };
        let second = DeviceInfo {
            identity: DeviceIdentity::new(hardware, Some(Arc::from("mouse-b")), Some(11)),
            name: Some("Twin Mouse".to_string()),
            transport: None,
        };

        let rules = toggle_device_rules(&[], &first).expect("should mutate");

        assert_eq!(
            preferred_device_rule(&rules, &first.identity).map(|r| r.reverse),
            Some(true)
        );
        assert!(preferred_device_rule(&rules, &second.identity).is_none());
    }

    #[test]
    fn serial_toggle_never_removes_a_shared_legacy_fallback() {
        let hardware = HardwareId {
            vendor_id: 0x1,
            product_id: 0x2,
        };
        let target = DeviceInfo {
            identity: DeviceIdentity::new(hardware, Some(Arc::from("mouse-a")), Some(10)),
            name: Some("Twin Mouse".to_string()),
            transport: None,
        };
        let fallback = DeviceRule::for_hardware(hardware, None, false);

        let pinned = toggle_device_rules(std::slice::from_ref(&fallback), &target)
            .expect("should add a concrete override");
        let restored = toggle_device_rules(&pinned, &target)
            .expect("should remove only the concrete override");

        assert_eq!(restored, vec![fallback]);
    }
}
