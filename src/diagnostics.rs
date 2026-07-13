//! Pure diagnostic vocabulary shared by the live macOS Debug Console and
//! platform-independent trace/replay tooling.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use crate::device::DeviceKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    Vertical,
    Horizontal,
}

impl Axis {
    pub fn code(self) -> &'static str {
        match self {
            Self::Vertical => "vertical",
            Self::Horizontal => "horizontal",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Vertical => "Vertical",
            Self::Horizontal => "Horizontal",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionCategory {
    Reversed,
    Passed,
    Ignored,
}

impl DecisionCategory {
    pub fn code(self) -> &'static str {
        match self {
            Self::Reversed => "reversed",
            Self::Passed => "passed",
            Self::Ignored => "ignored",
        }
    }
}

/// Stable reason for one axis decision. Serde names intentionally match
/// `code()` so trace files and detailed CSV diagnostics use one vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionReason {
    ScrollReversalOff,
    TemporarilyPaused,
    SyntheticEvent,
    RawInputGuard,
    Reversed,
    DeviceRuleReversed,
    UnknownDeviceNotReversed,
    DeviceRuleDisabled,
    TrackpadNatural,
    DeviceReversalOff,
    AxisDisabled,
}

impl DecisionReason {
    pub fn code(self) -> &'static str {
        match self {
            Self::ScrollReversalOff => "scroll_reversal_off",
            Self::TemporarilyPaused => "temporarily_paused",
            Self::SyntheticEvent => "synthetic_event",
            Self::RawInputGuard => "raw_input_guard",
            Self::Reversed => "reversed",
            Self::DeviceRuleReversed => "device_rule_reversed",
            Self::UnknownDeviceNotReversed => "unknown_device_not_reversed",
            Self::DeviceRuleDisabled => "device_rule_disabled",
            Self::TrackpadNatural => "trackpad_natural",
            Self::DeviceReversalOff => "device_reversal_off",
            Self::AxisDisabled => "axis_disabled",
        }
    }

    pub fn category(self) -> DecisionCategory {
        match self {
            Self::Reversed | Self::DeviceRuleReversed => DecisionCategory::Reversed,
            Self::TrackpadNatural | Self::AxisDisabled => DecisionCategory::Passed,
            Self::ScrollReversalOff
            | Self::TemporarilyPaused
            | Self::SyntheticEvent
            | Self::RawInputGuard
            | Self::UnknownDeviceNotReversed
            | Self::DeviceRuleDisabled
            | Self::DeviceReversalOff => DecisionCategory::Ignored,
        }
    }

    pub fn display_text(self, device_kind: DeviceKind) -> Cow<'static, str> {
        match self {
            Self::ScrollReversalOff => Cow::Borrowed("Ignored – scroll reversal is off"),
            Self::TemporarilyPaused => Cow::Borrowed("Ignored - temporarily paused"),
            Self::SyntheticEvent => Cow::Borrowed("Ignored – synthetic event"),
            Self::RawInputGuard => Cow::Borrowed("Ignored – raw input guard (remote desktop)"),
            Self::Reversed => Cow::Borrowed("Reversed"),
            Self::DeviceRuleReversed => Cow::Borrowed("Reversed – device rule"),
            Self::UnknownDeviceNotReversed => {
                Cow::Borrowed("Ignored – unknown devices not reversed")
            }
            Self::DeviceRuleDisabled => {
                Cow::Borrowed("Ignored – this device has a Don't reverse rule")
            }
            Self::TrackpadNatural => Cow::Borrowed("Passed through – trackpad natural"),
            Self::DeviceReversalOff => Cow::Owned(format!(
                "Ignored – {} reversal is off",
                reversal_kind_label(device_kind)
            )),
            Self::AxisDisabled => Cow::Borrowed("Passed through"),
        }
    }
}

fn reversal_kind_label(device_kind: DeviceKind) -> &'static str {
    match device_kind {
        DeviceKind::Mouse => "mouse",
        DeviceKind::Trackpad => "trackpad",
        DeviceKind::MagicMouse => "Magic Mouse",
        DeviceKind::Unknown => "unknown device",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_names_match_stable_codes() {
        #[derive(Serialize)]
        struct Wrapper {
            reason: DecisionReason,
        }

        for reason in [
            DecisionReason::ScrollReversalOff,
            DecisionReason::TemporarilyPaused,
            DecisionReason::SyntheticEvent,
            DecisionReason::RawInputGuard,
            DecisionReason::Reversed,
            DecisionReason::DeviceRuleReversed,
            DecisionReason::UnknownDeviceNotReversed,
            DecisionReason::DeviceRuleDisabled,
            DecisionReason::TrackpadNatural,
            DecisionReason::DeviceReversalOff,
            DecisionReason::AxisDisabled,
        ] {
            let serialized = toml::to_string(&Wrapper { reason }).unwrap();
            assert_eq!(serialized.trim(), format!("reason = \"{}\"", reason.code()));
        }
    }
}
