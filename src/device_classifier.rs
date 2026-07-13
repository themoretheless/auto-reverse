//! Pure scroll-source classification.
//!
//! The platform layer supplies a connected-device inventory plus observations
//! (continuous scroll, momentum phase, and whether a recent gesture had at
//! least two touching fingers). Keeping the policy here makes the heuristic
//! deterministic and testable without a live event tap or OS framework types.

use std::time::{Duration, Instant};

use crate::device::DeviceKind;

pub const CLASSIFIER_DESCRIPTION: &str = "discrete wheel = mouse; an exclusive connected trackpad or Magic Mouse wins; when both are connected, recent two-finger gestures identify the trackpad";

const TRACKPAD_TOUCH_WINDOW: Duration = Duration::from_millis(222);
const SOURCE_RESET_WINDOW: Duration = Duration::from_millis(333);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MomentumPhase {
    None,
    Began,
    Continued,
    Ended,
    Unknown,
}

/// Public-hardware evidence available before classifying a continuous scroll.
///
/// IOHID cannot attribute an individual continuous event, but it can answer a
/// simpler and decisive question: whether only one supported continuous
/// source is currently connected. The timing heuristic is needed only for
/// [`Self::Both`]. Unknown inventory deliberately falls back to trackpad so a
/// failed optional probe never reverses a built-in trackpad unexpectedly.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ContinuousSourceHint {
    TrackpadOnly,
    MagicMouseOnly,
    Both,
    #[default]
    Unknown,
}

impl ContinuousSourceHint {
    pub const fn from_presence(trackpad: bool, magic_mouse: bool) -> Self {
        match (trackpad, magic_mouse) {
            (true, false) => Self::TrackpadOnly,
            (false, true) => Self::MagicMouseOnly,
            (true, true) => Self::Both,
            (false, false) => Self::Unknown,
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::TrackpadOnly => "trackpad only",
            Self::MagicMouseOnly => "Magic Mouse only",
            Self::Both => "trackpad and Magic Mouse",
            Self::Unknown => "no recognized continuous device",
        }
    }

    const fn exclusive_kind(self) -> Option<DeviceKind> {
        match self {
            Self::TrackpadOnly => Some(DeviceKind::Trackpad),
            Self::MagicMouseOnly => Some(DeviceKind::MagicMouse),
            Self::Both | Self::Unknown => None,
        }
    }
}

/// Stateful public-API heuristic for separating two continuous sources.
///
/// A two-finger AppKit gesture immediately preceding a scroll identifies a
/// trackpad. Momentum events inherit the last continuous source. A normal
/// continuous event after the touch observation has gone stale is treated as
/// Magic Mouse-like. This intentionally mirrors Scroll Reverser's proven
/// timing model while preserving a distinct `MagicMouse` domain value.
#[derive(Debug, Clone)]
pub struct GestureSourceClassifier {
    source_hint: ContinuousSourceHint,
    last_two_finger_touch: Option<Instant>,
    two_finger_touch_pending: bool,
    last_continuous_kind: DeviceKind,
}

impl Default for GestureSourceClassifier {
    fn default() -> Self {
        Self::new(ContinuousSourceHint::Unknown)
    }
}

impl GestureSourceClassifier {
    pub const fn new(source_hint: ContinuousSourceHint) -> Self {
        Self {
            source_hint,
            last_two_finger_touch: None,
            two_finger_touch_pending: false,
            last_continuous_kind: match source_hint {
                ContinuousSourceHint::MagicMouseOnly | ContinuousSourceHint::Both => {
                    DeviceKind::MagicMouse
                }
                ContinuousSourceHint::TrackpadOnly | ContinuousSourceHint::Unknown => {
                    DeviceKind::Trackpad
                }
            },
        }
    }

    pub fn observe_gesture(&mut self, touching_fingers: usize, now: Instant) {
        if touching_fingers >= 2 {
            self.last_two_finger_touch = Some(now);
            self.two_finger_touch_pending = true;
        }
    }

    /// Applies a hot-plug inventory change and discards observations that
    /// belonged to the previous set of devices.
    pub fn update_source_hint(&mut self, source_hint: ContinuousSourceHint) {
        if self.source_hint != source_hint {
            *self = Self::new(source_hint);
        }
    }

