//! Menu-bar tray icon for the merged settings-window + event-tap process.
//!
//! Built with `tray-icon` (crates.io, Tauri org), which wraps `NSStatusItem`
//! on macOS. Per recommendation.md risk #4/#11: this needs the main thread
//! (same as any other AppKit/NSStatusItem work), so it must be built and
//! polled from the same thread eframe already owns - never from the
//! CGEventTap's background thread. `ui.rs` builds this once inside
//! `eframe::App::update` on the first frame and polls its event receivers
//! every frame after that; it never touches this from another thread.
//!
//! Menu is intentionally minimal per the plan: "Open Settings" and "Quit"
//! only - see recommendation.md risk #11 (tray design was explicitly left
//! unspecified beyond a working minimal menu, not a rich one).

use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder, TrayIconEvent};

const OPEN_SETTINGS_ID: &str = "open-settings";
const QUIT_ID: &str = "quit";

/// What the user asked for via the tray menu, polled once per frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    OpenSettings,
    Quit,
}

/// Owns the live `TrayIcon`. Dropping this removes the menu-bar icon (it
/// does not itself terminate the process - `TrayAction::Quit` is handled by
/// the caller, per the plan, typically via `std::process::exit`).
pub struct TrayHandle {
    _icon: TrayIcon,
}

/// Builds the tray icon and its menu. Must be called on the main thread
/// (eframe's thread), same as every other tray-icon/NSStatusItem call.
pub fn build() -> Result<TrayHandle, String> {
    let menu = Menu::new();
    let open_settings =
        MenuItem::with_id(MenuId::new(OPEN_SETTINGS_ID), "Open Settings", true, None);
    let quit = MenuItem::with_id(MenuId::new(QUIT_ID), "Quit", true, None);
    menu.append(&open_settings)
        .map_err(|error| format!("could not build tray menu: {error}"))?;
    menu.append(&quit)
        .map_err(|error| format!("could not build tray menu: {error}"))?;

    let icon = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Auto Reverse")
        .with_icon(default_icon())
        .build()
        .map_err(|error| format!("could not create the menu-bar icon: {error}"))?;

    Ok(TrayHandle { _icon: icon })
}

/// Non-blocking poll for a tray action, meant to be called once per eframe
/// update tick. Reads both the tray-icon-click channel (for tray-native
/// interactions such as double-click) and the muda menu-item-click channel
/// (for the actual "Open Settings"/"Quit" items), and maps either back to a
/// `TrayAction`.
pub fn poll_action() -> Option<TrayAction> {
    if let Ok(event) = MenuEvent::receiver().try_recv() {
        return match event.id().0.as_str() {
            OPEN_SETTINGS_ID => Some(TrayAction::OpenSettings),
            QUIT_ID => Some(TrayAction::Quit),
            _ => None,
        };
    }

    // Left-click on the tray icon itself (outside the menu) is treated the
    // same as "Open Settings" - the common menu-bar-app convention.
    if let Ok(TrayIconEvent::Click { .. }) = TrayIconEvent::receiver().try_recv() {
        return Some(TrayAction::OpenSettings);
    }

    None
}

/// A tiny solid-color square. Not a real design asset (recommendation.md
/// risk #11 flags icon design as unspecified/out of scope) - just enough
/// pixels for `tray-icon` to have something valid to hand to `NSStatusItem`
/// so the process-lifecycle behavior can actually be exercised end to end.
fn default_icon() -> tray_icon::Icon {
    const SIZE: u32 = 16;
    let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for _ in 0..(SIZE * SIZE) {
        rgba.extend_from_slice(&[0x33, 0x33, 0x33, 0xFF]);
    }
    tray_icon::Icon::from_rgba(rgba, SIZE, SIZE).expect("fixed-size solid icon is always valid")
}
