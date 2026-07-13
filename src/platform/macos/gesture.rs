//! Public AppKit gesture observation for Magic Mouse/trackpad classification.
//!
//! `core-graphics` models `CGEventType` as a Rust enum and omits AppKit's
//! gesture value (29). Feeding that value through its callback would create an
//! invalid enum discriminant, so this passive tap deliberately uses a tiny raw
//! C bridge with `u32` event types. The active scroll tap remains on the safe
//! crate wrapper.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Instant;

use core_foundation::base::TCFType;
use core_foundation::mach_port::{CFMachPort, CFMachPortRef};
use core_foundation::runloop::{CFRunLoop, CFRunLoopSource, kCFRunLoopCommonModes};
use core_foundation_sys::mach_port::CFMachPortInvalidate;
use core_graphics::event::CGEvent;
use objc2::rc::autoreleasepool;
use objc2_app_kit::{NSEvent, NSTouchPhase};
use objc2_core_graphics::CGEvent as ObjcCGEvent;

use crate::device::DeviceKind;
use crate::device_classifier::{ContinuousSourceHint, GestureSourceClassifier, MomentumPhase};
use crate::error::{AppError, AppResult};

const SESSION_EVENT_TAP: u32 = 1;
const TAIL_APPEND_EVENT_TAP: u32 = 1;
const LISTEN_ONLY_EVENT_TAP: u32 = 1;
const GESTURE_EVENT_TYPE: u32 = 29;
const GESTURE_EVENT_MASK: u64 = 1_u64 << GESTURE_EVENT_TYPE;
const TAP_DISABLED_BY_TIMEOUT: u32 = u32::MAX - 1;
const TAP_DISABLED_BY_USER_INPUT: u32 = u32::MAX;
const SCROLL_MOMENTUM_PHASE_FIELD: u32 = 123;

type GestureTapCallback = unsafe extern "C-unwind" fn(
    proxy: *mut c_void,
    event_type: u32,
    event: *mut c_void,
    user_info: *mut c_void,
) -> *mut c_void;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: u64,
        callback: Option<GestureTapCallback>,
        user_info: *mut c_void,
    ) -> CFMachPortRef;
    fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
}

static CLASSIFIER: OnceLock<Mutex<GestureSourceClassifier>> = OnceLock::new();
static GESTURE_AVAILABLE: AtomicBool = AtomicBool::new(false);
static GESTURE_PORT: AtomicUsize = AtomicUsize::new(0);

fn classifier_guard() -> MutexGuard<'static, GestureSourceClassifier> {
    let classifier = CLASSIFIER.get_or_init(|| Mutex::new(GestureSourceClassifier::default()));
    match classifier.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn reset_classifier(source_hint: ContinuousSourceHint) {
    *classifier_guard() = GestureSourceClassifier::new(source_hint);
}

unsafe extern "C-unwind" fn handle_gesture_event(
    _proxy: *mut c_void,
    event_type: u32,
    event: *mut c_void,
    _user_info: *mut c_void,
) -> *mut c_void {
    if matches!(
        event_type,
        TAP_DISABLED_BY_TIMEOUT | TAP_DISABLED_BY_USER_INPUT
    ) {
        let port = GESTURE_PORT.load(Ordering::Acquire);
        if port != 0 {
            unsafe { CGEventTapEnable(port as CFMachPortRef, true) };
        }
        return event;
    }

    if event_type != GESTURE_EVENT_TYPE || event.is_null() {
        return event;
    }

    autoreleasepool(|_| {
        // CoreGraphics guarantees a live CGEventRef for the duration of this
        // callback. objc2's CGEvent is the same opaque Core Foundation object;
        // borrowing it here neither retains nor takes ownership of the event.
        let cg_event = unsafe { &*event.cast::<ObjcCGEvent>() };
        if let Some(ns_event) = NSEvent::eventWithCGEvent(cg_event) {
            let touching = ns_event
                .touchesMatchingPhase_inView(NSTouchPhase::Touching, None)
                .count();
            classifier_guard().observe_gesture(touching, Instant::now());
        }
    });

    event
}

