//! Pure matching and mutation for per-device scroll rules.

use crate::device::{DeviceIdentity, DeviceKind, HardwareId};

use super::schema::{AppConfig, DeviceRule};

impl DeviceRule {
    pub fn for_hardware(hardware: HardwareId, name: Option<String>, reverse: bool) -> Self {
        Self {
            vendor_id: hardware.vendor_id,
            product_id: hardware.product_id,
            serial_number: None,
            location_id: None,
            name,
            reverse,
        }
    }

    /// Creates the narrowest rule supported by this device: serial first,
    /// connection location second, legacy vendor/product scope last.
    pub fn for_identity(identity: &DeviceIdentity, name: Option<String>, reverse: bool) -> Self {
        let serial_number = identity.serial_number.as_deref().map(str::to_owned);
        let location_id = if serial_number.is_none() {
            identity.location_id
        } else {
            None
        };
        Self {
            vendor_id: identity.hardware.vendor_id,
            product_id: identity.hardware.product_id,
            serial_number,
            location_id,
            name,
            reverse,
        }
    }

    pub fn matches(&self, identity: &DeviceIdentity) -> bool {
        if self.vendor_id != identity.hardware.vendor_id
            || self.product_id != identity.hardware.product_id
        {
            return false;
        }

        match (&self.serial_number, self.location_id) {
            (Some(serial), None) => identity.serial_number.as_deref() == Some(serial.as_str()),
            (None, Some(location)) => identity.location_id == Some(location),
            (None, None) => true,
            (Some(_), Some(_)) => false,
        }
    }

    pub fn is_hardware_wide(&self) -> bool {
        self.serial_number.is_none() && self.location_id.is_none()
    }

    pub(crate) fn is_preferred_for(&self, identity: &DeviceIdentity) -> bool {
        if let Some(serial) = &identity.serial_number {
            self.vendor_id == identity.hardware.vendor_id
                && self.product_id == identity.hardware.product_id
                && self.serial_number.as_deref() == Some(serial.as_ref())
                && self.location_id.is_none()
        } else if let Some(location) = identity.location_id {
            self.vendor_id == identity.hardware.vendor_id
                && self.product_id == identity.hardware.product_id
                && self.serial_number.is_none()
                && self.location_id == Some(location)
        } else {
            self.vendor_id == identity.hardware.vendor_id
                && self.product_id == identity.hardware.product_id
                && self.is_hardware_wide()
        }
    }

    pub(crate) fn has_same_selector(&self, other: &Self) -> bool {
        self.vendor_id == other.vendor_id
            && self.product_id == other.product_id
            && self.serial_number == other.serial_number
            && self.location_id == other.location_id
    }

    pub fn selector_description(&self) -> String {
        let qualifier = if let Some(serial) = &self.serial_number {
            format!(" serial_number={serial:?}")
        } else if let Some(location) = self.location_id {
            format!(" location_id=0x{location:08x}")
        } else {
            " (all devices with this hardware ID)".to_string()
        };
        format!(
            "vendor_id=0x{:04x} product_id=0x{:04x}{qualifier}",
            self.vendor_id, self.product_id
        )
    }

    fn specificity(&self) -> u8 {
        if self.serial_number.is_some() {
            2
        } else if self.location_id.is_some() {
            1
        } else {
            0
        }
    }
}

impl AppConfig {
    /// Returns the most specific matching rule, independent of config order.
    pub fn matching_device_rule(&self, identity: &DeviceIdentity) -> Option<&DeviceRule> {
        matching_device_rule(&self.device_rules, identity)
    }

    /// Returns only the rule represented by the concrete device's UI control,
    /// excluding less-specific inherited fallbacks.
    pub fn preferred_device_rule(&self, identity: &DeviceIdentity) -> Option<&DeviceRule> {
        preferred_device_rule(&self.device_rules, identity)
    }

    /// Full reversal policy: the narrowest matching physical-device rule wins;
    /// otherwise the per-kind flag decides.
    pub fn should_reverse(
        &self,
        device_kind: DeviceKind,
        identity: Option<&DeviceIdentity>,
    ) -> bool {
        if let Some(rule) = identity.and_then(|value| self.matching_device_rule(value)) {
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

/// Returns the most specific matching rule, independent of config order.
pub fn matching_device_rule<'a>(
    rules: &'a [DeviceRule],
    identity: &DeviceIdentity,
) -> Option<&'a DeviceRule> {
    rules
        .iter()
        .filter(|rule| rule.matches(identity))
        .max_by_key(|rule| rule.specificity())
}

/// Returns the rule using the narrowest selector this identity supports.
pub fn preferred_device_rule<'a>(
    rules: &'a [DeviceRule],
    identity: &DeviceIdentity,
) -> Option<&'a DeviceRule> {
    rules.iter().find(|rule| rule.is_preferred_for(identity))
}

