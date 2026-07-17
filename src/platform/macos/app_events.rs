//! Main-thread notification bridge for permission/device refresh triggers.

use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2_app_kit::NSApplicationDidBecomeActiveNotification;
use objc2_foundation::{NSNotification, NSNotificationCenter};

type ObserverToken = Retained<ProtocolObject<dyn NSObjectProtocol>>;

/// Coalesces activation notifications. Returning from System Settings is the
/// primary public signal that an Accessibility grant may have changed.
pub struct AppEventObserver {
    center: Retained<NSNotificationCenter>,
    observer: ObserverToken,
    became_active: Arc<AtomicBool>,
}

impl AppEventObserver {
    pub fn install() -> Result<Self, String> {
        let _main_thread = MainThreadMarker::new().ok_or_else(|| {
            "application event observer must be installed on the main thread".to_string()
        })?;
        let center = NSNotificationCenter::defaultCenter();
        let became_active = Arc::new(AtomicBool::new(false));
        let signal = Arc::clone(&became_active);
        let handler = RcBlock::new(move |_notification: NonNull<NSNotification>| {
            signal.store(true, Ordering::Release);
        });
        let observer = unsafe {
            center.addObserverForName_object_queue_usingBlock(
                Some(NSApplicationDidBecomeActiveNotification),
                None,
                None,
                &handler,
            )
        };

        Ok(Self {
            center,
            observer,
            became_active,
        })
    }

    pub fn poll_became_active(&self) -> bool {
        self.became_active.swap(false, Ordering::AcqRel)
    }
}

impl Drop for AppEventObserver {
    fn drop(&mut self) {
        let observer: &ProtocolObject<dyn NSObjectProtocol> = &self.observer;
        let object: &AnyObject = observer.as_ref();
        unsafe { self.center.removeObserver(object) };
    }
}