    pub fn classify_scroll(
        &mut self,
        continuous: bool,
        momentum_phase: MomentumPhase,
        now: Instant,
    ) -> DeviceKind {
        // Every scroll consumes the pending gesture observation. Otherwise a
        // mouse-wheel tick between a trackpad gesture and a later continuous
        // event could incorrectly lend that stale touch to the later device.
        let touch_pending = std::mem::take(&mut self.two_finger_touch_pending);
        if !continuous {
            return DeviceKind::Mouse;
        }

        if let Some(kind) = self.source_hint.exclusive_kind() {
            self.last_continuous_kind = kind;
            return kind;
        }
        if self.source_hint == ContinuousSourceHint::Unknown {
            self.last_continuous_kind = DeviceKind::Trackpad;
            return DeviceKind::Trackpad;
        }

        let touch_elapsed = self
            .last_two_finger_touch
            .map(|observed| now.saturating_duration_since(observed));

        let kind = if touch_pending
            && touch_elapsed.is_some_and(|elapsed| elapsed < TRACKPAD_TOUCH_WINDOW)
        {
            DeviceKind::Trackpad
        } else if momentum_phase == MomentumPhase::None
            && touch_elapsed.is_none_or(|elapsed| elapsed > SOURCE_RESET_WINDOW)
        {
            DeviceKind::MagicMouse
        } else {
            self.last_continuous_kind
        };

        self.last_continuous_kind = kind;
        kind
    }

