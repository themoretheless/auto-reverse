use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::string::{CFString, CFStringRef};

// AXIsProcessTrustedWithOptions + kAXTrustedCheckOptionPrompt (AXUIElement.h)
// is the documented way to make the OS actually show the Accessibility
// consent dialog; plain AXIsProcessTrusted never prompts, only reports.
// CGPreflightListenEventAccess/CGRequestListenEventAccess (CGEvent.h,
// macOS 10.15+) are the equivalent pair for Input Monitoring. All four
// verified directly against this machine's SDK headers, not assumed.
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
    static kAXTrustedCheckOptionPrompt: CFStringRef;
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightListenEventAccess() -> bool;
    fn CGRequestListenEventAccess() -> bool;
}

/// Whether the user has already granted this process Accessibility trust.
pub fn has_accessibility_trust() -> bool {
    unsafe { AXIsProcessTrusted() }
}

/// Whether the user has already granted this process Input Monitoring
/// access, which CGEventTap needs independently of Accessibility trust.
pub fn has_input_monitoring_access() -> bool {
    unsafe { CGPreflightListenEventAccess() }
}

/// Prompts the system to show the Accessibility consent dialog if this
/// process is not yet approved. A no-op if trust was already granted.
pub fn request_accessibility_trust() -> bool {
    unsafe {
        let prompt_key: CFString = TCFType::wrap_under_get_rule(kAXTrustedCheckOptionPrompt);
        let options = CFDictionary::from_CFType_pairs(&[(prompt_key, CFBoolean::true_value())]);
        AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef())
    }
}

/// Prompts the system to show the Input Monitoring consent dialog if this
/// process is not yet approved. A no-op if access was already granted.
pub fn request_input_monitoring_access() -> bool {
    unsafe { CGRequestListenEventAccess() }
}

/// Whether both permissions CGEventTap depends on are currently granted.
pub fn is_trusted() -> bool {
    has_accessibility_trust() && has_input_monitoring_access()
}

/// Asks the OS to show whichever consent dialogs are still outstanding,
/// then reports whether both permissions ended up granted. Calling the
/// Request variants is safe regardless of order - the two TCC services are
/// independent - and either call is a no-op if already granted, so this can
/// be called every time `run` starts without side effects on repeat runs.
pub fn request_missing_permissions() -> bool {
    let accessibility = has_accessibility_trust() || request_accessibility_trust();
    let input_monitoring = has_input_monitoring_access() || request_input_monitoring_access();
    accessibility && input_monitoring
}

/// Actionable guidance for the two privacy permissions a scroll-wheel event
/// tap depends on, since macOS gives no in-process detail beyond "denied".
pub fn print_permission_help() {
    eprintln!(
        "auto-reverse needs two permissions to intercept scroll events:\n\
         \n\
         1. Accessibility:    {}\n\
         2. Input Monitoring: {}\n\
         \n\
         If macOS just showed a permission dialog, approve it and re-run.\n\
         Otherwise open System Settings > Privacy & Security, add this\n\
         binary to whichever list(s) above say \"required\" (use the \"+\"\n\
         button and pick the compiled executable, e.g.\n\
         target/debug/auto-reverse), then run it again. Rebuilding the\n\
         binary changes its identity, so you will need to re-add it after\n\
         every `cargo build` during development.",
        permission_status(has_accessibility_trust()),
        permission_status(has_input_monitoring_access()),
    );
}

pub fn permission_status(granted: bool) -> &'static str {
    if granted { "granted" } else { "required" }
}
