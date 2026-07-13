//! Pure confidence and timeout policy for heuristic HID-to-CGEvent attribution.

use std::time::Duration;

pub const HIGH_CONFIDENCE_MAX_AGE: Duration = Duration::from_millis(8);
pub const ATTRIBUTION_TIMEOUT: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AttributionStatus {
    #[default]
    NotApplicable,
    MissingObservation,
    HighConfidence,
    MediumConfidence,
    TimedOut,
}

impl AttributionStatus {
    pub const fn accepts_identity(self) -> bool {
        matches!(self, Self::HighConfidence | Self::MediumConfidence)
    }

    pub const fn code(self) -> &'static str {
        match self {
            Self::NotApplicable => "not_applicable",
            Self::MissingObservation => "missing_observation",
            Self::HighConfidence => "high",
            Self::MediumConfidence => "medium",
            Self::TimedOut => "timed_out",
        }
    }
}

pub fn assess_wheel_attribution(
    continuous: bool,
    source_pid: i64,
    observation_age: Option<Duration>,
) -> AttributionStatus {
    if continuous || source_pid != 0 {
        return AttributionStatus::NotApplicable;
    }
    match observation_age {
        None => AttributionStatus::MissingObservation,
        Some(age) if age <= HIGH_CONFIDENCE_MAX_AGE => AttributionStatus::HighConfidence,
        Some(age) if age <= ATTRIBUTION_TIMEOUT => AttributionStatus::MediumConfidence,
        Some(_) => AttributionStatus::TimedOut,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_fresh_hardware_wheel_observations_are_accepted() {
        assert_eq!(
            assess_wheel_attribution(false, 0, Some(Duration::ZERO)),
            AttributionStatus::HighConfidence
        );
        assert_eq!(
            assess_wheel_attribution(false, 0, Some(HIGH_CONFIDENCE_MAX_AGE)),
            AttributionStatus::HighConfidence
        );
        assert_eq!(
            assess_wheel_attribution(
                false,
                0,
                Some(HIGH_CONFIDENCE_MAX_AGE + Duration::from_nanos(1)),
            ),
            AttributionStatus::MediumConfidence
        );
        assert_eq!(
            assess_wheel_attribution(false, 0, Some(ATTRIBUTION_TIMEOUT)),
            AttributionStatus::MediumConfidence
        );
        assert_eq!(
            assess_wheel_attribution(
                false,
                0,
                Some(ATTRIBUTION_TIMEOUT + Duration::from_nanos(1)),
            ),
            AttributionStatus::TimedOut
        );
    }

    #[test]
    fn continuous_and_injected_events_never_inherit_last_active_identity() {
        assert_eq!(
            assess_wheel_attribution(true, 0, Some(Duration::ZERO)),
            AttributionStatus::NotApplicable
        );
        assert_eq!(
            assess_wheel_attribution(false, 42, Some(Duration::ZERO)),
            AttributionStatus::NotApplicable
        );
    }

    #[test]
    fn absence_is_distinct_from_timeout_and_neither_is_accepted() {
        let missing = assess_wheel_attribution(false, 0, None);
        let timed_out = assess_wheel_attribution(
            false,
            0,
            Some(ATTRIBUTION_TIMEOUT + Duration::from_millis(1)),
        );

        assert_eq!(missing, AttributionStatus::MissingObservation);
        assert_eq!(timed_out, AttributionStatus::TimedOut);
        assert!(!missing.accepts_identity());
        assert!(!timed_out.accepts_identity());
    }
}
