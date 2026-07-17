//! Fail-closed release/runtime gate for the non-live dynamics experiment.

use std::ffi::OsStr;

use crate::scroll_dynamics::SmoothPreset;

pub const DYNAMICS_KILL_SWITCH_ENV: &str = "AUTO_REVERSE_DISABLE_DYNAMICS";
pub const DYNAMICS_ENABLED_BY_DEFAULT: bool = false;
pub const MIN_PHYSICAL_CLASSES: u8 = 6;
pub const MIN_COMPLETED_SESSIONS_PER_CLASS: u32 = 30;
pub const MAX_P95_MOVEMENT_REGRESSION_BPS: i32 = 500;
pub const MAX_SCHEDULER_TAIL_US: u64 = 8_000;
pub const MAX_FAIL_OPEN_VIOLATIONS: u32 = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DynamicsAcceptanceEvidence {
    pub physical_classes: u8,
    pub min_completed_sessions_per_class: u32,
    /// Basis points versus exact pass-through; negative means an improvement.
    pub p95_movement_regression_bps: i32,
    pub worst_scheduler_tail_us: u64,
    pub fail_open_violations: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DynamicsGateReason {
    PresetOff,
    RuntimeKillSwitch,
    ReleaseDefaultOff,
    MissingAcceptanceEvidence,
    PhysicalClassCoverage,
    SessionCoverage,
    MovementRegression,
    SchedulerTail,
    FailOpenViolation,
    Accepted,
}

impl DynamicsGateReason {
    pub const fn code(self) -> &'static str {
        match self {
            Self::PresetOff => "preset_off",
            Self::RuntimeKillSwitch => "runtime_kill_switch",
            Self::ReleaseDefaultOff => "release_default_off",
            Self::MissingAcceptanceEvidence => "missing_acceptance_evidence",
            Self::PhysicalClassCoverage => "physical_class_coverage",
            Self::SessionCoverage => "session_coverage",
            Self::MovementRegression => "movement_regression",
            Self::SchedulerTail => "scheduler_tail",
            Self::FailOpenViolation => "fail_open_violation",
            Self::Accepted => "accepted",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DynamicsGateDecision {
    pub requested: SmoothPreset,
    pub effective: SmoothPreset,
    pub reason: DynamicsGateReason,
    pub config_rollback_required: bool,
}

pub fn evaluate_dynamics_gate(
    requested: SmoothPreset,
    kill_switch_engaged: bool,
    enabled_by_default: bool,
    evidence: Option<DynamicsAcceptanceEvidence>,
) -> DynamicsGateDecision {
    let reason = if requested == SmoothPreset::Off {
        DynamicsGateReason::PresetOff
    } else if kill_switch_engaged {
        DynamicsGateReason::RuntimeKillSwitch
    } else if !enabled_by_default {
        DynamicsGateReason::ReleaseDefaultOff
    } else {
        match evidence {
            None => DynamicsGateReason::MissingAcceptanceEvidence,
            Some(evidence) if evidence.physical_classes < MIN_PHYSICAL_CLASSES => {
                DynamicsGateReason::PhysicalClassCoverage
            }
            Some(evidence)
                if evidence.min_completed_sessions_per_class < MIN_COMPLETED_SESSIONS_PER_CLASS =>
            {
                DynamicsGateReason::SessionCoverage
            }
            Some(evidence)
                if evidence.p95_movement_regression_bps > MAX_P95_MOVEMENT_REGRESSION_BPS =>
            {
                DynamicsGateReason::MovementRegression
            }
            Some(evidence) if evidence.worst_scheduler_tail_us > MAX_SCHEDULER_TAIL_US => {
                DynamicsGateReason::SchedulerTail
            }
            Some(evidence) if evidence.fail_open_violations > MAX_FAIL_OPEN_VIOLATIONS => {
                DynamicsGateReason::FailOpenViolation
            }
            Some(_) => DynamicsGateReason::Accepted,
        }
    };
    let accepted = matches!(
        reason,
        DynamicsGateReason::Accepted | DynamicsGateReason::PresetOff
    );
    DynamicsGateDecision {
        requested,
        effective: if accepted {
            requested
        } else {
            SmoothPreset::Off
        },
        reason,
        config_rollback_required: requested != SmoothPreset::Off && !accepted,
    }
}

/// Current build decision. No release evidence is embedded while dynamics is
/// non-live, so changing only the default constant cannot accidentally enable
/// it: the missing-evidence branch still resolves to exact Off.
pub fn runtime_dynamics_decision(requested: SmoothPreset) -> DynamicsGateDecision {
    evaluate_dynamics_gate(
        requested,
        runtime_kill_switch_engaged(),
        DYNAMICS_ENABLED_BY_DEFAULT,
        None,
    )
}

pub fn runtime_kill_switch_engaged() -> bool {
    kill_switch_from_value(std::env::var_os(DYNAMICS_KILL_SWITCH_ENV).as_deref())
}

pub fn kill_switch_from_value(value: Option<&OsStr>) -> bool {
    let Some(value) = value else {
        return false;
    };
    match value.to_string_lossy().trim().to_ascii_lowercase().as_str() {
        "0" | "false" | "off" | "no" => false,
        // Empty, malformed, or explicitly true values fail closed.
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accepted_evidence() -> DynamicsAcceptanceEvidence {
        DynamicsAcceptanceEvidence {
            physical_classes: MIN_PHYSICAL_CLASSES,
            min_completed_sessions_per_class: MIN_COMPLETED_SESSIONS_PER_CLASS,
            p95_movement_regression_bps: MAX_P95_MOVEMENT_REGRESSION_BPS,
            worst_scheduler_tail_us: MAX_SCHEDULER_TAIL_US,
            fail_open_violations: MAX_FAIL_OPEN_VIOLATIONS,
        }
    }

    #[test]
    fn current_build_is_fail_closed_even_for_a_saved_preset() {
        let decision = evaluate_dynamics_gate(
            SmoothPreset::Balanced,
            false,
            DYNAMICS_ENABLED_BY_DEFAULT,
            None,
        );
        assert_eq!(decision.effective, SmoothPreset::Off);
        assert_eq!(decision.reason, DynamicsGateReason::ReleaseDefaultOff);
        assert!(decision.config_rollback_required);
    }

    #[test]
    fn kill_switch_wins_over_complete_acceptance_evidence() {
        let decision =
            evaluate_dynamics_gate(SmoothPreset::Fast, true, true, Some(accepted_evidence()));
        assert_eq!(decision.effective, SmoothPreset::Off);
        assert_eq!(decision.reason, DynamicsGateReason::RuntimeKillSwitch);
    }

    #[test]
    fn every_release_threshold_is_fail_closed() {
        let mut cases = Vec::new();
        cases.push((None, DynamicsGateReason::MissingAcceptanceEvidence));

        let mut evidence = accepted_evidence();
        evidence.physical_classes -= 1;
        cases.push((Some(evidence), DynamicsGateReason::PhysicalClassCoverage));

        let mut evidence = accepted_evidence();
        evidence.min_completed_sessions_per_class -= 1;
        cases.push((Some(evidence), DynamicsGateReason::SessionCoverage));

        let mut evidence = accepted_evidence();
        evidence.p95_movement_regression_bps += 1;
        cases.push((Some(evidence), DynamicsGateReason::MovementRegression));

        let mut evidence = accepted_evidence();
        evidence.worst_scheduler_tail_us += 1;
        cases.push((Some(evidence), DynamicsGateReason::SchedulerTail));

        let mut evidence = accepted_evidence();
        evidence.fail_open_violations = MAX_FAIL_OPEN_VIOLATIONS + 1;
        cases.push((Some(evidence), DynamicsGateReason::FailOpenViolation));

        for (evidence, expected) in cases {
            let decision = evaluate_dynamics_gate(SmoothPreset::Precise, false, true, evidence);
            assert_eq!(decision.reason, expected);
            assert_eq!(decision.effective, SmoothPreset::Off);
        }
    }

    #[test]
    fn exact_thresholds_allow_an_explicitly_accepted_release() {
        let decision = evaluate_dynamics_gate(
            SmoothPreset::Precise,
            false,
            true,
            Some(accepted_evidence()),
        );
        assert_eq!(decision.reason, DynamicsGateReason::Accepted);
        assert_eq!(decision.effective, SmoothPreset::Precise);
        assert!(!decision.config_rollback_required);
    }

    #[test]
    fn malformed_kill_switch_values_fail_closed() {
        assert!(!kill_switch_from_value(None));
        for disabled in ["0", "false", "OFF", " no "] {
            assert!(!kill_switch_from_value(Some(OsStr::new(disabled))));
        }
        for engaged in ["", "1", "true", "yes", "unexpected"] {
            assert!(kill_switch_from_value(Some(OsStr::new(engaged))));
        }
    }
}
