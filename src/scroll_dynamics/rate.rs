//! Bounded input-time normalization and recent-rate estimation.

use super::DynamicsError;

pub const MIN_INPUT_DT_US: u64 = 1_000;
pub const MAX_INPUT_DT_US: u64 = 50_000;
pub const RATE_WINDOW_CAPACITY: usize = 8;
pub const MIN_RATE_INTERVALS: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeltaClamp {
    None,
    Minimum,
    Maximum,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NormalizedDelta {
    raw_us: u64,
    used_us: u64,
    clamp: DeltaClamp,
}

impl NormalizedDelta {
    pub fn raw_us(self) -> u64 {
        self.raw_us
    }

    pub fn used_us(self) -> u64 {
        self.used_us
    }

    pub fn clamp(self) -> DeltaClamp {
        self.clamp
    }
}

pub fn normalize_input_delta(
    previous_us: u64,
    current_us: u64,
) -> Result<NormalizedDelta, DynamicsError> {
    if current_us < previous_us {
        return Err(DynamicsError::TimestampOutOfOrder {
            previous: previous_us,
            current: current_us,
        });
    }

    let raw_us = current_us - previous_us;
    let (used_us, clamp) = if raw_us < MIN_INPUT_DT_US {
        (MIN_INPUT_DT_US, DeltaClamp::Minimum)
    } else if raw_us > MAX_INPUT_DT_US {
        (MAX_INPUT_DT_US, DeltaClamp::Maximum)
    } else {
        (raw_us, DeltaClamp::None)
    };
    Ok(NormalizedDelta {
        raw_us,
        used_us,
        clamp,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateEstimate {
    pub interval_count: usize,
    pub median_interval_us: u64,
    pub millihertz: u64,
}

#[derive(Debug, Clone)]
pub struct InputRateEstimator {
    intervals_us: [u64; RATE_WINDOW_CAPACITY],
    len: usize,
    next: usize,
}

impl InputRateEstimator {
    pub fn observe(&mut self, delta: NormalizedDelta) {
        if self.len < RATE_WINDOW_CAPACITY {
            self.intervals_us[self.len] = delta.used_us();
            self.len += 1;
            self.next = self.len % RATE_WINDOW_CAPACITY;
        } else {
            self.intervals_us[self.next] = delta.used_us();
            self.next = (self.next + 1) % RATE_WINDOW_CAPACITY;
        }
    }

    pub fn estimate(&self) -> Option<RateEstimate> {
        if self.len < MIN_RATE_INTERVALS {
            return None;
        }
        let mut sorted = self.intervals_us;
        sorted[..self.len].sort_unstable();
        let rank = (self.len * 50).div_ceil(100).max(1) - 1;
        let median_interval_us = sorted[rank];
        Some(RateEstimate {
            interval_count: self.len,
            median_interval_us,
            millihertz: 1_000_000_000_u64 / median_interval_us,
        })
    }

    pub fn interval_count(&self) -> usize {
        self.len
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

impl Default for InputRateEstimator {
    fn default() -> Self {
        Self {
            intervals_us: [0; RATE_WINDOW_CAPACITY],
            len: 0,
            next: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn delta(used_us: u64) -> NormalizedDelta {
        NormalizedDelta {
            raw_us: used_us,
            used_us,
            clamp: DeltaClamp::None,
        }
    }

    #[test]
    fn timestamp_delta_is_bounded_after_duplicate_and_long_stall() {
        assert_eq!(
            normalize_input_delta(10, 10).unwrap(),
            NormalizedDelta {
                raw_us: 0,
                used_us: MIN_INPUT_DT_US,
                clamp: DeltaClamp::Minimum,
            }
        );
        assert_eq!(
            normalize_input_delta(10, 1_000_010).unwrap(),
            NormalizedDelta {
                raw_us: 1_000_000,
                used_us: MAX_INPUT_DT_US,
                clamp: DeltaClamp::Maximum,
            }
        );
        assert!(matches!(
            normalize_input_delta(10, 9),
            Err(DynamicsError::TimestampOutOfOrder { .. })
        ));
    }

    #[test]
    fn rate_needs_three_intervals_and_uses_the_median() {
        let mut estimator = InputRateEstimator::default();
        estimator.observe(delta(10_000));
        estimator.observe(delta(50_000));
        assert_eq!(estimator.estimate(), None);
        estimator.observe(delta(11_000));

        assert_eq!(
            estimator.estimate(),
            Some(RateEstimate {
                interval_count: 3,
                median_interval_us: 11_000,
                millihertz: 90_909,
            })
        );
    }

    #[test]
    fn rate_window_is_fixed_and_evicts_old_observations() {
        let mut estimator = InputRateEstimator::default();
        for _ in 0..RATE_WINDOW_CAPACITY {
            estimator.observe(delta(50_000));
        }
        for _ in 0..RATE_WINDOW_CAPACITY {
            estimator.observe(delta(10_000));
        }

        let estimate = estimator.estimate().unwrap();
        assert_eq!(estimate.interval_count, RATE_WINDOW_CAPACITY);
        assert_eq!(estimate.median_interval_us, 10_000);
        assert_eq!(estimate.millihertz, 100_000);
    }
}
