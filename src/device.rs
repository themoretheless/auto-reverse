use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeviceKind {
    Mouse,
    Trackpad,
    MagicMouse,
    Unknown,
}

/// Hardware model reported by IOKit. Multiple physical devices can share the
/// same vendor/product pair, so this is not a complete device identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HardwareId {
    pub vendor_id: u32,
    pub product_id: u32,
}

/// Best available identity for one physical HID device.
///
/// Serial number is preferred because it normally survives reconnects and
/// port changes. `location_id` is a useful fallback for devices that expose no
/// serial, but identifies the connection location and can change when the
/// device is moved to another USB port. Both qualifiers are optional so old
/// vendor/product-only rules remain usable.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DeviceIdentity {
    pub hardware: HardwareId,
    pub serial_number: Option<Arc<str>>,
    pub location_id: Option<u32>,
}

impl DeviceIdentity {
    pub fn new(
        hardware: HardwareId,
        serial_number: Option<Arc<str>>,
        location_id: Option<u32>,
    ) -> Self {
        let serial_number = serial_number.and_then(|serial| {
            let trimmed = serial.trim();
            if trimmed.is_empty() {
                None
            } else if trimmed.len() == serial.len() {
                Some(serial)
            } else {
                Some(Arc::from(trimmed))
            }
        });

        Self {
            hardware,
            serial_number,
            location_id: location_id.filter(|location| *location != 0),
        }
    }

    pub fn hardware_only(hardware: HardwareId) -> Self {
        Self::new(hardware, None, None)
    }

    /// Compact, privacy-conscious discriminator shared by settings and tray.
    /// Full serials remain available through `Display` for explicit CLI use.
    pub fn compact_qualifier(&self) -> Option<String> {
        if let Some(serial) = &self.serial_number {
            let suffix_reversed: String = serial.chars().rev().take(12).collect();
            let suffix: String = suffix_reversed.chars().rev().collect();
            return Some(if suffix.len() == serial.len() {
                format!("serial {serial}")
            } else {
                format!("serial …{suffix}")
            });
        }
        self.location_id
            .map(|location| format!("port 0x{location:08x}"))
    }
}

impl fmt::Display for HardwareId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "vendor_id=0x{:04x} product_id=0x{:04x}",
            self.vendor_id, self.product_id
        )
    }
}

impl fmt::Display for DeviceIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.hardware)?;
        if let Some(serial) = &self.serial_number {
            write!(f, " serial_number={serial:?}")?;
        }
        if let Some(location) = self.location_id {
            write!(f, " location_id=0x{location:08x}")?;
        }
        Ok(())
    }
}

impl DeviceKind {
    /// Canonical lowercase, hyphenated name - the single source of truth
    /// `Display` and `FromStr` both build on, instead of each hand-rolling
    /// its own copy of the same four strings.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mouse => "mouse",
            Self::Trackpad => "trackpad",
            Self::MagicMouse => "magic-mouse",
            Self::Unknown => "unknown",
        }
    }
}

/// Single source of truth for describing `conservative_kind_from_continuity`
/// to a user (e.g. in the `doctor` command), so the CLI's explanation of the
/// classifier can't drift out of sync with what the classifier actually does.
pub const CLASSIFIER_DESCRIPTION: &str =
    "physical wheel = mouse, continuous scroll = trackpad-like";

/// The only device classifier actually wired into the live event tap today.
/// `IsContinuous` is the one signal CGEventTap's public API exposes, and it
/// cannot distinguish a Magic Mouse from a trackpad - both report continuous
/// scrolling identically - so continuous scroll is conservatively treated as
/// Trackpad. See recommendation.md for why `reverse_magic_mouse` currently
/// has no effect in practice.
pub fn conservative_kind_from_continuity(continuous: bool) -> DeviceKind {
    if continuous {
        DeviceKind::Trackpad
    } else {
        DeviceKind::Mouse
    }
}

impl fmt::Display for DeviceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for DeviceKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "mouse" => Ok(Self::Mouse),
            "trackpad" => Ok(Self::Trackpad),
            "magic-mouse" | "magic_mouse" => Ok(Self::MagicMouse),
            "unknown" => Ok(Self::Unknown),
            other => Err(format!(
                "unknown device kind `{other}`; expected mouse, trackpad, magic-mouse or unknown"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_continuous_scroll_is_mouse() {
        assert_eq!(conservative_kind_from_continuity(false), DeviceKind::Mouse);
    }

    #[test]
    fn continuous_scroll_is_trackpad() {
        assert_eq!(
            conservative_kind_from_continuity(true),
            DeviceKind::Trackpad
        );
    }

    #[test]
    fn display_and_from_str_round_trip_for_every_variant() {
        for kind in [
            DeviceKind::Mouse,
            DeviceKind::Trackpad,
            DeviceKind::MagicMouse,
            DeviceKind::Unknown,
        ] {
            assert_eq!(kind.to_string().parse::<DeviceKind>().unwrap(), kind);
        }
    }

    #[test]
    fn device_identity_normalizes_unusable_qualifiers() {
        let hardware = HardwareId {
            vendor_id: 1,
            product_id: 2,
        };

        let normalized = DeviceIdentity::new(hardware, Some(Arc::from("  ABC-123  ")), Some(0));
        assert_eq!(normalized.serial_number.as_deref(), Some("ABC-123"));
        assert_eq!(normalized.location_id, None);

        let blank = DeviceIdentity::new(hardware, Some(Arc::from("  ")), Some(7));
        assert_eq!(blank.serial_number, None);
        assert_eq!(blank.location_id, Some(7));
    }

    #[test]
    fn compact_qualifier_bounds_serial_and_names_location_as_port() {
        let hardware = HardwareId {
            vendor_id: 1,
            product_id: 2,
        };
        let serial = DeviceIdentity::new(hardware, Some(Arc::from("1234567890abcdef")), Some(42));
        let location = DeviceIdentity::new(hardware, None, Some(42));

        assert_eq!(
            serial.compact_qualifier().as_deref(),
            Some("serial …567890abcdef")
        );
        assert_eq!(
            location.compact_qualifier().as_deref(),
            Some("port 0x0000002a")
        );
        assert_eq!(
            DeviceIdentity::hardware_only(hardware).compact_qualifier(),
            None
        );
    }
}
