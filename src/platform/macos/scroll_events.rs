//! The single place that knows how scroll data is laid out inside a raw
//! macOS CGEvent: which fields to read to build a normalized
//! [`ScrollEvent`], and which fields to write to apply a
//! [`TransformDecision`] back. No policy lives here - that's `crate::scroll`.

use core_graphics::event::{CGEvent, CGEventField, EventField};

use crate::config::AppConfig;
use crate::device::{DeviceKind, HardwareId};
use crate::input::ScrollEvent;
use crate::scroll::{self, TransformDecision};

/// True for a discrete-notch wheel (a physical mouse). False for continuous
/// scrolling, which covers trackpads and, indistinguishably, Apple's Magic
/// Mouse - see the limitations note in recommendation.md.
pub fn is_physical_mouse_wheel(event: &CGEvent) -> bool {
    event.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_IS_CONTINUOUS) == 0
}

/// The posting process id (`kCGEventSourceUnixProcessID`). 0 for a genuine
/// hardware event observed at the HID tap; nonzero when another process
/// injected the event.
pub fn event_source_pid(event: &CGEvent) -> i64 {
    event.get_integer_value_field(EventField::EVENT_SOURCE_UNIX_PROCESS_ID)
}

pub fn event_from_cg_event(
    event: &CGEvent,
    device_kind: DeviceKind,
    hardware: Option<HardwareId>,
) -> ScrollEvent {
    ScrollEvent {
        device_kind,
        delta_vertical: event.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_1),
        delta_horizontal: event
            .get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_2),
        continuous: !is_physical_mouse_wheel(event),
        synthetic: false,
        source_pid: event.get_integer_value_field(EventField::EVENT_SOURCE_UNIX_PROCESS_ID),
        hardware,
    }
}

/// Runs the pure policy over the event and writes any changed deltas back.
///
/// Discrete (physical wheel) events: only the plain integer DeltaAxis1/2
/// fields are ever written. Empirically (verified against a live CGEvent,
/// not assumed) writing DeltaAxis1/2 makes macOS recompute
/// FixedPtDeltaAxis1/2 and PointDeltaAxis1/2 from the new value
/// automatically. Writing those derived fields ourselves afterward would
/// apply the change a second time and silently restore the original,
/// un-reversed direction for any pixel-precise consumer - so they must be
/// left untouched. See `writing_delta_also_flips_the_derived_pixel_and_fixed_point_fields`.
///
/// Continuous (trackpad, and indistinguishably Magic Mouse) events: the
/// opposite rule applies. DeltaAxis1/2 is a coarse, line-quantized value
/// here that is frequently 0 for perfectly real, nonzero per-touch pixel
/// motion (most individual continuous-scroll events do not cross a full
/// "line"), while the true precision consuming apps render lives in
/// PointDeltaAxis1/2 and FixedPtDeltaAxis1/2. Two consequences follow.
///
/// First, writing DeltaAxis1/2 on a continuous event makes macOS overwrite
/// those two with a fixed-size jump derived from the coarse value,
/// discarding the real motion - see
/// `writing_delta_on_a_continuous_event_destroys_pixel_precision` - so
/// continuous events negate PointDelta/FixedPtDelta directly instead,
/// leaving DeltaAxis1/2 untouched.
///
/// Second, deciding whether to negate must not be based on whether that
/// coarse value changed - `decision`'s per-axis reversed flags reflect
/// policy only (config says to reverse this axis or not), not this
/// specific event's coarse magnitude. Gating on "did the coarse delta
/// change" would skip every continuous event whose coarse delta happens to
/// read 0 on both sides, silently leaving it un-reversed while neighboring
/// events in the same swipe do get reversed - exactly the per-event
/// inconsistency that feels like stutter.
pub fn apply_config_in_place(
    event: &CGEvent,
    config: &AppConfig,
    device_kind: DeviceKind,
    hardware: Option<HardwareId>,
) -> TransformDecision {
    let normalized = event_from_cg_event(event, device_kind, hardware);
    let decision = scroll::transform_event(config, normalized);

    if normalized.continuous {
        if decision.vertical_reversed {
            negate_precise_axis(
                event,
                EventField::SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_1,
                EventField::SCROLL_WHEEL_EVENT_FIXED_POINT_DELTA_AXIS_1,
            );
        }
        if decision.horizontal_reversed {
            negate_precise_axis(
                event,
                EventField::SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_2,
                EventField::SCROLL_WHEEL_EVENT_FIXED_POINT_DELTA_AXIS_2,
            );
        }
    } else {
        let vertical_changed =
            decision.original.delta_vertical != decision.transformed.delta_vertical;
        let horizontal_changed =
            decision.original.delta_horizontal != decision.transformed.delta_horizontal;

        if vertical_changed {
            event.set_integer_value_field(
                EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_1,
                decision.transformed.delta_vertical,
            );
        }
        if horizontal_changed {
            event.set_integer_value_field(
                EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_2,
                decision.transformed.delta_horizontal,
            );
        }
    }

    decision
}

