//! Pure latency budgets and repeated-stall assessment.
//!
//! Platform adapters provide latency readings. This module deliberately
//! requires several readings before warning so one isolated maximum cannot
//! be presented as a persistent callback or scheduler problem.

use std::collections::VecDeque;
use std::error::Error;
use std::fmt;

pub const HISTORY_CAPACITY: usize = 5;
pub const MIN_READINGS_FOR_ASSESSMENT: usize = 3;
pub const BREACHES_FOR_WARNING: usize = 2;

/// Engineering budgets, not thresholds claimed by the cited HCI papers.
/// The 8 ms tail bound leaves roughly half a 60 Hz frame for the rest of the
/// input-to-display path.
pub const CALLBACK_BUDGET: StageBudget = StageBudget {
    average_us: 1_000.0,
    tail_us: 8_000.0,
};

pub const SCHEDULER_BUDGET: StageBudget = StageBudget {
    average_us: 2_000.0,
    tail_us: 8_000.0,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StageBudget {
    pub average_us: f64,
    pub tail_us: f64,
}

impl StageBudget {
    pub fn validate(self) -> Result<Self, LatencyBudgetError> {
        if !self.average_us.is_finite()
            || !self.tail_us.is_finite()
            || self.average_us <= 0.0
            || self.tail_us < self.average_us
        {
            return Err(LatencyBudgetError::InvalidBudget {
                average_us: self.average_us,
                tail_us: self.tail_us,
            });
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LatencyReading {
    average_us: f64,
    maximum_us: f64,
}

impl LatencyReading {
    pub fn new(average_us: f64, maximum_us: f64) -> Result<Self, LatencyBudgetError> {
        if !average_us.is_finite()
            || !maximum_us.is_finite()
            || average_us < 0.0
            || maximum_us < average_us
        {
            return Err(LatencyBudgetError::InvalidSample {
                average_us,
                maximum_us,
            });
        }
        Ok(Self {
            average_us,
            maximum_us,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LatencyStatus {
    Collecting,
    WithinBudget,
    RepeatedTailStalls,
    SustainedLatency,
}

impl LatencyStatus {
    pub fn is_warning(self) -> bool {
        matches!(self, Self::RepeatedTailStalls | Self::SustainedLatency)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LatencyAssessment {
    pub status: LatencyStatus,
    pub reading_count: usize,
    pub average_breaches: usize,
    pub tail_breaches: usize,
    pub worst_maximum_us: f64,
}

#[derive(Debug, Clone)]
pub struct LatencyHistory {
    budget: StageBudget,
    readings: VecDeque<LatencyReading>,
}

impl LatencyHistory {
    pub fn new(budget: StageBudget) -> Result<Self, LatencyBudgetError> {
        Ok(Self {
            budget: budget.validate()?,
            readings: VecDeque::with_capacity(HISTORY_CAPACITY),
        })
    }

    pub fn push(&mut self, reading: LatencyReading) {
        if self.readings.len() == HISTORY_CAPACITY {
            self.readings.pop_front();
        }
        self.readings.push_back(reading);
    }

    pub fn assessment(&self) -> LatencyAssessment {
        let average_breaches = self
            .readings
            .iter()
            .filter(|sample| sample.average_us > self.budget.average_us)
            .count();
        let tail_breaches = self
            .readings
            .iter()
            .filter(|sample| sample.maximum_us > self.budget.tail_us)
            .count();
        let worst_maximum_us = self
            .readings
            .iter()
            .map(|sample| sample.maximum_us)
            .fold(0.0_f64, f64::max);

        let status = if self.readings.len() < MIN_READINGS_FOR_ASSESSMENT {
            LatencyStatus::Collecting
        } else if average_breaches >= BREACHES_FOR_WARNING {
            LatencyStatus::SustainedLatency
        } else if tail_breaches >= BREACHES_FOR_WARNING {
            LatencyStatus::RepeatedTailStalls
        } else {
            LatencyStatus::WithinBudget
        };

        LatencyAssessment {
            status,
            reading_count: self.readings.len(),
            average_breaches,
            tail_breaches,
            worst_maximum_us,
        }
    }

    pub fn budget(&self) -> StageBudget {
        self.budget
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LatencyBudgetError {
    InvalidBudget { average_us: f64, tail_us: f64 },
    InvalidSample { average_us: f64, maximum_us: f64 },
}

impl fmt::Display for LatencyBudgetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBudget {
                average_us,
                tail_us,
            } => write!(
                f,
                "latency budget must be finite, positive, and ordered; average={average_us}, tail={tail_us}"
            ),
            Self::InvalidSample {
                average_us,
                maximum_us,
            } => write!(
                f,
                "latency sample must be finite, non-negative, and ordered; average={average_us}, maximum={maximum_us}"
            ),
        }
    }
}

impl Error for LatencyBudgetError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn history() -> LatencyHistory {
        LatencyHistory::new(CALLBACK_BUDGET).unwrap()
    }

    fn reading(average_us: f64, maximum_us: f64) -> LatencyReading {
        LatencyReading::new(average_us, maximum_us).unwrap()
    }

    #[test]
    fn one_outlier_never_becomes_a_warning() {
        let mut history = history();
        history.push(reading(200.0, 12_000.0));
        history.push(reading(220.0, 900.0));
        history.push(reading(240.0, 1_100.0));

        let assessment = history.assessment();
        assert_eq!(assessment.status, LatencyStatus::WithinBudget);
        assert_eq!(assessment.tail_breaches, 1);
        assert!(!assessment.status.is_warning());
    }

    #[test]
    fn repeated_tail_stalls_are_distinct_from_sustained_average_latency() {
        let mut tail_history = history();
        tail_history.push(reading(200.0, 9_000.0));
        tail_history.push(reading(250.0, 10_000.0));
        tail_history.push(reading(300.0, 700.0));
        assert_eq!(
            tail_history.assessment().status,
            LatencyStatus::RepeatedTailStalls
        );

        let mut average_history = history();
        average_history.push(reading(1_200.0, 2_000.0));
        average_history.push(reading(1_300.0, 2_100.0));
        average_history.push(reading(300.0, 600.0));
        assert_eq!(
            average_history.assessment().status,
            LatencyStatus::SustainedLatency
        );
    }

    #[test]
    fn assessment_waits_for_three_readings_and_keeps_only_five() {
        let mut history = history();
        history.push(reading(2_000.0, 3_000.0));
        history.push(reading(2_000.0, 3_000.0));
        assert_eq!(history.assessment().status, LatencyStatus::Collecting);

        for _ in 0..5 {
            history.push(reading(100.0, 200.0));
        }
        let assessment = history.assessment();
        assert_eq!(assessment.reading_count, HISTORY_CAPACITY);
        assert_eq!(assessment.average_breaches, 0);
        assert_eq!(assessment.status, LatencyStatus::WithinBudget);
    }

    #[test]
    fn invalid_budget_and_samples_are_rejected() {
        assert!(LatencyHistory::new(CALLBACK_BUDGET).is_ok());
        assert_eq!(
            LatencyHistory::new(SCHEDULER_BUDGET).unwrap().budget(),
            SCHEDULER_BUDGET
        );
        assert!(
            StageBudget {
                average_us: 5.0,
                tail_us: 4.0,
            }
            .validate()
            .is_err()
        );
        assert!(LatencyReading::new(f64::NAN, 4.0).is_err());
        assert!(LatencyReading::new(5.0, 4.0).is_err());
    }
}
