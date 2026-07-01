use core_graphics::event::{CGEvent, CGEventField, EventField};

use crate::config::AppConfig;
use crate::device::DeviceKind;
use crate::input::ScrollEvent;

// Only the plain integer Delta fields are negated directly. Empirically
// (verified against a live CGEvent, not assumed) writing DeltaAxis1/2 makes
// macOS recompute FixedPtDeltaAxis1/2 and PointDeltaAxis1/2 from the new
// value automatically. Negating those derived fields ourselves afterward
// would negate them a second time and silently restore the original,
// un-reversed direction for any pixel-precise consumer - so they must be
// left untouched.
const DELTA_FIELDS: [CGEventField; 2] = [
    EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_1,
    EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_2,
];

/// True for a discrete-notch wheel (a physical mouse). False for continuous
/// scrolling, which covers trackpads and, indistinguishably, Apple's Magic
/// Mouse - see the limitations note in recommendation.md.
pub fn is_physical_mouse_wheel(event: &CGEvent) -> bool {
    event.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_IS_CONTINUOUS) == 0
}

/// Negates the scroll delta on both axes in place. macOS derives the
/// fixed-point and pixel delta fields from these automatically, so only
/// these two need to be written. Uses `saturating_neg` because `-i64::MIN`
/// overflows; an event actually reporting that delta is physically
/// impossible, but a panic in an OS-level event callback is worse than a
/// clamped value.
pub fn reverse_in_place(event: &CGEvent) {
    for field in DELTA_FIELDS {
        let value = event.get_integer_value_field(field);
        event.set_integer_value_field(field, value.saturating_neg());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransformDecision {
    pub original: ScrollEvent,
    pub transformed: ScrollEvent,
    pub reversed: bool,
    pub step_size_applied: bool,
}

impl TransformDecision {
    pub fn changed(self) -> bool {
        self.original.delta_vertical != self.transformed.delta_vertical
            || self.original.delta_horizontal != self.transformed.delta_horizontal
    }
}

pub fn transform_event(config: &AppConfig, event: ScrollEvent) -> TransformDecision {
    let mut transformed = event;
    let mut reversed = false;
    let mut step_size_applied = false;

    let skip_as_injected = config.reverse_only_raw_input && event.source_pid != 0;

    if !config.enabled || event.synthetic || skip_as_injected {
        return TransformDecision {
            original: event,
            transformed,
            reversed,
            step_size_applied,
        };
    }

    let should_reverse = config.should_reverse_device(event.device_kind);

    if !event.continuous
        && config.discrete_scroll_step_size > 0
        && event.delta_vertical.unsigned_abs() == 1
    {
        transformed.delta_vertical *= config.discrete_scroll_step_size;
        step_size_applied = true;
    }

    if should_reverse && config.reverse_vertical {
        transformed.delta_vertical = transformed.delta_vertical.saturating_neg();
        reversed = true;
    }

    if should_reverse && config.reverse_horizontal {
        transformed.delta_horizontal = transformed.delta_horizontal.saturating_neg();
        reversed = true;
    }

    TransformDecision {
        original: event,
        transformed,
        reversed,
        step_size_applied,
    }
}

pub fn event_from_cg_event(event: &CGEvent, device_kind: DeviceKind) -> ScrollEvent {
    ScrollEvent {
        device_kind,
        delta_vertical: event.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_1),
        delta_horizontal: event
            .get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_2),
        continuous: !is_physical_mouse_wheel(event),
        synthetic: false,
        source_pid: event.get_integer_value_field(EventField::EVENT_SOURCE_UNIX_PROCESS_ID),
    }
}

pub fn apply_config_in_place(
    event: &CGEvent,
    config: &AppConfig,
    device_kind: DeviceKind,
) -> TransformDecision {
    let decision = transform_event(config, event_from_cg_event(event, device_kind));

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
    use crate::config::AppConfig;
    use crate::device::DeviceKind;
    use crate::input::ScrollEvent;

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
    fn reverse_in_place_negates_delta_on_both_axes() {
        let event = new_test_event();
        event.set_integer_value_field(EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_1, 3);
        event.set_integer_value_field(EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_2, -5);

        reverse_in_place(&event);

        assert_eq!(
            event.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_1),
            -3
        );
        assert_eq!(
            event.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_2),
            5
        );
    }

    #[test]
    fn reversing_delta_also_flips_the_derived_pixel_and_fixed_point_fields() {
        // Regression test for the double-negation bug: macOS derives
        // FixedPtDelta and PointDelta from Delta the moment Delta is
        // written, so reverse_in_place must NOT touch them separately.
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

        reverse_in_place(&event);

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
    fn default_config_reverses_discrete_mouse_vertical_scroll_with_step_size() {
        let event = ScrollEvent::new(DeviceKind::Mouse, 1, 0, false);

        let decision = transform_event(&AppConfig::default(), event);

        assert_eq!(decision.transformed.delta_vertical, -3);
        assert!(decision.reversed);
        assert!(decision.step_size_applied);
    }

    #[test]
    fn default_config_leaves_trackpad_scroll_untouched() {
        let event = ScrollEvent::new(DeviceKind::Trackpad, 4, 0, true);

        let decision = transform_event(&AppConfig::default(), event);

        assert_eq!(decision.transformed, event);
        assert!(!decision.changed());
    }

    #[test]
    fn horizontal_scroll_is_opt_in() {
        let event = ScrollEvent::new(DeviceKind::Mouse, 0, 7, false);
        let default_decision = transform_event(&AppConfig::default(), event);

        let config = AppConfig {
            reverse_horizontal: true,
            ..AppConfig::default()
        };
        let horizontal_decision = transform_event(&config, event);

        assert_eq!(default_decision.transformed.delta_horizontal, 7);
        assert_eq!(horizontal_decision.transformed.delta_horizontal, -7);
    }

    #[test]
    fn disabled_config_passes_scroll_through() {
        let config = AppConfig {
            enabled: false,
            ..AppConfig::default()
        };
        let event = ScrollEvent::new(DeviceKind::Mouse, 1, 2, false);

        let decision = transform_event(&config, event);

        assert_eq!(decision.transformed, event);
        assert!(!decision.changed());
    }

    #[test]
    fn reverse_only_raw_input_skips_events_injected_by_another_process() {
        let config = AppConfig {
            reverse_only_raw_input: true,
            ..AppConfig::default()
        };
        let injected = ScrollEvent {
            source_pid: 4242,
            ..ScrollEvent::new(DeviceKind::Mouse, 1, 0, false)
        };

        let decision = transform_event(&config, injected);

        assert_eq!(decision.transformed, injected);
        assert!(!decision.changed());
    }

    #[test]
    fn reverse_only_raw_input_still_reverses_genuine_hardware_events() {
        let config = AppConfig {
            reverse_only_raw_input: true,
            ..AppConfig::default()
        };
        let genuine = ScrollEvent::new(DeviceKind::Mouse, 1, 0, false);

        let decision = transform_event(&config, genuine);

        assert!(decision.reversed);
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
        let decision = apply_config_in_place(&event, &config, DeviceKind::Mouse);

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