fn negate_precise_axis(event: &CGEvent, point_field: CGEventField, fixed_field: CGEventField) {
    let point = event.get_integer_value_field(point_field);
    event.set_integer_value_field(point_field, point.saturating_neg());

    let fixed = event.get_double_value_field(fixed_field);
    event.set_double_value_field(fixed_field, -fixed);
}

#[cfg(test)]
mod tests {
    use core_graphics::event::ScrollEventUnit;
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    use super::*;

    // A plain `CGEvent::new` produces a typeless event with no backing
    // storage for scroll-wheel fields, so writes to them silently no-op.
    // Only an event actually created as a scroll-wheel event has that
    // storage, so tests build one via `new_scroll_event` and then overwrite
    // its fields, exactly as production code does to a real incoming event.
    fn new_test_event() -> CGEvent {
        let source = CGEventSource::new(CGEventSourceStateID::Private).unwrap();
        CGEvent::new_scroll_event(source, ScrollEventUnit::LINE, 1, 0, 0, 0).unwrap()
    }

    // Continuous (trackpad-style) events are built with the PIXEL unit, not
    // LINE, and flagged IsContinuous - this is what actually gives them
    // sub-line-precision Point/FixedPt values to reverse. wheel1 is the
    // vertical pixel delta.
    fn new_continuous_test_event(wheel1: i32) -> CGEvent {
        let source = CGEventSource::new(CGEventSourceStateID::Private).unwrap();
        let event =
            CGEvent::new_scroll_event(source, ScrollEventUnit::PIXEL, 1, wheel1, 0, 0).unwrap();
        event.set_integer_value_field(EventField::SCROLL_WHEEL_EVENT_IS_CONTINUOUS, 1);
        event
    }

    #[test]
    fn continuous_flag_marks_non_physical_wheel() {
        let event = new_test_event();
        event.set_integer_value_field(EventField::SCROLL_WHEEL_EVENT_IS_CONTINUOUS, 1);
        assert!(!is_physical_mouse_wheel(&event));

        event.set_integer_value_field(EventField::SCROLL_WHEEL_EVENT_IS_CONTINUOUS, 0);
        assert!(is_physical_mouse_wheel(&event));
    }

    #[test]
    fn writing_delta_also_flips_the_derived_pixel_and_fixed_point_fields() {
        // Regression test for the double-negation bug: macOS derives
        // FixedPtDelta and PointDelta from Delta the moment Delta is
        // written, so apply_config_in_place must ONLY write the Delta
        // fields and never touch the derived ones.
        let event = new_test_event();
        event.set_integer_value_field(EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_1, 3);
        let original_fixed_pt =
            event.get_double_value_field(EventField::SCROLL_WHEEL_EVENT_FIXED_POINT_DELTA_AXIS_1);
        let original_point =
            event.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_1);
        assert!(
            original_point != 0,
            "test assumption broken: macOS did not derive a pixel delta from Delta"
        );

        event.set_integer_value_field(EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_1, -3);

