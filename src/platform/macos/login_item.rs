//! Login-item registration for the GUI app bundle via `SMAppService`.
//!
//! This is a thin wrapper around `SMAppService.mainAppService()` (the
//! ServiceManagement framework, macOS 13+), which registers the *current
//! app bundle itself* as a login item - the whole bundle relaunches at
//! login exactly as if the user opened it, with whatever argv/behavior its
//! `Info.plist` and binary entrypoint already give it. Deliberately NOT
//! `agentServiceWithPlistName`: that variant needs a plist baked into
//! `Contents/Library/LaunchAgents/<label>.plist` inside the bundle at build
//! time, which `scripts/build-app-bundle.sh` does not do, and adding that
//! build step is out of scope here.
//!
//! The CLI still has a hand-rolled `~/Library/LaunchAgents` fallback in
//! `startup.rs` for lean/headless installs. Registration through this module
//! removes that legacy LaunchAgent so one login cannot start both the GUI and
//! a headless runtime. The two adapters remain separate; only their ownership
//! policy is coordinated here.
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

use super::startup;

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

fn register_main_app_service() -> Result<(), String> {
    let service = main_app_service();
    // SAFETY: `registerAndReturnError` is a plain Objective-C message send
    // documented to be safe to call at any time; the returned `NSError` (if
    // any) is owned and converted to an owned `String` before this function
    // returns, so no borrowed Objective-C state escapes.
    unsafe { service.registerAndReturnError() }
        .map_err(|error| error.localizedDescription().to_string())
}

fn unregister_main_app_service() -> Result<(), String> {
    let service = main_app_service();
    // SAFETY: see `register_main_app_service` above.
    unsafe { service.unregisterAndReturnError() }
        .map_err(|error| error.localizedDescription().to_string())
}

fn register_exclusively_with(
    already_registered: bool,
    register_service: impl FnOnce() -> Result<(), String>,
    disable_legacy_agent: impl FnOnce() -> Result<(), String>,
    rollback_service: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    if !already_registered {
        register_service()?;
    }

    if let Err(error) = disable_legacy_agent() {
        if already_registered {
            return Err(format!(
                "GUI login item is registered, but the legacy CLI LaunchAgent could not be removed: {error}"
            ));
        }

        return match rollback_service() {
            Ok(()) => Err(format!(
                "could not remove the legacy CLI LaunchAgent; GUI login-item registration was rolled back: {error}"
            )),
            Err(rollback_error) => Err(format!(
                "could not remove the legacy CLI LaunchAgent ({error}); GUI login-item rollback also failed ({rollback_error})"
            )),
        };
    }

    Ok(())
}

/// Registers this app bundle as the sole login-time runtime owner.
///
/// macOS may require approval in System Settings; `status()` reports
/// `RequiresApproval` until the user acts. A successfully registered GUI
/// replaces the legacy CLI LaunchAgent. If removing that agent fails after a
/// fresh registration, the GUI registration is rolled back so the operation
/// does not intentionally leave two startup owners behind.
pub fn register() -> Result<(), String> {
    let already_registered = matches!(
        status(),
        LoginItemStatus::Enabled | LoginItemStatus::RequiresApproval
    );
    register_exclusively_with(
        already_registered,
        register_main_app_service,
        || {
            startup::disable()
                .map(|_| ())
                .map_err(|error| error.to_string())
        },
        unregister_main_app_service,
    )
}

/// Unregisters this app bundle as a login item.
pub fn unregister() -> Result<(), String> {
    unregister_main_app_service()
}

fn needs_legacy_reconciliation(gui_status: LoginItemStatus, legacy_installed: bool) -> bool {
    legacy_installed
        && matches!(
            gui_status,
            LoginItemStatus::Enabled | LoginItemStatus::RequiresApproval
        )
}

