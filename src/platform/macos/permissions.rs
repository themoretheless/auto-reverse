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
// The AX functions return the Carbon-era `Boolean` (a plain `unsigned char`,
// per CFBase.h - any nonzero byte is a valid "true"), not the two-valued C99
// `bool` the CoreGraphics functions below use. Declaring these `-> bool`
// would be unsound: Rust's `bool` has a hard 0x00/0x01 validity invariant,
// so a real implementation returning e.g. 0xFF for true would be undefined
// behavior the moment it crosses the FFI boundary. Bind them as `u8` and
// compare explicitly instead.
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> u8;
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> u8;
    static kAXTrustedCheckOptionPrompt: CFStringRef;
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightListenEventAccess() -> bool;
    fn CGRequestListenEventAccess() -> bool;
}

/// Whether the user has already granted this process Accessibility trust.
pub fn has_accessibility_trust() -> bool {
    unsafe { AXIsProcessTrusted() != 0 }
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
        AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef()) != 0
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
         Otherwise open System Settings > Privacy & Security, add Auto\n\
         Reverse.app to whichever list(s) above say \"required\". Build it\n\
         with `scripts/build-app-bundle.sh`, then use the \"+\" button and\n\
         pick `target/debug/Auto Reverse.app`. If you are running the raw\n\
         CLI instead of the app bundle, add that exact executable path.\n\
         Rebuilding can change the code identity, so remove and re-add the\n\
         app/binary if macOS keeps denying access.",
        permission_status(has_accessibility_trust()),
        permission_status(has_input_monitoring_access()),
    );
}

pub fn permission_status(granted: bool) -> &'static str {
    if granted { "granted" } else { "required" }
}