    /// Safe classification when the passive gesture monitor is unavailable.
    /// Exclusive hardware evidence remains useful; ambiguous/unknown input
    /// stays natural by falling back to trackpad.
    pub const fn classify_without_gesture(&self, continuous: bool) -> DeviceKind {
        if !continuous {
            return DeviceKind::Mouse;
        }
        match self.source_hint {
            ContinuousSourceHint::MagicMouseOnly => DeviceKind::MagicMouse,
            ContinuousSourceHint::TrackpadOnly
            | ContinuousSourceHint::Both
            | ContinuousSourceHint::Unknown => DeviceKind::Trackpad,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn after(start: Instant, milliseconds: u64) -> Instant {
        start + Duration::from_millis(milliseconds)
    }

    fn both_sources() -> GestureSourceClassifier {
        GestureSourceClassifier::new(ContinuousSourceHint::Both)
    }

    #[test]
    fn source_hint_maps_connected_hardware_without_guessing() {
        assert_eq!(
            ContinuousSourceHint::from_presence(true, false),
            ContinuousSourceHint::TrackpadOnly
        );
        assert_eq!(
            ContinuousSourceHint::from_presence(false, true),
            ContinuousSourceHint::MagicMouseOnly
        );
        assert_eq!(
            ContinuousSourceHint::from_presence(true, true),
            ContinuousSourceHint::Both
        );
        assert_eq!(
            ContinuousSourceHint::from_presence(false, false),
            ContinuousSourceHint::Unknown
        );
    }

    #[test]
    fn exclusive_trackpad_inventory_overrides_missing_touch_observations() {
        let now = Instant::now();
        let mut classifier = GestureSourceClassifier::new(ContinuousSourceHint::TrackpadOnly);

        assert_eq!(
            classifier.classify_scroll(true, MomentumPhase::None, now),
            DeviceKind::Trackpad
        );
        assert_eq!(
            classifier.classify_scroll(true, MomentumPhase::Continued, now),
            DeviceKind::Trackpad
        );
    }

    #[test]
    fn exclusive_magic_mouse_inventory_needs_no_gesture_monitor() {
        let classifier = GestureSourceClassifier::new(ContinuousSourceHint::MagicMouseOnly);

        assert_eq!(
            classifier.classify_without_gesture(true),
            DeviceKind::MagicMouse
        );
        assert_eq!(
            classifier.classify_without_gesture(false),
            DeviceKind::Mouse
        );
    }

    #[test]
    fn hot_plug_replaces_stale_source_and_touch_state() {
        let now = Instant::now();
        let mut classifier = GestureSourceClassifier::new(ContinuousSourceHint::Both);
        classifier.observe_gesture(2, now);
        classifier.update_source_hint(ContinuousSourceHint::MagicMouseOnly);

        assert_eq!(
            classifier.classify_scroll(true, MomentumPhase::None, now),
            DeviceKind::MagicMouse
        );
        classifier.update_source_hint(ContinuousSourceHint::TrackpadOnly);
        assert_eq!(
            classifier.classify_scroll(true, MomentumPhase::None, now),
            DeviceKind::Trackpad
        );
    }

    #[test]
    fn unknown_or_ambiguous_inventory_falls_back_to_trackpad_without_gestures() {
        assert_eq!(
            GestureSourceClassifier::default().classify_without_gesture(true),
            DeviceKind::Trackpad
        );
        assert_eq!(
            both_sources().classify_without_gesture(true),
            DeviceKind::Trackpad
        );
    }

    #[test]
    fn discrete_scroll_is_always_a_mouse() {
        let now = Instant::now();
        let mut classifier = both_sources();
        classifier.observe_gesture(2, now);

        assert_eq!(
            classifier.classify_scroll(false, MomentumPhase::None, now),
            DeviceKind::Mouse
        );
    }

    #[test]
    fn discrete_scroll_consumes_pending_touch_observation() {
        let start = Instant::now();
        let mut classifier = both_sources();
        classifier.observe_gesture(2, start);
        assert_eq!(
            classifier.classify_scroll(false, MomentumPhase::None, after(start, 10)),
            DeviceKind::Mouse
        );

        assert_eq!(
            classifier.classify_scroll(true, MomentumPhase::None, after(start, 20)),
            DeviceKind::MagicMouse
        );
    }

    #[test]
    fn recent_two_finger_gesture_identifies_trackpad() {
        let start = Instant::now();
        let mut classifier = both_sources();
        classifier.observe_gesture(2, start);

        assert_eq!(
            classifier.classify_scroll(true, MomentumPhase::None, after(start, 100)),
            DeviceKind::Trackpad
        );
    }

    #[test]
    fn one_finger_gesture_is_ignored() {
        let now = Instant::now();
        let mut classifier = both_sources();
        classifier.observe_gesture(1, now);

        assert_eq!(
            classifier.classify_scroll(true, MomentumPhase::None, now),
            DeviceKind::MagicMouse
        );
    }

    #[test]
    fn continuous_scroll_without_touch_is_magic_mouse_like() {
        let now = Instant::now();
        let mut classifier = both_sources();

        assert_eq!(
            classifier.classify_scroll(true, MomentumPhase::None, now),
            DeviceKind::MagicMouse
        );
    }

    #[test]
    fn momentum_keeps_the_last_trackpad_source_after_touch_expires() {
        let start = Instant::now();
        let mut classifier = both_sources();
        classifier.observe_gesture(3, start);
        assert_eq!(
            classifier.classify_scroll(true, MomentumPhase::Began, after(start, 20)),
            DeviceKind::Trackpad
        );

        assert_eq!(
            classifier.classify_scroll(true, MomentumPhase::Continued, after(start, 500)),
            DeviceKind::Trackpad
        );
        assert_eq!(
            classifier.classify_scroll(true, MomentumPhase::Ended, after(start, 700)),
            DeviceKind::Trackpad
        );
    }

    #[test]
    fn normal_scroll_after_stale_touch_resets_to_magic_mouse() {
        let start = Instant::now();
        let mut classifier = both_sources();
        classifier.observe_gesture(2, start);
        assert_eq!(
            classifier.classify_scroll(true, MomentumPhase::None, after(start, 50)),
            DeviceKind::Trackpad
        );

        assert_eq!(
            classifier.classify_scroll(true, MomentumPhase::None, after(start, 334)),
            DeviceKind::MagicMouse
        );
    }

    #[test]
    fn two_finger_observation_is_consumed_then_recent_source_is_retained() {
        let start = Instant::now();
        let mut classifier = both_sources();
        classifier.observe_gesture(2, start);
        assert_eq!(
            classifier.classify_scroll(true, MomentumPhase::None, after(start, 10)),
            DeviceKind::Trackpad
        );

        assert_eq!(
            classifier.classify_scroll(true, MomentumPhase::None, after(start, 20)),
            DeviceKind::Trackpad
        );
    }

    #[test]
    fn timing_boundaries_match_the_documented_strict_windows() {
        let start = Instant::now();
        let mut at_touch_boundary = both_sources();
        at_touch_boundary.observe_gesture(2, start);
        assert_eq!(
            at_touch_boundary.classify_scroll(true, MomentumPhase::None, after(start, 222)),
            DeviceKind::MagicMouse
        );

        let mut at_reset_boundary = both_sources();
        at_reset_boundary.observe_gesture(2, start);
        assert_eq!(
            at_reset_boundary.classify_scroll(true, MomentumPhase::None, after(start, 10)),
            DeviceKind::Trackpad
        );
        assert_eq!(
            at_reset_boundary.classify_scroll(true, MomentumPhase::None, after(start, 333)),
            DeviceKind::Trackpad
        );
    }

    #[test]
    fn discrete_scroll_remains_mouse_with_unknown_inventory() {
        assert_eq!(
            GestureSourceClassifier::default().classify_without_gesture(false),
            DeviceKind::Mouse
        );
    }
}