/// Removes a legacy LaunchAgent left alongside an already-registered GUI
/// login item by an older release. This is idempotent and performs no launchd
/// work when either owner is absent.
pub fn reconcile_startup_ownership() -> Result<bool, String> {
    let gui_status = status();
    if !matches!(
        gui_status,
        LoginItemStatus::Enabled | LoginItemStatus::RequiresApproval
    ) {
        return Ok(false);
    }
    let legacy_status =
        startup::status_for_current_executable().map_err(|error| error.to_string())?;
    if !needs_legacy_reconciliation(gui_status, legacy_status.installed) {
        return Ok(false);
    }

    startup::disable().map_err(|error| error.to_string())?;
    Ok(true)
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
    use std::cell::RefCell;

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

    #[test]
    fn exclusive_registration_registers_then_removes_legacy_agent() {
        let calls = RefCell::new(Vec::new());
        register_exclusively_with(
            false,
            || {
                calls.borrow_mut().push("register");
                Ok(())
            },
            || {
                calls.borrow_mut().push("disable-legacy");
                Ok(())
            },
            || {
                calls.borrow_mut().push("rollback");
                Ok(())
            },
        )
        .expect("exclusive registration should succeed");

        assert_eq!(*calls.borrow(), ["register", "disable-legacy"]);
    }

    #[test]
    fn failed_service_registration_leaves_the_legacy_agent_alone() {
        let calls = RefCell::new(Vec::new());
        let error = register_exclusively_with(
            false,
            || {
                calls.borrow_mut().push("register");
                Err("service unavailable".to_string())
            },
            || {
                calls.borrow_mut().push("disable-legacy");
                Ok(())
            },
            || {
                calls.borrow_mut().push("rollback");
                Ok(())
            },
        )
        .expect_err("registration failure must be reported");

        assert_eq!(error, "service unavailable");
        assert_eq!(*calls.borrow(), ["register"]);
    }

    #[test]
    fn exclusive_registration_rolls_back_a_fresh_service_on_migration_failure() {
        let calls = RefCell::new(Vec::new());
        let error = register_exclusively_with(
            false,
            || {
                calls.borrow_mut().push("register");
                Ok(())
            },
            || {
                calls.borrow_mut().push("disable-legacy");
                Err("permission denied".to_string())
            },
            || {
                calls.borrow_mut().push("rollback");
                Ok(())
            },
        )
        .expect_err("migration failure must be reported");

        assert!(error.contains("rolled back"));
        assert_eq!(*calls.borrow(), ["register", "disable-legacy", "rollback"]);
    }

    #[test]
    fn existing_registration_is_preserved_when_legacy_removal_fails() {
        let calls = RefCell::new(Vec::new());
        let error = register_exclusively_with(
            true,
            || {
                calls.borrow_mut().push("register");
                Ok(())
            },
            || {
                calls.borrow_mut().push("disable-legacy");
                Err("read-only directory".to_string())
            },
            || {
                calls.borrow_mut().push("rollback");
                Ok(())
            },
        )
        .expect_err("migration failure must be reported");

        assert!(error.contains("is registered"));
        assert_eq!(*calls.borrow(), ["disable-legacy"]);
    }

    #[test]
    fn rollback_failure_reports_both_causes() {
        let error = register_exclusively_with(
            false,
            || Ok(()),
            || Err("legacy removal failed".to_string()),
            || Err("service rollback failed".to_string()),
        )
        .expect_err("both failures must be reported");

        assert!(error.contains("legacy removal failed"));
        assert!(error.contains("service rollback failed"));
    }

    #[test]
    fn reconciliation_runs_only_when_both_login_owners_exist() {
        assert!(needs_legacy_reconciliation(LoginItemStatus::Enabled, true));
        assert!(needs_legacy_reconciliation(
            LoginItemStatus::RequiresApproval,
            true
        ));
        assert!(!needs_legacy_reconciliation(
            LoginItemStatus::NotRegistered,
            true
        ));
        assert!(!needs_legacy_reconciliation(
            LoginItemStatus::Enabled,
            false
        ));
    }
}