/// Owns the passive gesture tap and its source for exactly one run-loop run.
pub struct GestureMonitor {
    tap: CFMachPort,
    source: CFRunLoopSource,
    run_loop: CFRunLoop,
}

impl GestureMonitor {
    pub fn start(source_hint: ContinuousSourceHint) -> AppResult<Self> {
        if GESTURE_PORT.load(Ordering::Acquire) != 0 {
            return Err(AppError::Platform(
                "a gesture monitor is already installed in this process".to_string(),
            ));
        }

        // Configure the inventory fallback before creating the optional tap.
        // If tap creation fails, `classify_scroll` can still use exclusive
        // trackpad/Magic Mouse evidence instead of guessing from continuity.
        reset_classifier(source_hint);
        let raw_tap = unsafe {
            CGEventTapCreate(
                SESSION_EVENT_TAP,
                TAIL_APPEND_EVENT_TAP,
                LISTEN_ONLY_EVENT_TAP,
                GESTURE_EVENT_MASK,
                Some(handle_gesture_event),
                std::ptr::null_mut(),
            )
        };
        if raw_tap.is_null() {
            return Err(AppError::Platform(
                "failed to create the passive AppKit gesture event tap".to_string(),
            ));
        }

        let tap = unsafe { CFMachPort::wrap_under_create_rule(raw_tap) };
        let source = match tap.create_runloop_source(0) {
            Ok(source) => source,
            Err(()) => {
                unsafe { CFMachPortInvalidate(tap.as_concrete_TypeRef()) };
                return Err(AppError::Platform(
                    "failed to create the gesture tap run-loop source".to_string(),
                ));
            }
        };
        let run_loop = CFRunLoop::get_current();
        run_loop.add_source(&source, unsafe { kCFRunLoopCommonModes });

        let port = tap.as_concrete_TypeRef() as usize;
        GESTURE_PORT.store(port, Ordering::Release);
        unsafe { CGEventTapEnable(tap.as_concrete_TypeRef(), true) };
        GESTURE_AVAILABLE.store(true, Ordering::Release);

        Ok(Self {
            tap,
            source,
            run_loop,
        })
    }

    pub fn port(&self) -> usize {
        self.tap.as_concrete_TypeRef() as usize
    }
}

impl Drop for GestureMonitor {
    fn drop(&mut self) {
        let port = self.port();
        if GESTURE_PORT
            .compare_exchange(port, 0, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            GESTURE_AVAILABLE.store(false, Ordering::Release);
            reset_classifier(ContinuousSourceHint::Unknown);
        }
        self.run_loop
            .remove_source(&self.source, unsafe { kCFRunLoopCommonModes });
        unsafe { CFMachPortInvalidate(self.tap.as_concrete_TypeRef()) };
    }
}

/// Classifies one active scroll event using the optional passive signal.
pub fn classify_scroll(
    event: &CGEvent,
    continuous: bool,
    live_source_hint: Option<ContinuousSourceHint>,
) -> DeviceKind {
    let mut classifier = classifier_guard();
    if let Some(source_hint) = live_source_hint {
        classifier.update_source_hint(source_hint);
    }
    if !GESTURE_AVAILABLE.load(Ordering::Acquire) {
        return classifier.classify_without_gesture(continuous);
    }

    let momentum_phase = match event.get_integer_value_field(SCROLL_MOMENTUM_PHASE_FIELD) {
        0 => MomentumPhase::None,
        1 => MomentumPhase::Began,
        2 => MomentumPhase::Continued,
        3 => MomentumPhase::Ended,
        _ => MomentumPhase::Unknown,
    };
    classifier.classify_scroll(continuous, momentum_phase, Instant::now())
}
