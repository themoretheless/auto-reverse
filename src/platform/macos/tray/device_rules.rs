//! Pure device-rule mutation used by the native tray quick-pick menu.

use crate::config::DeviceRule;
use crate::platform::macos::hid::DeviceInfo;

/// Cycles one device through Default -> Reverse -> Default.
///
/// `Don't reverse` is intentionally left untouched because the tray's
/// binary checkmark cannot represent the settings window's third state.
pub(super) fn toggle_device_rules(
    current_rules: &[DeviceRule],
    device: &DeviceInfo,
) -> Option<Vec<DeviceRule>> {
    let current_rule = current_rules
        .iter()
        .find(|rule| rule.matches(device.hardware))
        .map(|rule| rule.reverse);
    if current_rule == Some(false) {
        return None;
    }

    let mut updated: Vec<DeviceRule> = current_rules
        .iter()
        .filter(|rule| !rule.matches(device.hardware))
        .cloned()
        .collect();
    if current_rule != Some(true) {
        updated.push(DeviceRule {
            vendor_id: device.hardware.vendor_id,
            product_id: device.hardware.product_id,
            name: device.name.clone(),
            reverse: true,
        });
    }
    Some(updated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::HardwareId;

    fn device(vendor_id: u32, product_id: u32) -> DeviceInfo {
        DeviceInfo {
            hardware: HardwareId {
                vendor_id,
                product_id,
            },
            name: Some("Test Device".to_string()),
            transport: None,
        }
    }

    #[test]
    fn default_device_becomes_reversed() {
        let updated = toggle_device_rules(&[], &device(0x1, 0x2)).expect("should mutate");
        assert_eq!(
            updated,
            vec![DeviceRule {
                vendor_id: 0x1,
                product_id: 0x2,
                name: Some("Test Device".to_string()),
                reverse: true,
            }]
        );
    }

    #[test]
    fn reversed_device_cycles_back_to_default() {
        let rules = vec![DeviceRule {
            vendor_id: 0x1,
            product_id: 0x2,
            name: None,
            reverse: true,
        }];
        let updated = toggle_device_rules(&rules, &device(0x1, 0x2)).expect("should mutate");
        assert!(updated.is_empty());
    }

    #[test]
    fn explicit_dont_reverse_rule_is_never_touched() {
        let rules = vec![DeviceRule {
            vendor_id: 0x1,
            product_id: 0x2,
            name: None,
            reverse: false,
        }];
        assert_eq!(toggle_device_rules(&rules, &device(0x1, 0x2)), None);
    }

    #[test]
    fn unrelated_devices_rules_are_left_untouched() {
        let other = DeviceRule {
            vendor_id: 0x9,
            product_id: 0x9,
            name: None,
            reverse: true,
        };
        let rules = vec![other.clone()];
        let updated = toggle_device_rules(&rules, &device(0x1, 0x2)).expect("should mutate");
        assert!(updated.contains(&other));
        assert!(updated.iter().any(|rule| rule.vendor_id == 0x1));
    }
}
