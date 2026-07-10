//! Main-thread AppKit adapter for system sleep/wake notifications.
//!
//! `NSWorkspace` owns the notification center for these events. Objective-C
//! callbacks only publish the latest event into a tiny atomic signal; the
//! eframe coordinator polls it on its normal 250 ms tick and owns every
//! lifecycle decision. Keeping callback and policy separate makes wake
//! recovery deterministic and testable without sleeping the test machine.

use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2_app_kit::{
    NSWorkspace, NSWorkspaceDidWakeNotification, NSWorkspaceWillSleepNotification,
};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSNotificationName};

const NO_EVENT: u8 = 0;
const WILL_SLEEP_EVENT: u8 = 1;
const DID_WAKE_EVENT: u8 = 2;

type ObserverToken = Retained<ProtocolObject<dyn NSObjectProtocol>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerEvent {
    WillSleep,
    DidWake,
}

#[derive(Clone, Default)]
struct PowerSignal {
    latest: Arc<AtomicU8>,
}

impl PowerSignal {
    fn publish(&self, event: PowerEvent) {
        let encoded = match event {
            PowerEvent::WillSleep => WILL_SLEEP_EVENT,
            PowerEvent::DidWake => DID_WAKE_EVENT,
        };
        self.latest.store(encoded, Ordering::Release);
    }

    fn poll(&self) -> Option<PowerEvent> {
        match self.latest.swap(NO_EVENT, Ordering::AcqRel) {
            WILL_SLEEP_EVENT => Some(PowerEvent::WillSleep),
            DID_WAKE_EVENT => Some(PowerEvent::DidWake),
            _ => None,
        }
    }
}

/// Owns both `NSWorkspace` observer tokens. Install from eframe's main-thread
/// logic callback and keep this value alive for as long as wake recovery is
/// needed; dropping it unregisters both callbacks.
pub struct PowerEventObserver {
    center: Retained<NSNotificationCenter>,
    observers: [ObserverToken; 2],
    signal: PowerSignal,
}

impl PowerEventObserver {
    pub fn install() -> Result<Self, String> {
        let _main_thread = MainThreadMarker::new().ok_or_else(|| {
            "sleep/wake observer must be installed on the main thread".to_string()
        })?;
        let center = NSWorkspace::sharedWorkspace().notificationCenter();
        let signal = PowerSignal::default();
        let will_sleep = observe(
            &center,
            unsafe { NSWorkspaceWillSleepNotification },
            PowerEvent::WillSleep,
            signal.clone(),
        );
        let did_wake = observe(
            &center,
            unsafe { NSWorkspaceDidWakeNotification },
            PowerEvent::DidWake,
            signal.clone(),
        );

        Ok(Self {
            center,
            observers: [will_sleep, did_wake],
            signal,
        })
    }

    /// Returns only the latest unconsumed event. A normal sleep/wake pair can
    /// arrive while the UI thread is suspended; in that case `DidWake` is the
    /// useful final state and intentionally replaces the earlier sleep signal.
    pub fn poll(&self) -> Option<PowerEvent> {
        self.signal.poll()
    }
}

impl Drop for PowerEventObserver {
    fn drop(&mut self) {
        for observer in &self.observers {
            let observer: &ProtocolObject<dyn NSObjectProtocol> = observer;
            let object: &AnyObject = observer.as_ref();
            unsafe { self.center.removeObserver(object) };
        }
    }
}

fn observe(
    center: &NSNotificationCenter,
    name: &NSNotificationName,
    event: PowerEvent,
    signal: PowerSignal,
) -> ObserverToken {
    let handler = RcBlock::new(move |_notification: NonNull<NSNotification>| {
        signal.publish(event);
    });

    unsafe { center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &handler) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_power_event_wins_and_poll_consumes_it() {
        let signal = PowerSignal::default();
        signal.publish(PowerEvent::WillSleep);
        signal.publish(PowerEvent::DidWake);

        assert_eq!(signal.poll(), Some(PowerEvent::DidWake));
        assert_eq!(signal.poll(), None);
    }
}
