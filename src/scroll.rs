//! Pure scroll-reversal policy. This module has no CoreGraphics or other
//! platform dependency on purpose: given a normalized [`ScrollEvent`] and an
//! [`AppConfig`], it decides what the deltas should become. Reading real
//! CGEvents and writing the decision back lives in
//! `platform::macos::scroll_events`.

use crate::config::AppConfig;
use crate::input::ScrollEvent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransformDecision {
    pub original: ScrollEvent,
    pub transformed: ScrollEvent,
    pub reversed: bool,
    /// Whether policy says to reverse this specific axis, independent of
    /// whether the numeric delta actually changed value. A continuous
    /// (trackpad) event's real per-touch motion lives in fields this pure
    /// module never sees (CoreGraphics-specific PointDelta/FixedPtDelta);
    /// `ScrollEvent.delta_vertical/horizontal` only carries the coarse,
    /// often-zero line-quantized approximation for such events. Comparing
    /// original vs transformed on that coarse value would silently skip
    /// reversing plenty of real, nonzero-pixel continuous events whose
    /// coarse delta happens to read 0 - exactly the kind of per-event
    /// inconsistency that feels like stutter across a single swipe. The
    /// platform layer must gate its writes on these flags, not on
    /// `changed()`.
    pub vertical_reversed: bool,
    pub horizontal_reversed: bool,
    pub step_size_applied: bool,
}

impl TransformDecision {
    pub fn passthrough(event: ScrollEvent) -> Self {
        Self {
            original: event.clone(),
            transformed: event,
            reversed: false,
            vertical_reversed: false,
            horizontal_reversed: false,
            step_size_applied: false,
        }
    }

    pub fn changed(&self) -> bool {
        self.original.delta_vertical != self.transformed.delta_vertical
            || self.original.delta_horizontal != self.transformed.delta_horizontal
    }
}

pub fn transform_event(config: &AppConfig, event: ScrollEvent) -> TransformDecision {
    let skip_as_injected = config.reverse_only_raw_input && event.source_pid != 0;

    if !config.enabled
        || event.synthetic
        || event.hid_source.requires_passthrough()
        || skip_as_injected
    {
        return TransformDecision::passthrough(event);
    }

    let mut transformed = event.clone();
    let mut reversed = false;
    let mut vertical_reversed = false;
    let mut horizontal_reversed = false;
    let mut step_size_applied = false;
    let profile = config.resolve_device_profile(event.device_kind, event.identity.as_deref());
    let should_reverse = profile.reverse.value;

    // Gated on should_reverse too: step size is an accompaniment to
    // reversal for this device, not an independent global multiplier - a
    // device with its own reversal toggle off should be left untouched,
    // not just left un-reversed but still amplified. saturating_mul instead
    // of `*=` as defense in depth, even though the unsigned_abs() == 1
    // guard means overflow can't actually happen today.
    if should_reverse && !event.continuous && profile.step_size.value > 0 {
        if event.delta_vertical.unsigned_abs() == 1 {
            transformed.delta_vertical = transformed
                .delta_vertical
                .saturating_mul(profile.step_size.value);
            step_size_applied = true;
        }
        if event.delta_horizontal.unsigned_abs() == 1 {
            transformed.delta_horizontal = transformed
                .delta_horizontal
                .saturating_mul(profile.step_size.value);
            step_size_applied = true;
        }
    }

    if should_reverse && config.reverse_vertical {
        transformed.delta_vertical = transformed.delta_vertical.saturating_neg();
        reversed = true;
        vertical_reversed = true;
    }

    if should_reverse && config.reverse_horizontal {
        transformed.delta_horizontal = transformed.delta_horizontal.saturating_neg();
        reversed = true;
        horizontal_reversed = true;
    }

    TransformDecision {
        original: event,
        transformed,
        reversed,
        vertical_reversed,
        horizontal_reversed,
        step_size_applied,
    }
}

#[cfg(test)]
mod tests {
    use crate::device::DeviceKind;
    use crate::device_source::HidSourceClass;

    use super::*;

    #[test]
    fn default_config_reverses_discrete_mouse_vertical_scroll_with_step_size() {
        let event = ScrollEvent::new(DeviceKind::Mouse, 1, 0, false);

        let decision = transform_event(&AppConfig::default(), event);

        assert_eq!(decision.transformed.delta_vertical, -3);
        assert!(decision.reversed);
        assert!(decision.step_size_applied);
    }

    #[test]
    fn zero_step_size_disables_discrete_wheel_adjustment() {
        let config = AppConfig {
            discrete_scroll_step_size: 0,
            ..AppConfig::default()
        };
        let event = ScrollEvent::new(DeviceKind::Mouse, 1, 0, false);

        let decision = transform_event(&config, event);

        assert_eq!(decision.transformed.delta_vertical, -1);
        assert!(!decision.step_size_applied);
    }

    #[test]
    fn default_config_leaves_trackpad_scroll_untouched() {
        let event = ScrollEvent::new(DeviceKind::Trackpad, 4, 0, true);

        let decision = transform_event(&AppConfig::default(), event.clone());

        assert_eq!(decision.transformed, event);
        assert!(!decision.changed());
    }

