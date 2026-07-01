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
