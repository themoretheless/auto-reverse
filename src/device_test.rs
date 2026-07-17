//! Pure state machine for the Devices tab's local attribution check.
//!
//! Platform code supplies only already-vetted physical, discrete activities.
//! The state machine requires full `DeviceIdentity` equality, so two identical
//! mouse models cannot satisfy each other's test when serial/location data is
//! available.

use crate::device::DeviceIdentity;

pub const DEVICE_TEST_TIMEOUT_US: u64 = 5_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceActivity<'a> {
    pub identity: &'a DeviceIdentity,
    pub timestamp_us: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceTestSession {
    started_us: u64,
    detected_us: Option<u64>,
}

impl DeviceTestSession {
    pub fn start(now_us: u64) -> Self {
        Self {
            started_us: now_us,
            detected_us: None,
        }
    }

    pub fn observe(
        &mut self,
        target: &DeviceIdentity,
        now_us: u64,
        activities: &[DeviceActivity<'_>],
    ) -> DeviceTestStatus {
        if self.detected_us.is_none() {
            self.detected_us = activities
                .iter()
                .filter(|activity| {
                    activity.identity == target
                        && activity.timestamp_us >= self.started_us
                        && activity.timestamp_us <= now_us
                })
                .map(|activity| activity.timestamp_us)
                .max();
        }

        if let Some(detected_us) = self.detected_us {
            return DeviceTestStatus::Detected {
                age_us: now_us.saturating_sub(detected_us),
            };
        }

        let elapsed_us = now_us.saturating_sub(self.started_us);
        if elapsed_us >= DEVICE_TEST_TIMEOUT_US {
            DeviceTestStatus::TimedOut
        } else {
            DeviceTestStatus::Listening {
                remaining_us: DEVICE_TEST_TIMEOUT_US - elapsed_us,
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceTestStatus {
    Listening { remaining_us: u64 },
    Detected { age_us: u64 },
    TimedOut,
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::device::HardwareId;

    use super::*;

    fn identity(serial: &str) -> DeviceIdentity {
        DeviceIdentity::new(
            HardwareId {
                vendor_id: 1,
                product_id: 2,
            },
            Some(Arc::from(serial)),
            None,
        )
    }

    #[test]
    fn identical_models_do_not_satisfy_each_others_exact_test() {
        let first = identity("first");
        let second = identity("second");
        let mut session = DeviceTestSession::start(100);
        let activities = [DeviceActivity {
            identity: &second,
            timestamp_us: 200,
        }];

        assert_eq!(
            session.observe(&first, 300, &activities),
            DeviceTestStatus::Listening {
                remaining_us: DEVICE_TEST_TIMEOUT_US - 200
            }
        );
    }

    #[test]
    fn activity_before_start_or_after_now_is_ignored() {
        let target = identity("one");
        let mut session = DeviceTestSession::start(100);
        let activities = [
            DeviceActivity {
                identity: &target,
                timestamp_us: 99,
            },
            DeviceActivity {
                identity: &target,
                timestamp_us: 301,
            },
        ];

        assert!(matches!(
            session.observe(&target, 300, &activities),
            DeviceTestStatus::Listening { .. }
        ));
    }

    #[test]
    fn exact_activity_completes_and_stays_completed() {
        let target = identity("one");
        let mut session = DeviceTestSession::start(100);
        let activities = [DeviceActivity {
            identity: &target,
            timestamp_us: 250,
        }];

        assert_eq!(
            session.observe(&target, 300, &activities),
            DeviceTestStatus::Detected { age_us: 50 }
        );
        assert_eq!(
            session.observe(&target, 900, &[]),
            DeviceTestStatus::Detected { age_us: 650 }
        );
    }

    #[test]
    fn no_activity_times_out_at_the_bounded_deadline() {
        let target = identity("one");
        let mut session = DeviceTestSession::start(100);

        assert_eq!(
            session.observe(&target, 100 + DEVICE_TEST_TIMEOUT_US, &[]),
            DeviceTestStatus::TimedOut
        );
    }
}