    #[test]
    fn continuous_event_is_marked_reversed_even_when_the_coarse_delta_is_zero() {
        // Regression test for the trackpad stutter bug: a real touch event's
        // coarse, line-quantized delta is frequently 0 even when the real
        // (CoreGraphics-only) pixel motion is nonzero - `changed()` would
        // miss these, silently leaving that specific event un-reversed
        // while neighboring events in the same swipe do get reversed. The
        // platform layer must gate its writes on vertical_reversed /
        // horizontal_reversed, which reflect policy only, not this event's
        // particular coarse magnitude.
        let event = ScrollEvent::new(DeviceKind::Trackpad, 0, 0, true);
        let config = AppConfig {
            reverse_trackpad: true,
            ..AppConfig::default()
        };

        let decision = transform_event(&config, event);

        assert!(decision.vertical_reversed);
        // Negating a coarse 0 is still 0, so the pure ScrollEvent itself
        // looks unchanged - this is exactly why the platform layer cannot
        // use `changed()` to decide whether to touch the real pixel fields.
        assert!(!decision.changed());
    }

    #[test]
    fn horizontal_scroll_is_opt_in() {
        let event = ScrollEvent::new(DeviceKind::Mouse, 0, 7, false);
        let default_decision = transform_event(&AppConfig::default(), event.clone());

        let config = AppConfig {
            reverse_horizontal: true,
            ..AppConfig::default()
        };
        let horizontal_decision = transform_event(&config, event);

        assert_eq!(default_decision.transformed.delta_horizontal, 7);
        assert_eq!(horizontal_decision.transformed.delta_horizontal, -7);
    }

    #[test]
    fn step_size_does_not_apply_when_the_devices_own_reversal_is_off() {
        let config = AppConfig {
            reverse_mouse: false,
            discrete_scroll_step_size: 3,
            ..AppConfig::default()
        };
        let event = ScrollEvent::new(DeviceKind::Mouse, 1, 0, false);

        let decision = transform_event(&config, event.clone());

        assert_eq!(decision.transformed, event);
        assert!(!decision.step_size_applied);
        assert!(!decision.changed());
    }

    #[test]
    fn disabled_config_passes_scroll_through() {
        let config = AppConfig {
            enabled: false,
            ..AppConfig::default()
        };
        let event = ScrollEvent::new(DeviceKind::Mouse, 1, 2, false);

        let decision = transform_event(&config, event.clone());

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

        let decision = transform_event(&config, injected.clone());

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
    fn observed_virtual_and_unknown_hid_sources_fail_open() {
        for hid_source in [HidSourceClass::Virtual, HidSourceClass::Unknown] {
            let event = ScrollEvent {
                hid_source,
                ..ScrollEvent::new(DeviceKind::Mouse, 1, 0, false)
            };

            let decision = transform_event(&AppConfig::default(), event.clone());

            assert_eq!(decision.transformed, event);
            assert!(!decision.changed());
        }
    }

    #[test]
    fn observed_physical_hid_source_still_uses_normal_policy() {
        let event = ScrollEvent {
            hid_source: HidSourceClass::Physical,
            ..ScrollEvent::new(DeviceKind::Mouse, 1, 0, false)
        };

        let decision = transform_event(&AppConfig::default(), event);

        assert_eq!(decision.transformed.delta_vertical, -3);
        assert!(decision.reversed);
    }

    #[test]
    fn device_rule_pins_a_specific_mouse_off_while_other_mice_still_reverse() {
        use crate::config::DeviceRule;
        use std::sync::Arc;

        use crate::device::{DeviceIdentity, HardwareId};

        let config = AppConfig {
            device_rules: vec![DeviceRule::for_hardware(
                HardwareId {
                    vendor_id: 0x046d,
                    product_id: 0xc52b,
                },
                None,
                false,
            )],
            ..AppConfig::default()
        };
        let ruled_mouse = ScrollEvent {
            identity: Some(Arc::new(DeviceIdentity::hardware_only(HardwareId {
                vendor_id: 0x046d,
                product_id: 0xc52b,
            }))),
            ..ScrollEvent::new(DeviceKind::Mouse, 1, 0, false)
        };
        let other_mouse = ScrollEvent {
            identity: Some(Arc::new(DeviceIdentity::hardware_only(HardwareId {
                vendor_id: 0x1532,
                product_id: 0x0067,
            }))),
            ..ScrollEvent::new(DeviceKind::Mouse, 1, 0, false)
        };

        let ruled = transform_event(&config, ruled_mouse);
        let other = transform_event(&config, other_mouse);

        assert!(!ruled.changed());
        assert!(other.reversed);
    }

    #[test]
    fn device_rule_step_size_overrides_global_wheel_step() {
        use std::sync::Arc;

        use crate::config::DeviceRule;
        use crate::device::{DeviceIdentity, HardwareId};

        let hardware = HardwareId {
            vendor_id: 0x046d,
            product_id: 0xc52b,
        };
        let config = AppConfig {
            discrete_scroll_step_size: 3,
            device_rules: vec![DeviceRule {
                step_size: Some(7),
                ..DeviceRule::for_hardware(hardware, None, true)
            }],
            ..AppConfig::default()
        };
        let event = ScrollEvent {
            identity: Some(Arc::new(DeviceIdentity::hardware_only(hardware))),
            ..ScrollEvent::new(DeviceKind::Mouse, 1, 0, false)
        };

        let decision = transform_event(&config, event);

        assert_eq!(decision.transformed.delta_vertical, -7);
        assert!(decision.step_size_applied);
    }

    #[test]
    fn pure_transform_honors_magic_mouse_config() {
        let config = AppConfig {
            reverse_magic_mouse: false,
            ..AppConfig::default()
        };
        let event = ScrollEvent::new(DeviceKind::MagicMouse, 1, 0, true);

        let decision = transform_event(&config, event.clone());

        assert_eq!(decision.transformed, event);
        assert!(!decision.changed());
    }
}
