//! Privacy-bounded diagnostics text suitable for the clipboard.
//!
//! The input vocabulary deliberately has no timestamps, deltas, device names,
//! hardware IDs, process IDs, serials, app names or window titles. Keeping the
//! boundary structural is stronger than collecting a rich event and trying to
//! redact strings after formatting it.

use std::fmt::Write as _;

use crate::config::AppConfig;
use crate::device::DeviceKind;
use crate::diagnostics::{DecisionCategory, DecisionReason};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeStatus {
    Idle,
    Disabled,
    Paused,
    WaitingForPermission,
    Starting,
    Running,
    AlreadyRunning,
    Stopped,
    Failed,
}

impl RuntimeStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Disabled => "disabled",
            Self::Paused => "temporarily paused",
            Self::WaitingForPermission => "waiting for Accessibility",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::AlreadyRunning => "another instance owns the event tap",
            Self::Stopped => "event tap stopped",
            Self::Failed => "event tap failed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SummaryEvent {
    pub device_kind: DeviceKind,
    pub reason: DecisionReason,
}

pub struct DiagnosticsSummaryInput<'a> {
    pub config: &'a AppConfig,
    pub runtime_status: RuntimeStatus,
    pub accessibility_granted: bool,
    pub events: &'a [SummaryEvent],
    pub event_capacity: usize,
}

pub fn build_diagnostics_summary(input: DiagnosticsSummaryInput<'_>) -> String {
    let mut text = String::with_capacity(768);
    writeln!(text, "Auto Reverse diagnostics summary").unwrap();
    writeln!(text, "Version: {}", env!("CARGO_PKG_VERSION")).unwrap();
    writeln!(text, "Runtime: {}", input.runtime_status.label()).unwrap();
    writeln!(
        text,
        "Accessibility: {}",
        if input.accessibility_granted {
            "granted"
        } else {
            "required"
        }
    )
    .unwrap();
    writeln!(
        text,
        "Configuration: {}",
        input.config.plain_english_summary()
    )
    .unwrap();
    writeln!(
        text,
        "Wheel step: {}; smooth preset: {}; posted-input guard: {}",
        input.config.discrete_scroll_step_size,
        input.config.smooth_preset.as_str(),
        on_off(input.config.reverse_only_raw_input),
    )
    .unwrap();
    writeln!(
        text,
        "Per-device rules: {}",
        input.config.device_rules.len()
    )
    .unwrap();
    writeln!(
        text,
        "Buffered decisions: {} / {}",
        input.events.len(),
        input.event_capacity
    )
    .unwrap();

    let outcome_counts = [
        count_category(input.events, DecisionCategory::Reversed),
        count_category(input.events, DecisionCategory::Passed),
        count_category(input.events, DecisionCategory::Ignored),
    ];
    writeln!(
        text,
        "Outcomes: reversed {}, passed {}, ignored {}",
        outcome_counts[0], outcome_counts[1], outcome_counts[2]
    )
    .unwrap();

    writeln!(
        text,
        "Device types: mouse {}, trackpad {}, Magic Mouse {}, unknown {}",
        count_kind(input.events, DeviceKind::Mouse),
        count_kind(input.events, DeviceKind::Trackpad),
        count_kind(input.events, DeviceKind::MagicMouse),
        count_kind(input.events, DeviceKind::Unknown),
    )
    .unwrap();

    let mut reasons = ALL_REASONS
        .iter()
        .copied()
        .filter_map(|reason| {
            let count = input
                .events
                .iter()
                .filter(|event| event.reason == reason)
                .count();
            (count > 0).then_some((reason, count))
        })
        .collect::<Vec<_>>();
    reasons.sort_by(|(left_reason, left_count), (right_reason, right_count)| {
        right_count
            .cmp(left_count)
            .then_with(|| left_reason.code().cmp(right_reason.code()))
    });
    if reasons.is_empty() {
        writeln!(text, "Decision reasons: none observed").unwrap();
    } else {
        let summary = reasons
            .iter()
            .map(|(reason, count)| format!("{} {count}", reason.code()))
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(text, "Decision reasons: {summary}").unwrap();
    }

    text.push_str(
        "Privacy: aggregate only; no raw deltas, timestamps, device identifiers, process IDs, app names, window titles, or event trace.\n",
    );
    text
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

fn count_category(events: &[SummaryEvent], category: DecisionCategory) -> usize {
    events
        .iter()
        .filter(|event| event.reason.category() == category)
        .count()
}

fn count_kind(events: &[SummaryEvent], kind: DeviceKind) -> usize {
    events
        .iter()
        .filter(|event| event.device_kind == kind)
        .count()
}

const ALL_REASONS: &[DecisionReason] = &[
    DecisionReason::ScrollReversalOff,
    DecisionReason::TemporarilyPaused,
    DecisionReason::SyntheticEvent,
    DecisionReason::VirtualHidSource,
    DecisionReason::UnknownHidSource,
    DecisionReason::RawInputGuard,
    DecisionReason::Reversed,
    DecisionReason::DeviceRuleReversed,
    DecisionReason::UnknownDeviceNotReversed,
    DecisionReason::DeviceRuleDisabled,
    DecisionReason::TrackpadNatural,
    DecisionReason::DeviceReversalOff,
    DecisionReason::AxisDisabled,
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::HardwareId;

    #[test]
    fn summary_is_deterministic_and_contains_only_aggregates() {
        let config = AppConfig {
            device_rules: vec![crate::config::DeviceRule::for_hardware(
                HardwareId {
                    vendor_id: 0x046d,
                    product_id: 0xb034,
                },
                Some("Private Mouse Name".to_string()),
                true,
            )],
            ..AppConfig::default()
        };
        let events = [
            SummaryEvent {
                device_kind: DeviceKind::Mouse,
                reason: DecisionReason::DeviceRuleReversed,
            },
            SummaryEvent {
                device_kind: DeviceKind::Trackpad,
                reason: DecisionReason::TrackpadNatural,
            },
        ];

        let summary = build_diagnostics_summary(DiagnosticsSummaryInput {
            config: &config,
            runtime_status: RuntimeStatus::Running,
            accessibility_granted: true,
            events: &events,
            event_capacity: 500,
        });

        assert!(summary.contains("Outcomes: reversed 1, passed 1, ignored 0"));
        assert!(summary.contains("Device types: mouse 1, trackpad 1"));
        assert!(summary.contains("Per-device rules: 1"));
        assert!(summary.contains("device_rule_reversed 1, trackpad_natural 1"));
        for private_value in [
            "Private Mouse Name",
            "046d",
            "b034",
            "source_pid",
            "raw_delta",
            "timestamp_ms",
        ] {
            assert!(!summary.contains(private_value));
        }
    }

    #[test]
    fn empty_summary_has_explicit_zero_counts() {
        let summary = build_diagnostics_summary(DiagnosticsSummaryInput {
            config: &AppConfig::default(),
            runtime_status: RuntimeStatus::WaitingForPermission,
            accessibility_granted: false,
            events: &[],
            event_capacity: 500,
        });

        assert!(summary.contains("Accessibility: required"));
        assert!(summary.contains("Buffered decisions: 0 / 500"));
        assert!(summary.contains("Decision reasons: none observed"));
    }
}
