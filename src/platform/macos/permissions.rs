use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::string::{CFString, CFStringRef};
use std::env;
use std::ffi::OsStr;
use std::path::Path;

// AXIsProcessTrustedWithOptions + kAXTrustedCheckOptionPrompt (AXUIElement.h)
// is the documented way to make the OS actually show the Accessibility
// consent dialog; plain AXIsProcessTrusted never prompts, only reports.
//
// Auto Reverse installs an active CGEventTap that observes and modifies scroll
// events. Accessibility grants both event posting and listening; Input
// Monitoring grants listening only and is therefore neither sufficient nor a
// separate requirement for this runtime. Do not gate startup on
// CGPreflightListenEventAccess: macOS can keep that value false in a process
// whose Accessibility grant already allows the tap, which was the exact
// false-negative reproduced on 2026-07-13.
//
// The AX functions return the Carbon-era `Boolean` (a plain `unsigned char`,
// per CFBase.h - any nonzero byte is a valid "true"), not Rust's two-valued
// `bool`. Declaring these `-> bool` would be unsound: Rust's `bool` has a hard
// 0x00/0x01 validity invariant, so a real implementation returning e.g. 0xFF
// for true would be undefined behavior the moment it crosses the FFI
// boundary. Bind them as `u8` and compare explicitly instead.
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> u8;
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> u8;
    static kAXTrustedCheckOptionPrompt: CFStringRef;
}

/// Whether the user has already granted this process Accessibility trust.
pub fn has_accessibility_trust() -> bool {
    unsafe { AXIsProcessTrusted() != 0 }
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

/// Whether the active scroll tap can observe and modify events.
pub fn has_scroll_control_access() -> bool {
    scroll_control_access_from(has_accessibility_trust())
}

/// Requests the one TCC grant required by the active scroll tap.
pub fn request_scroll_control_access() -> bool {
    scroll_control_access_from(has_accessibility_trust() || request_accessibility_trust())
}

/// Prepares permission state for a persisted feature state. Disabled features
/// perform a read-only check and never trigger a TCC consent dialog.
pub fn prepare_scroll_control_access(feature_enabled: bool) -> bool {
    let granted = has_accessibility_trust();
    if should_prompt_for_feature(feature_enabled, granted) {
        scroll_control_access_from(request_accessibility_trust())
    } else {
        scroll_control_access_from(granted)
    }
}

const fn scroll_control_access_from(accessibility: bool) -> bool {
    accessibility
}

/// Actionable guidance for the Accessibility grant the active event tap uses.
pub fn print_permission_help() {
    let permission_target = env::current_exe()
        .ok()
        .map(|executable| {
            app_bundle_root(&executable)
                .unwrap_or(&executable)
                .display()
                .to_string()
        })
        .unwrap_or_else(|| "the exact Auto Reverse.app or executable you launched".to_string());
    eprintln!(
        "auto-reverse needs Accessibility permission to control scroll events:\n\
         \n\
         1. Accessibility:    {}\n\
         \n\
         If macOS just showed a permission dialog, approve it and re-run.\n\
         Otherwise open System Settings > Privacy & Security > Accessibility\n\
         and use the \"+\" button. Add this\n\
         exact app or executable: {permission_target}\n\
         \n\
         Input Monitoring is not separately required: Accessibility already\n\
         grants the event listening used by this active scroll tap.\n\
         \n\
         Ad-hoc rebuilds change the code identity. Use a stable Apple\n\
         Development or Developer ID signature; after changing identity,\n\
         remove and re-add the app if macOS keeps denying access.",
        permission_status(has_accessibility_trust()),
    );
}

fn app_bundle_root(executable: &Path) -> Option<&Path> {
    executable
        .ancestors()
        .find(|ancestor| ancestor.extension() == Some(OsStr::new("app")))
}

pub fn permission_status(granted: bool) -> &'static str {
    if granted { "granted" } else { "required" }
}

const fn should_prompt_for_feature(feature_enabled: bool, already_granted: bool) -> bool {
    feature_enabled && !already_granted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_executable_reports_the_app_as_the_permission_target() {
        let executable = Path::new("/Applications/Auto Reverse.app/Contents/MacOS/auto-reverse");
        assert_eq!(
            app_bundle_root(executable),
            Some(Path::new("/Applications/Auto Reverse.app"))
        );
    }

    #[test]
    fn raw_cli_has_no_app_bundle_root() {
        assert_eq!(
            app_bundle_root(Path::new("/usr/local/bin/auto-reverse")),
            None
        );
    }

    #[test]
    fn accessibility_alone_is_enough_for_the_active_scroll_tap() {
        assert!(scroll_control_access_from(true));
        assert!(!scroll_control_access_from(false));
    }

    #[test]
    fn disabled_feature_does_not_select_the_prompting_path() {
        assert!(!should_prompt_for_feature(false, false));
        assert!(!should_prompt_for_feature(false, true));
        assert!(should_prompt_for_feature(true, false));
        assert!(!should_prompt_for_feature(true, true));
    }
}
