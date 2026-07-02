use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeviceKind {
    Mouse,
    Trackpad,
    MagicMouse,
    Unknown,
}

/// Identity of a specific physical HID device, as reported by IOKit.
/// Vendor and product IDs are the stable pair a user can target with a
/// per-device config rule ("this exact Logitech, not mice in general").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HardwareId {
    pub vendor_id: u32,
    pub product_id: u32,
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
}
