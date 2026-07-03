//! Login-item registration for the GUI app bundle via `SMAppService`.
//!
//! This is a thin wrapper around `SMAppService.mainAppService()` (the
//! ServiceManagement framework, macOS 13+), which registers the *current
//! app bundle itself* as a login item - the whole bundle relaunches at
//! login exactly as if the user opened it, with whatever argv/behavior its
//! `Info.plist` or launcher script already gives it. Deliberately NOT
//! `agentServiceWithPlistName`: that variant needs a plist baked into
//! `Contents/Library/LaunchAgents/<label>.plist` inside the bundle at build
//! time, which `scripts/build-app-bundle.sh` does not do, and adding that
//! build step is out of scope here.
//!
//! This is completely separate from `startup.rs` (the hand-rolled
//! `~/Library/LaunchAgents` plist writer used by the CLI's
//! `enable-startup`/`disable-startup`, which targets the headless `run`
//! binary directly). Two different mechanisms for two different use cases
//! - see recommendation.md risk #6. Do not unify them.
//!
//! Accepted risk (recommendation.md risk #1): an Apple DTS engineer has
//! stated that ad-hoc code signing (`codesign --sign -`, exactly what
//! `scripts/build-app-bundle.sh` does today) can make `SMAppService`
//! registration unstable. This module does not work around that; it only
//! wraps the API faithfully and reports what `status()` actually says.
//!
//! Gated on the `gui` feature: it only makes sense for the bundled app, not
//! the lean CLI-only build.

use objc2_foundation::NSString;
use objc2_service_management::{SMAppService, SMAppServiceStatus};

/// Mirrors `SMAppServiceStatus` so callers do not need to depend on objc2
/// types directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginItemStatus {
    /// Not registered, or was registered and then unregistered.
    NotRegistered,
    /// Registered and eligible to run at login.
    Enabled,
    /// Registered, but needs the user's approval in System Settings before
    /// it will actually run.
    RequiresApproval,
    /// ServiceManagement could not find a matching service at all.
    NotFound,
}

impl LoginItemStatus {
    fn from_raw(status: SMAppServiceStatus) -> Self {
        match status {
            SMAppServiceStatus::Enabled => Self::Enabled,
            SMAppServiceStatus::RequiresApproval => Self::RequiresApproval,
            SMAppServiceStatus::NotFound => Self::NotFound,
            // NotRegistered and any future/unknown raw value both mean
            // "nothing usable is registered" from this app's point of view.
            _ => Self::NotRegistered,
        }
    }

    pub fn summary(self) -> &'static str {
        match self {
            Self::NotRegistered => "not registered",
            Self::Enabled => "enabled",
            Self::RequiresApproval => {
                "registered, but needs approval in System Settings > General > Login Items"
            }
            Self::NotFound => "not found (ServiceManagement has no record of this service)",
        }
    }
}

fn main_app_service() -> objc2::rc::Retained<SMAppService> {
    // SAFETY: `mainAppService` is a simple Objective-C class method with no
    // preconditions beyond the ServiceManagement framework being linked,
    // which objc2-service-management guarantees.
    unsafe { SMAppService::mainAppService() }
}

/// Registers this app bundle as a login item. Requires user approval the
/// first time (macOS shows this in System Settings > General > Login
/// Items); `status()` reports `RequiresApproval` until the user acts.
pub fn register() -> Result<(), String> {
    let service = main_app_service();
    // SAFETY: `registerAndReturnError` is a plain Objective-C message send
    // documented to be safe to call at any time; the returned `NSError` (if
    // any) is owned and converted to an owned `String` before this function
    // returns, so no borrowed Objective-C state escapes.
    unsafe { service.registerAndReturnError() }
        .map_err(|error| error.localizedDescription().to_string())
}

/// Unregisters this app bundle as a login item.
pub fn unregister() -> Result<(), String> {
    let service = main_app_service();
    // SAFETY: see `register` above.
    unsafe { service.unregisterAndReturnError() }
        .map_err(|error| error.localizedDescription().to_string())
}

/// Current registration status, read fresh from ServiceManagement.
pub fn status() -> LoginItemStatus {
    let service = main_app_service();
    // SAFETY: `status` is a plain, side-effect-free Objective-C message
    // send.
    let raw = unsafe { service.status() };
    LoginItemStatus::from_raw(raw)
}

/// Bundle identifier this module registers, for diagnostics only (matches
/// `CFBundleIdentifier` in `scripts/build-app-bundle.sh`'s `Info.plist`).
/// `mainAppService()` reads this from the running bundle itself; this
/// constant is not passed to any objc2 call, it is only for humans reading
/// `doctor`/UI output.
pub const BUNDLE_IDENTIFIER: &str = "com.auto-reverse.app";

// NSString round-trip helper reserved for future use (e.g. if a
// diagnostics view wants to show the identifier string via NSString);
// unused today, so keep it out of the public surface to avoid dead_code
// warnings. Kept as a private fn instead of deleted so the intent (why
// objc2_foundation::NSString is a real dependency here) is documented.
#[allow(dead_code)]
fn identifier_nsstring() -> objc2::rc::Retained<NSString> {
    NSString::from_str(BUNDLE_IDENTIFIER)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_labels_are_distinct_and_actionable() {
        let all = [
            LoginItemStatus::NotRegistered,
            LoginItemStatus::Enabled,
            LoginItemStatus::RequiresApproval,
            LoginItemStatus::NotFound,
        ];
        let labels: Vec<&str> = all.iter().map(|s| s.summary()).collect();
        for (i, a) in labels.iter().enumerate() {
            for (j, b) in labels.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b, "status summaries must be distinguishable");
                }
            }
        }
    }
}
