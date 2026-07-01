// AXIsProcessTrusted is the standard public check for Accessibility trust.
// CGPreflightListenEventAccess/CGRequestListenEventAccess (CGEvent.h, macOS
// 10.15+) are the equivalent pair for the separate Input Monitoring
// permission CGEventTap also needs - verified directly against this
// machine's SDK header, not assumed.
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
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

/// Prompts the system to show the Input Monitoring consent dialog if this
/// process is not yet approved. A no-op if access was already granted.
pub fn request_input_monitoring_access() -> bool {
    unsafe { CGRequestListenEventAccess() }
}

/// Whether both permissions CGEventTap depends on are currently granted.
pub fn is_trusted() -> bool {
    has_accessibility_trust() && has_input_monitoring_access()
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
         Open System Settings > Privacy & Security, add this binary to\n\
         whichever list(s) above say \"required\" (use the \"+\" button and\n\
         pick the compiled executable, e.g. target/debug/auto-reverse), then\n\
         run it again. Rebuilding the binary changes its identity, so you\n\
         will need to re-add it after every `cargo build` during development.",
        permission_status(has_accessibility_trust()),
        permission_status(has_input_monitoring_access()),
    );
}

fn permission_status(granted: bool) -> &'static str {
    if granted { "granted" } else { "required" }
}