        assert_eq!(
            event.get_double_value_field(EventField::SCROLL_WHEEL_EVENT_FIXED_POINT_DELTA_AXIS_1),
            -original_fixed_pt
        );
        assert_eq!(
            event.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_1),
            -original_point
        );
    }

    #[test]
    fn apply_config_in_place_writes_expected_delta_fields() {
        let event = new_test_event();
        event.set_integer_value_field(EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_1, 1);
        event.set_integer_value_field(EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_2, 2);

        let config = AppConfig {
            reverse_horizontal: true,
            ..AppConfig::default()
        };
        let decision = apply_config_in_place(&event, &config, DeviceKind::Mouse, None);

        assert!(decision.changed());
        assert_eq!(
            event.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_1),
            -3
        );
        assert_eq!(
            event.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_2),
            -2
        );
    }

    #[test]
    fn writing_delta_on_a_continuous_event_destroys_pixel_precision() {
        // Regression test for the trackpad stutter bug: unlike a discrete
        // wheel event, a continuous event's real per-touch motion lives in
        // Point/FixedPtDelta, not the coarse line-based Delta. Writing
        // Delta here does NOT cascade proportionally - it replaces the
        // original pixel-precise value with a fixed-size jump, regardless
        // of how many pixels the touch actually moved.
        let event = new_continuous_test_event(3);
        let original_point =
            event.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_1);
        assert_eq!(
            original_point, 3,
            "test assumption: PIXEL unit sets Point directly"
        );

        let delta = event.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_1);
        event.set_integer_value_field(EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_1, -delta);

        let after_point =
            event.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_1);
        assert_ne!(
            after_point, -original_point,
            "writing Delta was expected to clobber Point with an unrelated fixed value, \
             not proportionally negate it - if this now fails, macOS's behavior changed \
             and apply_config_in_place's continuous-event branch should be revisited"
        );
    }

    #[test]
    fn apply_config_in_place_negates_precise_fields_for_continuous_events() {
        let event = new_continuous_test_event(5);
        let original_point =
            event.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_1);
        let original_fixed =
            event.get_double_value_field(EventField::SCROLL_WHEEL_EVENT_FIXED_POINT_DELTA_AXIS_1);
        let original_delta =
            event.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_1);

        let config = AppConfig {
            reverse_trackpad: true,
            ..AppConfig::default()
        };
        let decision = apply_config_in_place(&event, &config, DeviceKind::Trackpad, None);

        assert!(decision.reversed);
        assert_eq!(
            event.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_1),
            -original_point
        );
        assert_eq!(
            event.get_double_value_field(EventField::SCROLL_WHEEL_EVENT_FIXED_POINT_DELTA_AXIS_1),
            -original_fixed
        );
        // The coarse Delta field is deliberately left untouched for
        // continuous events - nothing reads it for pixel-precise scrolling,
        // and writing it would clobber the two assertions above.
        assert_eq!(
            event.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_1),
            original_delta
        );
    }

    #[test]
    fn apply_config_in_place_still_reverses_a_continuous_event_whose_coarse_delta_is_zero() {
        // Regression test for the trackpad stutter bug: build an event
        // exactly like a real touch tick that moved real pixels but didn't
        // cross a full "line" - Delta reads 0 while Point/FixedPt carry the
        // real, nonzero motion (set directly, which - unlike writing
        // Delta - does not cascade to the other fields, so this precisely
        // recreates the scenario without any construction side effect).
        let event = new_continuous_test_event(0);
        assert_eq!(
            event.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_1),
            0,
            "test assumption: wheel1=0 gives a zero coarse delta"
        );
        event.set_integer_value_field(EventField::SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_1, 5);
        event.set_double_value_field(EventField::SCROLL_WHEEL_EVENT_FIXED_POINT_DELTA_AXIS_1, 0.5);

        let config = AppConfig {
            reverse_trackpad: true,
            ..AppConfig::default()
        };
        let decision = apply_config_in_place(&event, &config, DeviceKind::Trackpad, None);

        assert!(
            decision.vertical_reversed,
            "policy says reverse trackpad scrolling regardless of this event's coarse delta"
        );
        assert_eq!(
            event.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_1),
            -5,
            "the real, nonzero pixel motion must still be reversed even though Delta read 0"
        );
        assert_eq!(
            event.get_double_value_field(EventField::SCROLL_WHEEL_EVENT_FIXED_POINT_DELTA_AXIS_1),
            -0.5
        );
    }
}
