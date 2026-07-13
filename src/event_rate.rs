//! Event-arrival rate diagnostics derived from observed timestamps.
//!
//! These values describe the stream Auto Reverse actually received. They are
//! deliberately not called device polling rates: macOS may coalesce, split,
//! or schedule input independently of a device's advertised hardware rate.

use crate::device::DeviceKind;
use crate::statistics::{Distribution, distribution};

pub const MILLIHERTZ_PER_HERTZ: u64 = 1_000;
pub const DEFAULT_ACTIVE_GAP_US: u64 = 150_000;
const MICROSECONDS_PER_SECOND_TIMES_MILLIHERTZ: u64 = 1_000_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventRateSample {
    pub timestamp_us: u64,
    pub device_kind: DeviceKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EventRateHistogram {
    pub below_30_hz: usize,
    pub from_30_to_60_hz: usize,
    pub from_60_to_120_hz: usize,
    pub from_120_to_240_hz: usize,
    pub at_least_240_hz: usize,
}

impl EventRateHistogram {
    pub fn total(self) -> usize {
        self.below_30_hz
            + self.from_30_to_60_hz
            + self.from_60_to_120_hz
            + self.from_120_to_240_hz
            + self.at_least_240_hz
    }

    fn observe(&mut self, rate_millihz: u64) {
        match rate_millihz {
            0..30_000 => self.below_30_hz += 1,
            30_000..60_000 => self.from_30_to_60_hz += 1,
            60_000..120_000 => self.from_60_to_120_hz += 1,
            120_000..240_000 => self.from_120_to_240_hz += 1,
            _ => self.at_least_240_hz += 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceEventRate {
    pub device_kind: DeviceKind,
    pub timestamp_count: usize,
    pub rates_millihz: Distribution,
    pub histogram: EventRateHistogram,
}

pub fn analyze_event_rates(samples: &[EventRateSample]) -> Vec<DeviceEventRate> {
    analyze_event_rates_with_gap(samples, DEFAULT_ACTIVE_GAP_US)
}

pub fn analyze_event_rates_with_gap(
    samples: &[EventRateSample],
    maximum_active_interval_us: u64,
) -> Vec<DeviceEventRate> {
    const DEVICE_KINDS: [DeviceKind; 4] = [
        DeviceKind::Mouse,
        DeviceKind::Trackpad,
        DeviceKind::MagicMouse,
        DeviceKind::Unknown,
    ];

    DEVICE_KINDS
        .into_iter()
        .filter_map(|device_kind| analyze_device(samples, device_kind, maximum_active_interval_us))
        .collect()
}

fn analyze_device(
    samples: &[EventRateSample],
    device_kind: DeviceKind,
    maximum_active_interval_us: u64,
) -> Option<DeviceEventRate> {
    let mut timestamps = samples
        .iter()
        .filter(|sample| sample.device_kind == device_kind)
        .map(|sample| sample.timestamp_us)
        .collect::<Vec<_>>();
    timestamps.sort_unstable();
    timestamps.dedup();

    let rates = timestamps
        .windows(2)
        .filter_map(|pair| {
            let interval_us = pair[1].saturating_sub(pair[0]);
            (interval_us > 0 && interval_us <= maximum_active_interval_us)
                .then(|| MICROSECONDS_PER_SECOND_TIMES_MILLIHERTZ / interval_us)
        })
        .collect::<Vec<_>>();
    let rates_millihz = distribution(rates.clone())?;
    let mut histogram = EventRateHistogram::default();
    for rate in rates {
        histogram.observe(rate);
    }

    Some(DeviceEventRate {
        device_kind,
        timestamp_count: timestamps.len(),
        rates_millihz,
        histogram,
    })
}

pub fn millihertz_to_hertz(rate: u64) -> f64 {
    rate as f64 / MILLIHERTZ_PER_HERTZ as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(timestamp_us: u64, device_kind: DeviceKind) -> EventRateSample {
        EventRateSample {
            timestamp_us,
            device_kind,
        }
    }

    #[test]
    fn duplicate_axis_timestamps_count_as_one_arrival() {
        let report = analyze_event_rates(&[
            sample(0, DeviceKind::Mouse),
            sample(0, DeviceKind::Mouse),
            sample(10_000, DeviceKind::Mouse),
            sample(20_000, DeviceKind::Mouse),
        ]);

        assert_eq!(report.len(), 1);
        assert_eq!(report[0].timestamp_count, 3);
        assert_eq!(report[0].rates_millihz.p50, 100_000);
        assert_eq!(report[0].histogram.from_60_to_120_hz, 2);
        assert_eq!(report[0].histogram.total(), 2);
    }

    #[test]
    fn reports_each_observed_device_kind_separately() {
        let report = analyze_event_rates(&[
            sample(0, DeviceKind::Trackpad),
            sample(5_000, DeviceKind::Trackpad),
            sample(0, DeviceKind::MagicMouse),
            sample(20_000, DeviceKind::MagicMouse),
        ]);

        assert_eq!(report.len(), 2);
        assert_eq!(report[0].device_kind, DeviceKind::Trackpad);
        assert_eq!(report[0].rates_millihz.max, 200_000);
        assert_eq!(report[1].device_kind, DeviceKind::MagicMouse);
        assert_eq!(report[1].rates_millihz.max, 50_000);
    }

    #[test]
    fn one_timestamp_has_no_rate_claim() {
        assert!(analyze_event_rates(&[sample(0, DeviceKind::Mouse)]).is_empty());
    }

    #[test]
    fn idle_gaps_do_not_pretend_to_be_a_low_device_event_rate() {
        let report = analyze_event_rates(&[
            sample(0, DeviceKind::Mouse),
            sample(10_000, DeviceKind::Mouse),
            sample(2_000_000, DeviceKind::Mouse),
            sample(2_010_000, DeviceKind::Mouse),
        ]);

        assert_eq!(report[0].histogram.total(), 2);
        assert_eq!(report[0].rates_millihz.min, 100_000);
    }
}
