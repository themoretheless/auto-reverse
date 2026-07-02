//! The single place that knows how scroll data is laid out inside a raw
//! macOS CGEvent: which fields to read to build a normalized
//! [`ScrollEvent`], and which fields to write to apply a
//! [`TransformDecision`] back. No policy lives here - that's `crate::scroll`.

use core_graphics::event::{CGEvent, EventField};

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
/// Only the plain integer DeltaAxis1/2 fields are ever written. Empirically
/// (verified against a live CGEvent, not assumed) writing DeltaAxis1/2
/// makes macOS recompute FixedPtDeltaAxis1/2 and PointDeltaAxis1/2 from the
/// new value automatically. Writing those derived fields ourselves
/// afterward would apply the change a second time and silently restore the
/// original, un-reversed direction for any pixel-precise consumer - so they
/// must be left untouched. See the regression test below.
pub fn apply_config_in_place(
    event: &CGEvent,
    config: &AppConfig,
    device_kind: DeviceKind,
    hardware: Option<HardwareId>,
) -> TransformDecision {
    let decision =
        scroll::transform_event(config, event_from_cg_event(event, device_kind, hardware));

    if decision.original.delta_vertical != decision.transformed.delta_vertical {
        event.set_integer_value_field(
            EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_1,
            decision.transformed.delta_vertical,
        );
    }

    if decision.original.delta_horizontal != decision.transformed.delta_horizontal {
        event.set_integer_value_field(
            EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_2,
            decision.transformed.delta_horizontal,
        );
    }

    decision
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
}
