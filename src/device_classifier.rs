//! Pure scroll-source classification.
//!
//! AppKit supplies only observations (continuous scroll, momentum phase, and
//! whether a recent gesture had at least two touching fingers). Keeping the
//! timing policy here makes the heuristic deterministic and testable without
//! a live event tap or any macOS framework types.

use std::time::{Duration, Instant};

use crate::device::DeviceKind;

pub const CLASSIFIER_DESCRIPTION: &str = "discrete wheel = mouse; recent two-finger gesture = trackpad; other continuous scroll = Magic Mouse-like";

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

/// Stateful public-API heuristic for separating two continuous sources.
///
/// A two-finger AppKit gesture immediately preceding a scroll identifies a
/// trackpad. Momentum events inherit the last continuous source. A normal
/// continuous event after the touch observation has gone stale is treated as
/// Magic Mouse-like. This intentionally mirrors Scroll Reverser's proven
/// timing model while preserving a distinct `MagicMouse` domain value.
#[derive(Debug, Clone)]
pub struct GestureSourceClassifier {
    last_two_finger_touch: Option<Instant>,
    two_finger_touch_pending: bool,
    last_continuous_kind: DeviceKind,
}

impl Default for GestureSourceClassifier {
    fn default() -> Self {
        Self {
            last_two_finger_touch: None,
            two_finger_touch_pending: false,
            last_continuous_kind: DeviceKind::MagicMouse,
        }
    }
}

impl GestureSourceClassifier {
    pub fn observe_gesture(&mut self, touching_fingers: usize, now: Instant) {
        if touching_fingers >= 2 {
            self.last_two_finger_touch = Some(now);
            self.two_finger_touch_pending = true;
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
}

/// Safe fallback when the passive gesture monitor cannot be installed.
/// It preserves the pre-classifier behavior and therefore never starts
/// reversing a user's trackpad merely because the optional signal failed.
pub fn conservative_kind_from_continuity(continuous: bool) -> DeviceKind {
    if continuous {
        DeviceKind::Trackpad
    } else {
        DeviceKind::Mouse
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn after(start: Instant, milliseconds: u64) -> Instant {
        start + Duration::from_millis(milliseconds)
    }

    #[test]
    fn discrete_scroll_is_always_a_mouse() {
        let now = Instant::now();
        let mut classifier = GestureSourceClassifier::default();
        classifier.observe_gesture(2, now);

        assert_eq!(
            classifier.classify_scroll(false, MomentumPhase::None, now),
            DeviceKind::Mouse
        );
    }

    #[test]
    fn discrete_scroll_consumes_pending_touch_observation() {
        let start = Instant::now();
        let mut classifier = GestureSourceClassifier::default();
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
        let mut classifier = GestureSourceClassifier::default();
        classifier.observe_gesture(2, start);

        assert_eq!(
            classifier.classify_scroll(true, MomentumPhase::None, after(start, 100)),
            DeviceKind::Trackpad
        );
    }

    #[test]
    fn one_finger_gesture_is_ignored() {
        let now = Instant::now();
        let mut classifier = GestureSourceClassifier::default();
        classifier.observe_gesture(1, now);

        assert_eq!(
            classifier.classify_scroll(true, MomentumPhase::None, now),
            DeviceKind::MagicMouse
        );
    }

    #[test]
    fn continuous_scroll_without_touch_is_magic_mouse_like() {
        let now = Instant::now();
        let mut classifier = GestureSourceClassifier::default();

        assert_eq!(
            classifier.classify_scroll(true, MomentumPhase::None, now),
            DeviceKind::MagicMouse
        );
    }

    #[test]
    fn momentum_keeps_the_last_trackpad_source_after_touch_expires() {
        let start = Instant::now();
        let mut classifier = GestureSourceClassifier::default();
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
        let mut classifier = GestureSourceClassifier::default();
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
        let mut classifier = GestureSourceClassifier::default();
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
        let mut at_touch_boundary = GestureSourceClassifier::default();
        at_touch_boundary.observe_gesture(2, start);
        assert_eq!(
            at_touch_boundary.classify_scroll(true, MomentumPhase::None, after(start, 222)),
            DeviceKind::MagicMouse
        );

        let mut at_reset_boundary = GestureSourceClassifier::default();
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
    fn conservative_fallback_preserves_previous_behavior() {
        assert_eq!(conservative_kind_from_continuity(false), DeviceKind::Mouse);
        assert_eq!(
            conservative_kind_from_continuity(true),
            DeviceKind::Trackpad
        );
    }
}
