//! On-demand interval latency snapshots for this process's active scroll tap.
//!
//! `CGGetEventTapList` resets each listed tap's minimum and maximum latency to
//! its average as a side effect. Apple defines the min/max interval but not the
//! average accumulation window. Callers must request snapshots explicitly and
//! avoid presenting the average as an interval average; this adapter never
//! polls.

use std::error::Error;
use std::fmt;
use std::mem::MaybeUninit;
use std::ptr;

use objc2_core_graphics::{
    CGError, CGEventTapInformation, CGEventTapOptions, CGEventType, CGGetEventTapList,
};

const MAX_EVENT_TAPS: u32 = 4_096;
const GROWTH_HEADROOM: u32 = 16;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TapLatency {
    pub event_tap_id: u32,
    pub enabled: bool,
    pub minimum_us: f32,
    pub average_us: f32,
    pub maximum_us: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TapLatencySnapshot {
    pub listed_tap_count: usize,
    pub possibly_truncated: bool,
    pub active_scroll_taps: Vec<TapLatency>,
}

pub fn current_process_scroll_snapshot() -> Result<TapLatencySnapshot, TapMetricsError> {
    snapshot_for_pid(
        i32::try_from(std::process::id())
            .map_err(|_| TapMetricsError::InvalidProcessId(std::process::id()))?,
    )
}

fn snapshot_for_pid(pid: i32) -> Result<TapLatencySnapshot, TapMetricsError> {
    let mut initial_count = 0_u32;
    check_cg_error(unsafe { CGGetEventTapList(0, ptr::null_mut(), &mut initial_count) })?;
    if initial_count > MAX_EVENT_TAPS {
        return Err(TapMetricsError::TooManyTaps(initial_count));
    }
    if initial_count == 0 {
        return Ok(TapLatencySnapshot {
            listed_tap_count: 0,
            possibly_truncated: false,
            active_scroll_taps: Vec::new(),
        });
    }

    let capacity = initial_count
        .saturating_add(GROWTH_HEADROOM)
        .min(MAX_EVENT_TAPS);
    let mut storage = Vec::<MaybeUninit<CGEventTapInformation>>::with_capacity(capacity as usize);
    let mut filled = 0_u32;
    check_cg_error(unsafe {
        CGGetEventTapList(
            capacity,
            storage.as_mut_ptr().cast::<CGEventTapInformation>(),
            &mut filled,
        )
    })?;
    if filled > capacity {
        return Err(TapMetricsError::InvalidFilledCount { filled, capacity });
    }

    unsafe { storage.set_len(filled as usize) };
    let listed = storage
        .into_iter()
        .map(|entry| unsafe { entry.assume_init() })
        .collect::<Vec<_>>();
    let active_scroll_taps = listed
        .iter()
        .filter(|tap| is_active_scroll_tap(tap, pid))
        .map(tap_latency)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(TapLatencySnapshot {
        listed_tap_count: listed.len(),
        // Headroom normally makes `filled < capacity`. Equality means the
        // list may have grown between the count and fill calls.
        possibly_truncated: filled == capacity,
        active_scroll_taps,
    })
}

fn is_active_scroll_tap(tap: &CGEventTapInformation, pid: i32) -> bool {
    let scroll_mask = 1_u64 << CGEventType::ScrollWheel.0;
    tap.tappingProcess == pid
        && tap.options.0 & CGEventTapOptions::ListenOnly.0 == 0
        && tap.eventsOfInterest & scroll_mask != 0
}

fn tap_latency(tap: &CGEventTapInformation) -> Result<TapLatency, TapMetricsError> {
    let values = [tap.minUsecLatency, tap.avgUsecLatency, tap.maxUsecLatency];
    if values
        .iter()
        .any(|value| !value.is_finite() || *value < 0.0)
        || tap.minUsecLatency > tap.avgUsecLatency
        || tap.avgUsecLatency > tap.maxUsecLatency
    {
        return Err(TapMetricsError::InvalidLatency {
            event_tap_id: tap.eventTapID,
            minimum_us: tap.minUsecLatency,
            average_us: tap.avgUsecLatency,
            maximum_us: tap.maxUsecLatency,
        });
    }

    Ok(TapLatency {
        event_tap_id: tap.eventTapID,
        enabled: tap.enabled,
        minimum_us: tap.minUsecLatency,
        average_us: tap.avgUsecLatency,
        maximum_us: tap.maxUsecLatency,
    })
}

fn check_cg_error(error: CGError) -> Result<(), TapMetricsError> {
    if error == CGError::Success {
        Ok(())
    } else {
        Err(TapMetricsError::CoreGraphics(error.0))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TapMetricsError {
    CoreGraphics(i32),
    InvalidProcessId(u32),
    TooManyTaps(u32),
    InvalidFilledCount {
        filled: u32,
        capacity: u32,
    },
    InvalidLatency {
        event_tap_id: u32,
        minimum_us: f32,
        average_us: f32,
        maximum_us: f32,
    },
}

impl fmt::Display for TapMetricsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CoreGraphics(code) => {
                write!(f, "CGGetEventTapList failed with CGError {code}")
            }
            Self::InvalidProcessId(pid) => write!(f, "process ID {pid} does not fit pid_t"),
            Self::TooManyTaps(count) => write!(
                f,
                "CoreGraphics reported {count} event taps; the safety limit is {MAX_EVENT_TAPS}"
            ),
            Self::InvalidFilledCount { filled, capacity } => write!(
                f,
                "CoreGraphics filled {filled} event taps into capacity {capacity}"
            ),
            Self::InvalidLatency {
                event_tap_id,
                minimum_us,
                average_us,
                maximum_us,
            } => write!(
                f,
                "event tap {event_tap_id} returned invalid latency values min={minimum_us}, avg={average_us}, max={maximum_us}"
            ),
        }
    }
}

