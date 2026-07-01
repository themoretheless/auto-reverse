// AXIsProcessTrusted is the standard public check for Accessibility trust.
// There is no equivalent public check for the separate Input Monitoring
// permission that CGEventTap also needs on modern macOS, so a failed tap
// creation is the only reliable signal for that half - see
// `event_tap::install_and_run`'s `Err` case.
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
}

/// Whether the user has already granted this process Accessibility trust.
pub fn is_trusted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

/// Actionable guidance for the two privacy permissions a scroll-wheel event
/// tap depends on, since macOS gives no in-process detail beyond "denied".
pub fn print_permission_help() {
    eprintln!(
        "auto-reverse needs two permissions to intercept scroll events:\n\
         \n\
         1. System Settings > Privacy & Security > Accessibility\n\
         2. System Settings > Privacy & Security > Input Monitoring\n\
         \n\
         Add this binary to both lists (use the \"+\" button and pick the\n\
         compiled executable, e.g. target/debug/auto-reverse), then run it\n\
         again. Rebuilding the binary changes its identity, so you will need\n\
         to re-add it after every `cargo build` during development."
    );
}