/// Produces a new rule list for the UI/tray three-state control.
///
/// Every selection replaces only that device's preferred selector. Therefore
/// `Default` means "no concrete override": an older hardware-wide or port
/// fallback may still be inherited, but changing one serial-numbered device
/// never silently changes its identical siblings.
pub fn with_device_rule_selection(
    current_rules: &[DeviceRule],
    identity: &DeviceIdentity,
    name: Option<&str>,
    selection: Option<bool>,
) -> Vec<DeviceRule> {
    let mut updated: Vec<DeviceRule> = current_rules
        .iter()
        .filter(|rule| !rule.is_preferred_for(identity))
        .cloned()
        .collect();

    if let Some(reverse) = selection {
        updated.push(DeviceRule::for_identity(
            identity,
            name.map(str::to_owned),
            reverse,
        ));
    }
    updated
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn identity(serial: Option<&str>, location_id: Option<u32>) -> DeviceIdentity {
        DeviceIdentity::new(
            HardwareId {
                vendor_id: 0x046d,
                product_id: 0xc52b,
            },
            serial.map(Arc::from),
            location_id,
        )
    }

    #[test]
    fn serial_rule_wins_over_location_and_hardware_wide_rules() {
        let target = identity(Some("mouse-b"), Some(42));
        let config = AppConfig {
            device_rules: vec![
                DeviceRule::for_hardware(target.hardware, None, false),
                DeviceRule {
                    location_id: Some(42),
                    reverse: false,
                    ..DeviceRule::for_hardware(target.hardware, None, false)
                },
                DeviceRule::for_identity(&target, None, true),
            ],
            ..AppConfig::default()
        };

        assert!(config.should_reverse(DeviceKind::Mouse, Some(&target)));
    }

    #[test]
    fn serial_rules_distinguish_identical_hardware() {
        let first = identity(Some("mouse-a"), Some(10));
        let second = identity(Some("mouse-b"), Some(10));
        let config = AppConfig {
            reverse_mouse: false,
            device_rules: vec![DeviceRule::for_identity(&first, None, true)],
            ..AppConfig::default()
        };

        assert!(config.should_reverse(DeviceKind::Mouse, Some(&first)));
        assert!(!config.should_reverse(DeviceKind::Mouse, Some(&second)));
    }

    #[test]
    fn location_rule_is_used_when_no_serial_rule_matches() {
        let first_port = identity(None, Some(10));
        let second_port = identity(None, Some(11));
        let config = AppConfig {
            reverse_mouse: false,
            device_rules: vec![DeviceRule::for_identity(&first_port, None, true)],
            ..AppConfig::default()
        };

        assert!(config.should_reverse(DeviceKind::Mouse, Some(&first_port)));
        assert!(!config.should_reverse(DeviceKind::Mouse, Some(&second_port)));
    }

    #[test]
    fn default_selection_clears_only_the_concrete_rule() {
        let target = identity(Some("mouse-a"), Some(10));
        let rules = vec![
            DeviceRule::for_hardware(target.hardware, None, false),
            DeviceRule::for_identity(&target, None, true),
        ];

        let updated = with_device_rule_selection(&rules, &target, None, None);

        assert_eq!(updated.len(), 1);
        assert!(updated[0].is_hardware_wide());
        assert!(preferred_device_rule(&updated, &target).is_none());
        assert_eq!(
            matching_device_rule(&updated, &target).map(|rule| rule.reverse),
            Some(false)
        );
    }

    #[test]
    fn concrete_selection_preserves_hardware_wide_fallback() {
        let target = identity(Some("mouse-a"), Some(10));
        let fallback = DeviceRule::for_hardware(target.hardware, None, false);

        let updated =
            with_device_rule_selection(std::slice::from_ref(&fallback), &target, None, Some(true));

        assert!(updated.contains(&fallback));
        assert_eq!(updated.len(), 2);
        let config = AppConfig {
            device_rules: updated,
            ..AppConfig::default()
        };
        assert!(config.should_reverse(DeviceKind::Mouse, Some(&target)));
    }
}