impl Error for TapMetricsError {}

#[cfg(test)]
mod tests {
    use objc2_core_graphics::{CGEventTapLocation, CGEventTapOptions};

    use super::*;

    fn info(pid: i32, options: CGEventTapOptions, mask: u64) -> CGEventTapInformation {
        CGEventTapInformation {
            eventTapID: 7,
            tapPoint: CGEventTapLocation::HIDEventTap,
            options,
            eventsOfInterest: mask,
            tappingProcess: pid,
            processBeingTapped: 0,
            enabled: true,
            minUsecLatency: 2.0,
            avgUsecLatency: 4.0,
            maxUsecLatency: 9.0,
        }
    }

    #[test]
    fn selector_requires_same_process_active_scroll_filter() {
        let scroll_mask = 1_u64 << CGEventType::ScrollWheel.0;
        assert!(is_active_scroll_tap(
            &info(42, CGEventTapOptions::Default, scroll_mask),
            42
        ));
        assert!(!is_active_scroll_tap(
            &info(42, CGEventTapOptions::ListenOnly, scroll_mask),
            42
        ));
        assert!(!is_active_scroll_tap(
            &info(41, CGEventTapOptions::Default, scroll_mask),
            42
        ));
        assert!(!is_active_scroll_tap(
            &info(42, CGEventTapOptions::Default, 1),
            42
        ));
    }

    #[test]
    fn invalid_latency_is_rejected_before_presentation() {
        let mut tap = info(42, CGEventTapOptions::Default, 1);
        tap.maxUsecLatency = f32::NAN;
        assert!(matches!(
            tap_latency(&tap),
            Err(TapMetricsError::InvalidLatency { .. })
        ));

        let mut tap = info(42, CGEventTapOptions::Default, 1);
        tap.avgUsecLatency = 10.0;
        tap.maxUsecLatency = 9.0;
        assert!(matches!(
            tap_latency(&tap),
            Err(TapMetricsError::InvalidLatency { .. })
        ));
    }
}
