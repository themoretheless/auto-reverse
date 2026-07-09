//! The settings window (`auto-reverse ui`), built with egui/eframe.
//!
//! Design intent (see architecture.md, "Дизайн продукта"): a quiet system
//! utility panel, not a dashboard. The user opens it rarely - to flip a
//! switch or diagnose permissions - so the hierarchy is: current state
//! first (readable in three seconds), the master switch right under it,
//! then progressively rarer controls. Changes autosave immediately, macOS
//! settings style; there is no Save button to forget.
//!
//! Honesty rules the layout follows:
//! - No control is shown for config fields that do nothing today
//!   (`reverse_magic_mouse`, `reverse_unknown`, the menu-bar/update
//!   placeholders). Rendering dead switches would be lying with widgets.
//! - The trackpad toggle says it also covers a real Magic Mouse, because
//!   the tap cannot tell them apart.
//!
//! ## Merged process (`ui` == settings window + scroll reversal)
//!
//! `ui` and `run` used to be two independent processes: opening the
//! settings window spawned a detached `<binary> run` child. That required
//! a lot of coordination (a flock-based `daemon_lock`, a "Start
//! now"/"Restart" button, a background reaper thread) purely to avoid two
//! live `CGEventTap`s and to let config changes reach an already-running
//! daemon.
//!
//! This is now one process: `SettingsApp::load()` spawns the `CGEventTap`
//! on a background thread in-process (`event_tap::install_and_run`,
//! `platform::macos::event_tap`), sharing one `Arc<RwLock<AppConfig>>`
//! between that thread and the GUI. Every settings change writes through
//! the shared config (in addition to saving to disk), so the very next
//! scroll event picks it up live - no restart, no child process. The
//! `daemon_lock` (`platform::macos::daemon_lock`) is still acquired, now by
//! `install_and_run` itself, so a headless `run` process (still supported,
//! e.g. via `enable-startup`'s LaunchAgent) and this in-process tap thread
//! can never both hold a live tap at once.
//!
//! A menu-bar tray icon (`platform::macos::tray`) is present for the
//! lifetime of the process. Closing the window (red button or Cmd-W) hides
//! it rather than quitting - the window's close is intercepted via
//! `ViewportCommand::CancelClose` and turned into a hide, so the background
//! tap thread and tray icon keep running. Only the tray menu's "Quit"
//! really exits the process (`std::process::exit(0)`), which also tears
//! down the tap thread and releases its `daemon_lock` (process exit closes
//! the lock's file descriptor, which the kernel treats as a release).
//!
//! The window-close intercept above only covers the window-level close
//! event; it does nothing against the separate, application-level standard
//! quit pathway (Cmd-Q, Dock/Activity Monitor "Quit", or an AppleScript
//! `tell application "Auto Reverse" to quit"), which terminates the process
//! directly and would otherwise bypass the hide-not-quit behavior entirely.
//! `platform::macos::quit_handler` closes that gap by overriding the
//! `kAEQuitApplication` Apple Event those all resolve to, at the Apple
//! Event Manager level - see that module's doc comment for why this is
//! done there and not via `NSApplicationDelegate`.

use std::sync::{Arc, RwLock};

use eframe::egui::{self, Color32, RichText, ViewportCommand};

use crate::config::{AppConfig, ConfigStore, DeviceRule};
use crate::platform::macos::{
    daemon_lock, debug_log, event_tap, hid, login_item, permissions, quit_handler, tray,
};

const WINDOW_WIDTH: f32 = 400.0;
const WINDOW_HEIGHT: f32 = 640.0;

/// Launches the settings window and starts scroll reversal in-process;
/// blocks until the user actually quits (via the tray menu), not merely
/// until the window is closed/hidden.
///
/// Acquires its own exclusive lock (a sibling of `daemon_lock`'s own
/// `run.lock`) BEFORE building any window or tray icon. `daemon_lock`
/// itself only gates the CGEventTap, not the GUI - a second `ui` process
/// launched directly (bypassing Finder/LaunchServices' single-instance
/// activation, e.g. `open` twice in a row, or `config.enabled == false` so
/// the tap thread never even attempts to start) would otherwise build a
/// second live window and a second menu-bar icon with nothing to tell the
/// user which one is authoritative. If this lock is already held, this
/// returns an error immediately instead of opening a redundant window.
pub fn run_settings_window() -> Result<(), String> {
    let ui_lock_path = daemon_lock::default_path().with_file_name("ui.lock");
    let Some(_ui_lock) =
        daemon_lock::try_acquire(&ui_lock_path).map_err(|error| error.to_string())?
    else {
        return Err("Auto Reverse is already open".to_string());
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Auto Reverse")
            .with_inner_size([WINDOW_WIDTH, WINDOW_HEIGHT])
            .with_min_inner_size([WINDOW_WIDTH, 480.0]),
        // Keep the process (and the tray icon, and the background
        // CGEventTap thread) alive after the window closes - see the
        // module doc comment. This is eframe 0.35's default already, but
        // set explicitly so the intent is not silently lost if a future
        // eframe version changes its default.
        run_and_return: true,
        ..Default::default()
    };

    eframe::run_native(
        "Auto Reverse",
        options,
        Box::new(|cc| {
            install_system_fonts(&cc.egui_ctx);
            Ok(Box::new(SettingsApp::load()))
        }),
    )
    .map_err(|error| format!("could not open the settings window: {error}"))?;

    // run_native returns here once the window is closed (run_and_return),
    // NOT only on a real quit - closing the window is handled as a hide
    // inside SettingsApp::logic, so reaching this point without the process
    // already having called std::process::exit (tray Quit) would mean every
    // window-close silently ended the process, defeating the entire point
    // of the merge. Block instead of returning, so the tray icon and the
    // background tap thread keep running; only std::process::exit (wired to
    // the tray's Quit action) ever actually ends the process.
    loop {
        std::thread::park();
    }
}

/// Loads the real macOS system fonts (SF Pro / SF Mono) into egui so the
/// settings window and Debug Console render with the OS's own font instead
/// of egui's bundled `Ubuntu-Light`/`Hack` fallback - the handoff (ids "1b"
/// and "1f") implies native system-font rendering throughout, and egui's
/// default look was the single biggest visible gap between the mockup and
/// the built app. Called once from `run_settings_window`'s
/// `eframe::run_native` closure via `cc.egui_ctx` - `CreationContext`'s own
/// doc comment for that field names `egui::Context::set_fonts` as exactly
/// what it is for, which is what this uses (as opposed to the lower-level
/// `Context::add_font`, which would work too but is a thinner, less-visible
/// wrapper around the same `FontDefinitions` this function already needs to
/// build for the "insert at the front of the family, keep everything else"
/// behavior described below).
///
/// Reads `/System/Library/Fonts/SFNS.ttf` (SF Pro, proportional) and
/// `/System/Library/Fonts/SFNSMono.ttf` (SF Mono, monospace) - both at a
/// fixed, well-known path on every modern macOS install, confirmed present
/// on this machine. Each is inserted as the FIRST (highest-priority) font
/// in its family's fallback list, ahead of `FontDefinitions::default()`'s
/// own bundled fonts, rather than replacing the defaults outright - so if
/// either file is missing or unreadable for any reason (a future macOS
/// renames it, sandboxing, anything else), that family silently keeps
/// falling back to egui's bundled font instead of rendering blank/tofu
/// glyphs. This function can never fail or panic: a missing font file is
/// treated as "nothing to prepend", not an error to propagate, so the UI is
/// never left unstyled or crashed by a font that isn't where expected.
fn install_system_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    if let Ok(bytes) = std::fs::read("/System/Library/Fonts/SFNS.ttf") {
        fonts.font_data.insert(
            "SF Pro".to_owned(),
            Arc::new(egui::FontData::from_owned(bytes)),
        );
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "SF Pro".to_owned());
    }

    if let Ok(bytes) = std::fs::read("/System/Library/Fonts/SFNSMono.ttf") {
        fonts.font_data.insert(
            "SF Mono".to_owned(),
            Arc::new(egui::FontData::from_owned(bytes)),
        );
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .insert(0, "SF Mono".to_owned());
    }

    ctx.set_fonts(fonts);
}

struct SettingsApp {
    store: ConfigStore,
    /// Local UI copy, edited directly by widgets. Written through to
    /// `shared_config` (and disk) whenever a control changes.
    config: AppConfig,
    /// Shared with the background CGEventTap thread - this is what makes
    /// config changes apply live instead of requiring a restart.
    shared_config: Arc<RwLock<AppConfig>>,
    devices: Vec<hid::DeviceInfo>,
    /// Why the device list is empty, when it failed rather than genuinely
    /// finding nothing - shown inline instead of silently swallowed.
    devices_error: Option<String>,
    /// Last save failure, shown inline; None while everything persists fine.
    save_error: Option<String>,
    load_error: Option<String>,
    /// Set when the in-process tap thread failed or returned immediately
    /// instead of settling into its run loop.
    tap_error: Option<String>,
    /// Whether this UI process has already tried to start the in-process tap.
    /// If permissions were missing at launch, this stays false so the app can
    /// start automatically once the user grants them in System Settings.
    tap_start_attempted: bool,
    /// True only when the startup handshake timed out, which is the normal
    /// signal that `install_and_run` entered the forever-blocking tap loop.
    tap_thread_running: bool,
    /// Set on a login_item register/unregister failure.
    login_item_error: Option<String>,
    tray: Option<tray::TrayHandle>,
    /// Whether `quit_handler::install()` has run yet - it must happen on
    /// the main thread after `NSApplication` has started, same constraint
    /// as building the tray icon, so it is done lazily on the first tick
    /// rather than in `load()`.
    quit_handler_installed: bool,
    /// Which of the three segmented tabs (handoff "1b") is showing.
    selected_tab: SettingsTab,
    /// Set by the tray's "Open Debug Console…" item (`TrayAction::OpenDebugConsole`)
    /// and cleared when the console viewport's own close button is used.
    /// Drives whether `logic` calls `show_viewport_immediate` for the
    /// console this tick - see `debug_console` below.
    show_debug_console: bool,
    /// State local to the Debug Console window (handoff "1f") - filter tab,
    /// search text, and the last export/clear error, if any. Kept separate
    /// from the settings-window fields above since it is only ever touched
    /// while `show_debug_console` is true.
    debug_console: DebugConsoleState,
}

#[derive(Default)]
struct DebugConsoleState {
    filter: DebugFilter,
    search: String,
    export_error: Option<String>,
    export_success: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum DebugFilter {
    #[default]
    All,
    Reversed,
    Passed,
    Ignored,
}

/// The three tabs of the settings window (handoff "1b" - segmented tabs).
/// Status and the master "Reverse scrolling" toggle stay pinned above these
/// tabs, and Config path/Restore defaults stay pinned below, per the
/// handoff's own design note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsTab {
    General,
    Devices,
    Permissions,
}

impl SettingsApp {
    fn load() -> Self {
        // One-shot, mirrors the old run_event_tap(): the request_* calls are
        // what actually register this binary with TCC (and pop the native
        // consent dialogs) - the has_* checks the permissions panel uses
        // are read-only and never do this. Without it, an install whose
        // only entry point is this window never appears in System
        // Settings > Privacy & Security for the user to grant.
        let permissions_ready = permissions::request_missing_permissions();

        let store = ConfigStore::default();
        let (config, load_error) = match store.load_or_create() {
            Ok(config) => (config, None),
            Err(error) => (AppConfig::default(), Some(error.to_string())),
        };

        let shared_config = Arc::new(RwLock::new(config.clone()));

        let login_item_error = None;

        let mut app = Self {
            store,
            config,
            shared_config,
            devices: Vec::new(),
            devices_error: None,
            save_error: None,
            load_error,
            tap_error: None,
            tap_start_attempted: false,
            tap_thread_running: false,
            login_item_error,
            tray: None,
            quit_handler_installed: false,
            selected_tab: SettingsTab::General,
            show_debug_console: false,
            debug_console: DebugConsoleState::default(),
        };
        // Opening the app is now also how scroll reversal starts, not just
        // where you look at settings. If permissions are missing, leave the
        // attempt pending; `panel_contents` retries automatically when the
        // permission checks turn green.
        if app.config.enabled && permissions_ready {
            app.start_tap_thread();
        }
        app.refresh_devices();
        app
    }

    fn refresh_devices(&mut self) {
        match hid::list_pointing_devices() {
            Ok(devices) => {
                self.devices = devices;
                self.devices_error = None;
            }
            Err(error) => {
                self.devices = Vec::new();
                self.devices_error = Some(error.to_string());
            }
        }
    }

    /// Persists to disk AND publishes to the shared config the background
    /// tap thread reads - this is what makes a settings change apply to the
    /// very next scroll event instead of requiring a restart.
    ///
    /// On a disk-save failure, `self.config` (bound to every widget) is
    /// rolled back to whatever `shared_config` still holds - the last state
    /// that actually persisted and is actually running. Without this, a
    /// failed save left the checkboxes/sliders showing the new, unapplied
    /// values while the live tap thread kept using the old ones, with no
    /// visible sign of the split, and a later successful save would apply
    /// both the old failed edit and the new one together, well after the
    /// user made the first one and after it appeared to fail.
    fn save(&mut self) {
        match self.store.save(&self.config) {
            Ok(()) => {
                self.save_error = None;
                let mut guard = match self.shared_config.write() {
                    Ok(guard) => guard,
                    Err(poisoned) => poisoned.into_inner(),
                };
                *guard = self.config.clone();
            }
            Err(error) => {
                self.save_error = Some(error.to_string());
                let guard = match self.shared_config.read() {
                    Ok(guard) => guard,
                    Err(poisoned) => poisoned.into_inner(),
                };
                self.config = guard.clone();
            }
        }
    }

    fn start_tap_thread(&mut self) {
        self.tap_start_attempted = true;
        match spawn_tap_thread(Arc::clone(&self.shared_config)) {
            TapStartOutcome::Running => {
                self.tap_thread_running = true;
                self.tap_error = None;
            }
            TapStartOutcome::StoppedImmediately => {
                self.tap_thread_running = false;
                self.tap_error = Some(
                    "the tap stopped immediately; another Auto Reverse instance may already be running"
                        .to_string(),
                );
            }
            TapStartOutcome::Failed(error) => {
                self.tap_thread_running = false;
                self.tap_error = Some(error);
            }
        }
    }

    fn sync_config_from_shared(&mut self) {
        let guard = match self.shared_config.read() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        self.config = guard.clone();
    }

    fn handle_enabled_changed(&mut self, permissions_ready: bool) {
        if self.config.enabled {
            if permissions_ready && !self.tap_start_attempted {
                self.start_tap_thread();
            } else if !permissions_ready {
                self.tap_error = Some(
                    "permissions are still missing; grant them, then Auto Reverse \
                     will retry automatically"
                        .to_string(),
                );
            }
        } else {
            if !self.tap_thread_running {
                self.tap_start_attempted = false;
            }
            self.tap_error = None;
        }
    }
}

impl eframe::App for SettingsApp {
    // `logic` runs before `ui` on every tick, AND (per its own doc comment)
    // continues to run while the window is hidden as long as something
    // calls `Context::request_repaint` - which is exactly the case here:
    // the window can be hidden (via CancelClose+Visible(false) below) while
    // the tray icon still needs its menu polled, so tray handling and the
    // close-intercept both live here rather than in `ui`, which egui only
    // calls for a visible viewport.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Build the tray icon once on the main thread eframe already
        // drives; NSStatusItem/AppKit has the same constraint as eframe's
        // own window. Doing this here (first tick) rather than in `load()`
        // keeps all AppKit-touching setup on the thread eframe guarantees
        // is the right one.
        if self.tray.is_none() {
            let store_for_tray = self.store.clone();
            match tray::build(Arc::clone(&self.shared_config), move |config| {
                // Same disk-save path SettingsApp::save() uses - see that
                // function's doc comment. A tray-driven toggle must not
                // diverge from what the settings window itself persists,
                // so this reuses ConfigStore::save on the identical
                // shared_config the tray already wrote through, rather
                // than re-implementing a second write path.
                store_for_tray
                    .save(config)
                    .map_err(|error| error.to_string())
            }) {
                Ok(handle) => self.tray = Some(handle),
                Err(error) => {
                    self.tap_error = Some(match &self.tap_error {
                        Some(existing) => format!("{existing}; also: tray icon failed: {error}"),
                        None => format!("tray icon failed: {error}"),
                    })
                }
            }
        }

        if let Some(tray) = &mut self.tray {
            tray.set_status(tray::TrayStatus::from_config(&self.config));
        }

        // Same one-time, main-thread, first-tick setup as the tray icon
        // above: overrides the kAEQuitApplication Apple Event so Cmd-Q,
        // Dock quit, and AppleScript `quit` can no longer terminate the
        // process - see quit_handler's module doc comment for why this
        // cannot be done via NSApplicationDelegate instead.
        if !self.quit_handler_installed {
            quit_handler::install();
            self.quit_handler_installed = true;
        }

        // Closing the window (red button or Cmd-W) must hide, not quit -
        // only the tray's Quit action really exits the process. Without
        // canceling the close, eframe's run_and_return mode would make
        // run_native() return right here, and run_settings_window's park
        // loop would still keep the process alive, but the window itself
        // would be gone with no way to get it back except relaunching the
        // whole app - so this is still needed for a usable "hide" affordance,
        // not just for keeping the process alive.
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        }

        // Cmd-Q / Dock quit / AppleScript quit: quit_handler already
        // swallowed the kAEQuitApplication event before NSApplication ever
        // saw it (so the process was never at risk of exiting), this just
        // mirrors the window-close-to-hide UX for that same user intent.
        if quit_handler::poll_quit_requested() {
            ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        }

        match tray::poll_action() {
            Some(tray::TrayAction::OpenSettings) => {
                ctx.send_viewport_cmd(ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(ViewportCommand::Focus);
            }
            Some(tray::TrayAction::Quit) => {
                // The tap thread and its daemon_lock are torn down by the
                // OS on process exit - same reliance on
                // process-exit-releases-the-lock semantics the old
                // spawn-a-child design already had.
                std::process::exit(0);
            }
            Some(tray::TrayAction::ToggleEnabled) => {
                // The tray's menu-click handler already wrote through
                // shared_config and saved to disk (see tray::build's
                // on_disk_save closure above) - pull that change back into
                // the widget-bound self.config so the settings window (if
                // open) reflects a tray-driven toggle instead of showing
                // stale values until the next edit.
                let enabled_before = self.config.enabled;
                self.sync_config_from_shared();
                if enabled_before != self.config.enabled {
                    self.handle_enabled_changed(permissions_ready());
                }
            }
            Some(tray::TrayAction::ToggleDevice(_)) => {
                // Device quick-picks use the same shared config write path
                // as the master switch above; they do not affect tap
                // lifecycle, so only resync the window-bound copy.
                self.sync_config_from_shared();
            }
            Some(tray::TrayAction::OpenDebugConsole) => {
                self.show_debug_console = true;
            }
            Some(tray::TrayAction::SaveFailed) => {
                self.sync_config_from_shared();
                self.save_error = Some(
                    tray::take_last_save_error()
                        .unwrap_or_else(|| "tray change could not be saved".to_string()),
                );
            }
            None => {}
        }

        if self.show_debug_console {
            self.debug_console_viewport(ctx);
        }

        // The tray menu can ask for attention (Open Settings while hidden,
        // or Quit) outside of any window input event, so keep ticking
        // instead of only-on-input - this is what keeps `logic` running at
        // all while the window is hidden.
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            // Scroll instead of clipping: on a small screen or with many
            // devices the content must never silently lose its bottom
            // (Restore defaults lives there).
            egui::ScrollArea::vertical().show(ui, |ui| {
                self.panel_contents(ui);
            });
        });
    }
}

impl SettingsApp {
    /// Restructured into three tabs (handoff "1b"): General, Devices,
    /// Permissions. Status and the master "Reverse scrolling" toggle are
    /// pinned above the tab strip; Config path + Restore defaults are
    /// pinned below - both per the handoff's own design note that these
    /// are cross-cutting and should stay visible regardless of which tab
    /// is open. Every behavior below (auto-save on change, tap-start/retry,
    /// login_item_row, permissions_panel, device_rules editing) is
    /// unchanged from the single-panel layout this replaces - only the
    /// layout changed.
    fn panel_contents(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = 8.0;
        let mut changed = false;

        status_header(ui, &self.config);
        ui.add_space(4.0);

        // The single most-used control, directly under the status - stays
        // above the tabs on every tab.
        let enabled_before = self.config.enabled;
        changed |= styled_checkbox(
            ui,
            &mut self.config.enabled,
            RichText::new("Reverse scrolling").size(14.0).strong(),
            18.0,
            5.0,
        )
        .changed();
        let enabled_changed = enabled_before != self.config.enabled;

        // Independent of which tab is rendered below: granting permissions
        // (or the config becoming enabled) must auto-start the tap on the
        // very next tick even if the user is looking at General or
        // Devices, not only when the Permissions tab happens to be open.
        let permissions_ready = permissions_ready();
        if self.config.enabled && permissions_ready && !self.tap_start_attempted {
            self.start_tap_thread();
        }

        // Pinned above the tabs, not inside the Permissions tab's content:
        // in the single-panel layout this replaced, a failed tap start was
        // always visible regardless of scroll position. Tab-scoping it
        // would silently hide "scroll reversal could not start" from a user
        // looking at General or Devices - the exact kind of error this
        // project's honesty rules (see module doc comment) require staying
        // visible, not buried behind a tab click.
        if let Some(error) = &self.tap_error {
            ui.label(
                RichText::new(format!("Scroll reversal could not start: {error}"))
                    .color(Color32::from_rgb(0xC0, 0x39, 0x2B))
                    .small(),
            );
            // No mockup example for this error/retry state - styled as the
            // same small bordered-chip button as "Refresh devices"/"Turn
            // off" (padding:5px 12px), the closest handoff analog.
            if permissions_ready
                && self.config.enabled
                && styled_button(ui, "Retry starting scroll reversal", egui::vec2(12.0, 5.0))
                    .clicked()
            {
                self.start_tap_thread();
            }
        }
        if !permissions_ready && self.config.enabled {
            ui.horizontal(|ui| {
                ui.label("Scroll reversal");
                ui.label(
                    RichText::new("waiting for permissions")
                        .color(Color32::from_rgb(0xE5, 0x9E, 0x2F)),
                );
            });
        }

        ui.add_space(8.0);
        ui.separator();

        tab_strip(ui, &mut self.selected_tab);

        match self.selected_tab {
            SettingsTab::General => {
                section(ui, "What gets reversed");
                ui.add_enabled_ui(self.config.enabled, |ui| {
                    changed |= styled_checkbox(
                        ui,
                        &mut self.config.reverse_mouse,
                        RichText::new("Mouse wheel").size(13.0),
                        16.0,
                        4.0,
                    )
                    .changed();
                    changed |= styled_checkbox(
                        ui,
                        &mut self.config.reverse_trackpad,
                        RichText::new("Trackpad (includes Magic Mouse)").size(13.0),
                        16.0,
                        4.0,
                    )
                    .changed();
                });

                section(ui, "Directions");
                ui.add_enabled_ui(self.config.enabled, |ui| {
                    ui.horizontal(|ui| {
                        changed |= styled_checkbox(
                            ui,
                            &mut self.config.reverse_vertical,
                            RichText::new("Vertical").size(13.0),
                            16.0,
                            4.0,
                        )
                        .changed();
                        ui.add_space(20.0);
                        changed |= styled_checkbox(
                            ui,
                            &mut self.config.reverse_horizontal,
                            RichText::new("Horizontal").size(13.0),
                            16.0,
                            4.0,
                        )
                        .changed();
                    });
                });

                section(ui, "Wheel step size");
                ui.add_enabled_ui(self.config.enabled && self.config.reverse_mouse, |ui| {
                    ui.horizontal(|ui| {
                        // handoff: `gap:12px` between the track and the value
                        // column, which itself has `min-width:14px` - set the
                        // gap explicitly (rather than relying on egui's
                        // unrelated default item_spacing) so the reserved
                        // width below is exact, not a rough estimate: 12 + 14
                        // = 26, not the 32 a prior pass used.
                        ui.spacing_mut().item_spacing.x = 12.0;
                        let slider_width = (ui.available_width() - 26.0).max(40.0);
                        changed |= styled_step_slider(
                            ui,
                            &mut self.config.discrete_scroll_step_size,
                            0,
                            20,
                            slider_width,
                        )
                        .changed();
                        ui.label(
                            RichText::new(self.config.discrete_scroll_step_size.to_string())
                                .small(),
                        );
                    });
                    ui.label(
                        RichText::new("Lines per wheel notch. 0 keeps the system speed.")
                            .small()
                            .weak(),
                    );
                });
            }
            SettingsTab::Devices => {
                section(ui, "Per-device rules");
                let (rules_changed, wants_refresh) =
                    device_rules(ui, &self.devices, &mut self.config);
                changed |= rules_changed;
                if wants_refresh {
                    self.refresh_devices();
                }
                if let Some(error) = &self.devices_error {
                    ui.label(
                        RichText::new(format!("Device list unavailable: {error}"))
                            .color(Color32::from_rgb(0xC0, 0x39, 0x2B))
                            .small(),
                    );
                }
            }
            SettingsTab::Permissions => {
                section(ui, "Permissions");
                permissions_panel(ui);
                // Scroll-reversal error/waiting notices are pinned above
                // the tabs now (see panel_contents), not repeated here.
                ui.add_space(8.0);
                ui.separator();
                section(ui, "Start at login");
                self.login_item_row(ui);
            }
        }

        ui.add_space(8.0);
        ui.separator();
        footer(ui, &self.store, &self.load_error, &self.save_error);

        // Handoff "1b": "padding:6px 14px".
        if styled_button(ui, "Restore defaults", egui::vec2(14.0, 6.0)).clicked() {
            self.config = AppConfig::default();
            changed = true;
        }

        if changed {
            self.save();
            // Note: there is no in-process "stop the tap thread" action
            // for the disable-only branch - the background thread reads
            // the shared config on every event and simply passes events
            // through unmodified when `enabled` is false (the same
            // pass-through behavior `scroll::transform_event` already
            // implements), so disabling here takes effect immediately
            // without needing to tear down the thread.
            if self.save_error.is_none() && enabled_changed {
                self.handle_enabled_changed(permissions_ready);
            }
        }
    }

    /// Renders the Debug Console (handoff "1f") as a second native viewport.
    ///
    /// Uses `show_viewport_immediate` rather than `show_viewport_deferred`:
    /// the console's entire state (`DebugConsoleState`) lives on
    /// `SettingsApp` itself, which is neither `Send` nor `Sync` (it owns
    /// AppKit `Retained<...>` tray objects), so the `deferred` API's
    /// `Fn(&mut Ui, ViewportClass) + Send + Sync + 'static` callback bound
    /// cannot borrow it directly. `immediate`'s `FnMut` callback has no such
    /// bound and runs synchronously, in-line, once per `logic` tick - the
    /// same 250ms cadence the tray/window already repaint at, so the "both
    /// viewports repaint together" cost `show_viewport_immediate` warns
    /// about is not a real regression here. Called every tick that
    /// `show_debug_console` is true, exactly like egui's viewport docs
    /// require ("call this each pass when the child viewport should exist").
    fn debug_console_viewport(&mut self, ctx: &egui::Context) {
        let viewport_id = egui::ViewportId::from_hash_of("auto-reverse-debug-console");
        let builder = egui::ViewportBuilder::default()
            .with_title("Debug Console — Auto Reverse")
            .with_inner_size([640.0, 480.0])
            .with_min_inner_size([480.0, 320.0]);

        // Borrow-split: the closure below needs `&mut self.debug_console`
        // but must not also capture `self.show_debug_console` mutably at
        // the same time as `self` is borrowed for the outer call, so the
        // "close requested" signal is read back via a local and applied
        // after `show_viewport_immediate` returns instead of from within
        // the closure.
        let mut close_requested = false;
        let debug_console = &mut self.debug_console;

        ctx.show_viewport_immediate(viewport_id, builder, |ctx, class| {
            if class == egui::ViewportClass::EmbeddedWindow {
                // This egui backend cannot open a second native window;
                // still render the content so the feature degrades to an
                // in-window panel rather than silently doing nothing.
            }

            if ctx.input(|i| i.viewport().close_requested()) {
                close_requested = true;
            }

            egui::CentralPanel::default().show(ctx, |ui| {
                debug_console_contents(ui, debug_console);
            });
        });

        if close_requested {
            self.show_debug_console = false;
        }
    }

    fn login_item_row(&mut self, ui: &mut egui::Ui) {
        let status = login_item::status();
        ui.horizontal(|ui| {
            ui.label("Auto Reverse.app at login");
            ui.label(RichText::new(status.summary()).small());
        });
        ui.horizontal(|ui| match status {
            login_item::LoginItemStatus::Enabled
            | login_item::LoginItemStatus::RequiresApproval => {
                // Handoff "1b" Permissions tab: "padding:5px 12px".
                if styled_button(ui, "Turn off", egui::vec2(12.0, 5.0)).clicked() {
                    self.login_item_error = login_item::unregister().err();
                }
            }
            login_item::LoginItemStatus::NotRegistered | login_item::LoginItemStatus::NotFound => {
                if styled_button(ui, "Turn on", egui::vec2(12.0, 5.0)).clicked() {
                    self.login_item_error = login_item::register().err();
                }
            }
        });
        if let Some(error) = &self.login_item_error {
            ui.label(
                RichText::new(format!("Start at login failed: {error}"))
                    .color(Color32::from_rgb(0xC0, 0x39, 0x2B))
                    .small(),
            );
        }
        ui.label(
            RichText::new(
                "Separate from the CLI's enable-startup command - this registers the app \
                 bundle itself with macOS.",
            )
            .small()
            .weak(),
        );
    }
}

fn status_header(ui: &mut egui::Ui, config: &AppConfig) {
    let accessibility = permissions::has_accessibility_trust();
    let input_monitoring = permissions::has_input_monitoring_access();

    let (dot_color, status_word) = if !config.enabled {
        (Color32::GRAY, "OFF")
    } else if !accessibility || !input_monitoring {
        (Color32::from_rgb(0xE5, 0x9E, 0x2F), "NEEDS PERMISSION")
    } else {
        (Color32::from_rgb(0x34, 0xA8, 0x53), "ON")
    };

    ui.horizontal(|ui| {
        status_dot(ui, dot_color, 6.0, 16.0);
        ui.label(RichText::new(status_word).size(18.0).strong());
    });
    ui.label(RichText::new(config.plain_english_summary()).weak());
}

fn status_dot(ui: &mut egui::Ui, color: Color32, radius: f32, size: f32) {
    // A painted circle, not the "●" glyph: egui's default font can render
    // that codepoint inconsistently, which reads as a broken status icon.
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), radius, color);
}

/// The segmented-control tab strip (handoff "1b"): a light-gray track with
/// the active segment drawn as a raised, contrasting pill. egui has no
/// built-in tab widget, so this is built from `ui.horizontal` plus manually
/// painted rects - the exact colors below are read from the handoff's id="1b"
/// section (`background:#E3E3E7` track / `#fff` active segment with a
/// `box-shadow` in light mode, `#2C2C2E` track / `#48484A` active segment in
/// dark mode). `ui.visuals().dark_mode` is what already drives this
/// project's system-native light/dark following elsewhere, so the same
/// check picks the right pair here instead of introducing a separate theme
/// mechanism.
fn tab_strip(ui: &mut egui::Ui, selected: &mut SettingsTab) {
    let dark = ui.visuals().dark_mode;
    let track_color = if dark {
        Color32::from_rgb(0x2C, 0x2C, 0x2E)
    } else {
        Color32::from_rgb(0xE3, 0xE3, 0xE7)
    };
    let active_color = if dark {
        Color32::from_rgb(0x48, 0x48, 0x4A)
    } else {
        Color32::WHITE
    };
    let active_text = if dark {
        Color32::from_rgb(0xF2, 0xF2, 0xF3)
    } else {
        Color32::from_rgb(0x1D, 0x1D, 0x1F)
    };
    let inactive_text = if dark {
        Color32::from_rgb(0x9A, 0x9A, 0xA0)
    } else {
        Color32::from_rgb(0x6E, 0x6E, 0x73)
    };

    egui::Frame::new()
        .fill(track_color)
        .corner_radius(8.0)
        .inner_margin(2.0)
        .show(ui, |ui| {
            let total_width = ui.available_width();
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let segment_width = (total_width - 4.0) / 3.0;
                for (tab, label) in [
                    (SettingsTab::General, "General"),
                    (SettingsTab::Devices, "Devices"),
                    (SettingsTab::Permissions, "Permissions"),
                ] {
                    let is_active = *selected == tab;
                    let (color, text_color) = if is_active {
                        (active_color, active_text)
                    } else {
                        (Color32::TRANSPARENT, inactive_text)
                    };
                    let button = egui::Button::new(
                        RichText::new(label).size(12.0).strong().color(text_color),
                    )
                    .fill(color)
                    .corner_radius(6.0)
                    .min_size(egui::vec2(segment_width, 24.0));
                    if ui.add(button).clicked() {
                        *selected = tab;
                    }
                }
            });
        });
}

// ---------------------------------------------------------------------
// Design tokens (handoff "1b"/"1f") shared by the custom-painted controls
// below - the checkbox, the wheel-step slider, the bordered-chip buttons,
// and the device-rule dropdown chip all read from these instead of each
// re-deriving the same light/dark pairs inline. `tab_strip` above and
// `debug_filter_strip` below predate this pass and already hardcode their
// own copies of some of the same hex values inline - left as-is rather than
// refactored onto these tokens, since both are explicitly out of scope
// ("already correctly styled, leave that alone, do not touch working code").
// ---------------------------------------------------------------------

/// Accent blue: `#2F6FE4` light / `#5B93FF` dark. Drives the checked
/// checkbox fill, the slider's filled track portion, and the device-rule
/// "Reverse" chip's border/text/arrow.
fn accent_color(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0x5B, 0x93, 0xFF)
    } else {
        Color32::from_rgb(0x2F, 0x6F, 0xE4)
    }
}

/// Color painted ON TOP of `accent_color` (the checkmark glyph inside a
/// checked checkbox): white in light mode, near-black `#1E1E1F` in dark
/// mode per the handoff. Deliberately NOT the same as `primary_text_color`.
fn accent_glyph_color(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0x1E, 0x1E, 0x1F)
    } else {
        Color32::WHITE
    }
}

/// Neutral border: `#C7C7CC` light / `#48484A` dark. Used by the unchecked
/// checkbox border, every bordered-chip button, the slider knob's border,
/// and the device-rule chip's "Default"/"Don't reverse" border.
fn control_border_color(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0x48, 0x48, 0x4A)
    } else {
        Color32::from_rgb(0xC7, 0xC7, 0xCC)
    }
}

/// Neutral surface: `#fff` light / `#2C2C2E` dark. Used by the unchecked
/// checkbox background, every bordered-chip button's background, and the
/// device-rule chip's "Default"/"Don't reverse" background.
fn control_surface_color(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0x2C, 0x2C, 0x2E)
    } else {
        Color32::WHITE
    }
}

/// Primary label text: `#1D1D1F` light / `#F2F2F3` dark. Used by button
/// labels and the device-rule chip's "Default"/"Reverse" text. (Checkbox
/// labels intentionally use `ui.visuals().text_color()` instead - see
/// `styled_checkbox`'s doc comment for why.)
fn primary_text_color(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0xF2, 0xF2, 0xF3)
    } else {
        Color32::from_rgb(0x1D, 0x1D, 0x1F)
    }
}

/// Muted secondary tone: `#9A9AA0`, used as-is in both themes per the
/// handoff (the same value `tab_strip` above already uses for its own dark
/// `inactive_text`). Used here for the device-rule chip's "Default" dropdown
/// arrow, which the handoff draws in a different, dimmer color than the
/// chip's own text.
fn muted_glyph_color() -> Color32 {
    Color32::from_rgb(0x9A, 0x9A, 0xA0)
}

/// The device-rule chip's "Reverse" background in dark mode. NOT shown
/// anywhere in the handoff - it depicts the light-mode Devices tab and the
/// dark-mode General tab, but never a dark-mode Devices tab, so there is no
/// literal value to read. Extrapolated as `accent_color(true)` blended 20%
/// over `control_surface_color(true)` (`#2C2C2E`), mirroring the light
/// mode's own relationship (`#EEF3FE` is accent blue tinted into white).
/// Computed once as a fixed, opaque hex rather than left as a translucent
/// overlay, so it reads the same regardless of what's behind it.
fn reverse_chip_dark_bg() -> Color32 {
    Color32::from_rgb(0x35, 0x41, 0x58)
}

/// A custom-painted checkbox matching the handoff's id="1b" exactly - egui's
/// built-in `Checkbox` widget has a different shape/corner-radius/color
/// than the mockup. Same low-level `ui.allocate_exact_size` + `ui.painter()`
/// pattern `status_dot` above already uses, not a different one. One
/// helper serves both sizes the mockup uses: the master "Reverse scrolling"
/// toggle (18x18, 5px corners) and the four sub-checkboxes (16x16, 4px
/// corners) - callers pass `box_size`/`corner_radius` accordingly.
///
/// The whole row (box + label) is one clickable region, matching
/// `ui.checkbox`'s own behavior where clicking the label also toggles it:
/// the box and label are laid out first with `Sense::hover()` each (so
/// neither reacts to a click on its own), then a single `ui.interact` over
/// their combined bounding rect drives the actual toggle - exactly one
/// toggle per click, not a double-count from two overlapping click senses.
///
/// Checkbox labels deliberately use `ui.visuals().text_color()` (the
/// current theme's ordinary text color) rather than the handoff's literal
/// `#1D1D1F`/`#F2F2F3` hex - every other untouched label in this file
/// already renders through the theme's default text color rather than a
/// hardcoded one, so hardcoding it only for checkbox labels would make them
/// subtly mismatch their neighboring labels instead of matching the mockup
/// more closely.
///
/// Preserves `ui.checkbox`'s exact behavior: toggles `*checked`, returns a
/// `Response` whose `.changed()` is set exactly when the value actually
/// flipped (so existing `changed |= ....changed()` call sites don't need to
/// change), and respects `ui.add_enabled_ui`'s disabled/greyed-out state
/// automatically - disabling a `Ui` multiplies its `Painter`'s opacity (see
/// `Ui::disable`), which applies to raw `ui.painter()` calls exactly like it
/// does to built-in widgets, and also disables `Sense`d interaction on that
/// `Ui` (see `Ui::interact`'s use of `self.enabled`), so no extra plumbing
/// is needed here for either.
fn styled_checkbox(
    ui: &mut egui::Ui,
    checked: &mut bool,
    label: impl Into<RichText>,
    box_size: f32,
    corner_radius: f32,
) -> egui::Response {
    let label: RichText = label.into();
    let dark = ui.visuals().dark_mode;
    let fill = accent_color(dark);
    let glyph_color = accent_glyph_color(dark);
    let border_color = control_border_color(dark);
    let unchecked_bg = control_surface_color(dark);
    let text_color = ui.visuals().text_color();

    let id = ui.make_persistent_id(("styled_checkbox", label.text().to_owned()));

    let row = ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0; // handoff "gap:10px"
        let (box_rect, _) =
            ui.allocate_exact_size(egui::vec2(box_size, box_size), egui::Sense::hover());
        if ui.is_rect_visible(box_rect) {
            let painter = ui.painter();
            if *checked {
                painter.rect_filled(box_rect, corner_radius, fill);
                // Exact handoff checkmark sizes, not a ratio of box_size:
                // 12px for the 18px master toggle, 11px for the 16px
                // sub-checkboxes - a 0.62 ratio previously used here landed
                // at 11.16px/9.92px, neither an exact match.
                let checkmark_size = if box_size >= 18.0 { 12.0 } else { 11.0 };
                painter.text(
                    box_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "✓",
                    egui::FontId::proportional(checkmark_size),
                    glyph_color,
                );
            } else {
                painter.rect_filled(box_rect, corner_radius, unchecked_bg);
                painter.rect_stroke(
                    box_rect,
                    corner_radius,
                    egui::Stroke::new(1.5, border_color),
                    egui::StrokeKind::Inside,
                );
            }
        }
        // `selectable(false)`: a plain `ui.label()` senses click-and-drag
        // (egui's default `interaction.selectable_labels = true`), which
        // competes with the row's own outer click sense below - a click
        // that drifts a few points while over the label text lands on the
        // label's text-selection drag instead of the checkbox toggle,
        // silently skipping it. Not selectable, this label only senses
        // hover, so the outer `ui.interact` below is the sole click target.
        ui.add(egui::Label::new(label.color(text_color)).selectable(false));
    });

    let mut response = ui.interact(row.response.rect, id, egui::Sense::click());
    if response.clicked() {
        *checked = !*checked;
        response.mark_changed();
    }
    response
}

/// Custom-painted equivalent of `egui::Slider::new(value, min..=max)`, used
/// for "Wheel step size" (handoff "1b"): a 4px track, a blue fill portion
/// sized to the current value's proportion of the range, and a 14px round
/// knob with a border and a subtle drop shadow - matching the mockup's
/// slider chrome instead of egui's own default slider widget. Same
/// `ui.allocate_exact_size` + `ui.painter()` + drag-response pattern as this
/// file's other custom-painted controls; same value semantics as the
/// `egui::Slider` it replaces (drag or click along the track to change the
/// value, result clamped to `min..=max`, `Response::changed()` set exactly
/// when the value actually moved).
///
/// Takes an explicit `width` (rather than grabbing `ui.available_width()`
/// internally) because, per the handoff, the trailing numeric value label
/// shares the same row and needs room reserved for it - the caller computes
/// `width` accordingly (see the "Wheel step size" call site).
fn styled_step_slider(
    ui: &mut egui::Ui,
    value: &mut i64,
    min: i64,
    max: i64,
    width: f32,
) -> egui::Response {
    let dark = ui.visuals().dark_mode;
    let track_bg = if dark {
        Color32::from_rgb(0x2C, 0x2C, 0x2E)
    } else {
        Color32::from_rgb(0xE3, 0xE3, 0xE7)
    };
    let fill_color = accent_color(dark);
    let knob_fill = if dark {
        Color32::from_rgb(0xE8, 0xE8, 0xE9)
    } else {
        Color32::WHITE
    };
    let knob_border = control_border_color(dark);
    // box-shadow alpha: 0.4 dark / 0.15 light, per the handoff.
    let knob_shadow = if dark {
        Color32::from_black_alpha(0x66)
    } else {
        Color32::from_black_alpha(0x26)
    };

    let desired_size = egui::vec2(width, 20.0);
    let (rect, mut response) = ui.allocate_exact_size(desired_size, egui::Sense::click_and_drag());

    let span = (max - min).max(1) as f32;
    if (response.dragged() || response.clicked())
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let t = ((pointer.x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0);
        let new_value = (min + (t * span).round() as i64).clamp(min, max);
        if new_value != *value {
            *value = new_value;
            response.mark_changed();
        }
    }

    if ui.is_rect_visible(rect) {
        let track_rect = egui::Rect::from_center_size(rect.center(), egui::vec2(rect.width(), 4.0));
        let painter = ui.painter();
        painter.rect_filled(track_rect, 2.0, track_bg);

        let t = ((*value - min) as f32 / span).clamp(0.0, 1.0);
        let fill_width = track_rect.width() * t;
        if fill_width > 0.0 {
            let fill_rect = egui::Rect::from_min_size(track_rect.min, egui::vec2(fill_width, 4.0));
            painter.rect_filled(fill_rect, 2.0, fill_color);
        }

        let knob_center = egui::pos2(track_rect.left() + fill_width, rect.center().y);
        const KNOB_RADIUS: f32 = 7.0; // 14px diameter, per the handoff
        painter.circle_filled(knob_center + egui::vec2(0.0, 1.0), KNOB_RADIUS, knob_shadow);
        painter.circle_filled(knob_center, KNOB_RADIUS, knob_fill);
        painter.circle_stroke(
            knob_center,
            KNOB_RADIUS,
            egui::Stroke::new(1.0, knob_border),
        );
    }

    response
}

/// Wraps `label` in the handoff's bordered-chip button chrome
/// (`border:1px solid #C7C7CC`/`#48484A`, `border-radius:6px`,
/// `background:#fff`/`#2C2C2E`, 12px text) used for every plain button in
/// the mockup - Restore defaults, Refresh devices, Turn on/off, Retry
/// starting scroll reversal, Open Privacy & Security settings, and the
/// Debug Console's Export…/Clear. Only the padding differs between call
/// sites (see each call site's own comment for the handoff's exact value),
/// so `padding` is the one thing left for the caller to supply.
///
/// egui's `Button` has no per-instance padding knob, so this scopes
/// `ui.spacing_mut().button_padding` for the single `ui.add(...)` call via
/// `ui.scope` (which builds a genuine child `Ui` with its own cloned style,
/// so the override never leaks to sibling widgets) - the same mechanism
/// `device_rule_chip` below uses for its own scoped visuals, just touching
/// `Spacing` instead of `Visuals`.
fn styled_button(ui: &mut egui::Ui, label: &str, padding: egui::Vec2) -> egui::Response {
    let dark = ui.visuals().dark_mode;
    let bg = control_surface_color(dark);
    let border = control_border_color(dark);
    let text_color = primary_text_color(dark);

    ui.scope(|ui| {
        ui.spacing_mut().button_padding = padding;
        ui.add(
            egui::Button::new(RichText::new(label).size(12.0).color(text_color))
                .fill(bg)
                .stroke(egui::Stroke::new(1.0, border))
                .corner_radius(6.0),
        )
    })
    .inner
}

/// The per-device Default/Reverse/Don't-reverse picker (handoff "1b"),
/// rendered as a bordered pill/chip rather than a plain `ComboBox`. Still
/// backed by a real `egui::ComboBox` for the popup mechanics (open/close,
/// keyboard nav, `selectable_value`) - only the CLOSED button's appearance
/// is restyled, via three stock `ComboBox` extension points, rather than a
/// hand-rolled clickable-chip-plus-popup:
/// - `ui.scope` + `ui.visuals_mut().widgets.{inactive,hovered,active,open}`
///   to recolor the chip's own fill/border/corner-radius;
/// - `.selected_text(RichText::new(...).color(...))` to color the text
///   independent of those visuals (a `RichText`'s own explicit color wins
///   over a widget-visuals fallback color - see `Painter::galley`'s doc
///   comment on `fallback_color`);
/// - `.icon(...)` to paint the "▾" glyph in its own color, since the
///   handoff's "Default" state uses a DIFFERENT color for the arrow
///   (`#9A9AA0`) than for the text (`#1D1D1F`) - a single overridden
///   `fg_stroke` could not represent that, since `ComboBox` uses it for
///   both the text and (by default) the arrow.
///
/// This was chosen over a fully custom chip with a hand-rolled popup
/// because all three hooks above are already stock, documented `ComboBox`
/// API - reusing its real popup mechanics is less code and less risk than
/// reimplementing click-to-open/selection/keyboard-nav for a three-item
/// menu from scratch.
///
/// Colors: "Default" and "Reverse" match the handoff's id="1b" Devices tab
/// exactly in light mode (dark mode's "Reverse" background is an
/// extrapolation - see `reverse_chip_dark_bg`'s doc comment). "Don't
/// reverse" has NO mockup example in either theme anywhere in the handoff -
/// it renders identically to "Default" (neutral chip) here rather than
/// inventing an unspecified third accent color the design never showed.
///
/// Preserves the exact `Option<bool>` semantics the caller already relies
/// on (`None` = Default, `Some(true)` = Reverse, `Some(false)` = Don't
/// reverse): this function only paints and edits `*selection` via the
/// popup's `selectable_value` calls, identically to the `egui::ComboBox` it
/// replaces. `device_rules` still does its own `retain`/`push` afterward
/// based on whether `*selection` differs from the value it had going in -
/// unchanged by this function.
fn device_rule_chip(
    ui: &mut egui::Ui,
    id_salt: impl egui::AsIdSalt,
    selection: &mut Option<bool>,
) -> egui::Response {
    let dark = ui.visuals().dark_mode;
    let is_reverse = *selection == Some(true);

    let bg = if is_reverse {
        if dark {
            reverse_chip_dark_bg()
        } else {
            Color32::from_rgb(0xEE, 0xF3, 0xFE)
        }
    } else {
        control_surface_color(dark)
    };
    let border = if is_reverse {
        accent_color(dark)
    } else {
        control_border_color(dark)
    };
    let text_color = if is_reverse {
        accent_color(dark)
    } else {
        primary_text_color(dark)
    };
    let arrow_color = if is_reverse {
        accent_color(dark)
    } else {
        muted_glyph_color()
    };

    let label_text = match *selection {
        None => "Default",
        Some(true) => "Reverse",
        Some(false) => "Don't reverse",
    };

    ui.scope(|ui| {
        ui.spacing_mut().button_padding = egui::vec2(8.0, 4.0); // handoff "padding:4px 8px"
        ui.spacing_mut().icon_spacing = 6.0; // handoff "gap:6px"

        {
            // The mockup only depicts the resting state, but a fixed look
            // across inactive/hovered/active/open (a prior pass) is a real
            // interaction-feedback regression versus both egui's own
            // default ComboBox and every other control in this window -
            // hovering or pressing the chip must not look identical to
            // resting. Tint the background towards white (dark mode) or
            // black (light mode) by a small amount, more for
            // active/open than hovered, and thicken the border to match.
            let towards = if dark { Color32::WHITE } else { Color32::BLACK };
            let widgets = &mut ui.visuals_mut().widgets;
            widgets.inactive.weak_bg_fill = bg;
            widgets.inactive.bg_stroke = egui::Stroke::new(1.0, border);
            widgets.inactive.corner_radius = 6.0.into();

            widgets.hovered.weak_bg_fill = bg.lerp_to_gamma(towards, 0.06);
            widgets.hovered.bg_stroke = egui::Stroke::new(1.0, border);
            widgets.hovered.corner_radius = 6.0.into();

            for visuals in [&mut widgets.active, &mut widgets.open] {
                visuals.weak_bg_fill = bg.lerp_to_gamma(towards, 0.12);
                visuals.bg_stroke = egui::Stroke::new(1.5, border);
                visuals.corner_radius = 6.0.into();
            }
        }

        egui::ComboBox::from_id_salt(id_salt)
            .selected_text(RichText::new(label_text).size(12.0).color(text_color))
            .icon(
                move |ui: &egui::Ui,
                      rect: egui::Rect,
                      _visuals: &egui::style::WidgetVisuals,
                      _is_open: bool| {
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "▾",
                        // handoff: the arrow <span> has no font-size of its
                        // own, so it inherits the chip's own 12px, not a
                        // smaller glyph.
                        egui::FontId::proportional(12.0),
                        arrow_color,
                    );
                },
            )
            .show_ui(ui, |ui| {
                ui.selectable_value(selection, None, "Default");
                ui.selectable_value(selection, Some(true), "Reverse");
                ui.selectable_value(selection, Some(false), "Don't reverse");
            })
            .response
    })
    .inner
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(8.0);
    let color = if ui.visuals().dark_mode {
        Color32::from_rgb(0x7C, 0x7C, 0x82)
    } else {
        Color32::from_rgb(0x8E, 0x8E, 0x93)
    };
    ui.label(
        RichText::new(title.to_uppercase())
            .size(11.0)
            .strong()
            .color(color),
    );
}

/// Per-device rows: each connected pointing device gets a
/// Default / Reverse / Don't reverse choice that edits `device_rules`.
/// Returns (config changed, user asked to refresh the device list).
fn device_rules(
    ui: &mut egui::Ui,
    devices: &[hid::DeviceInfo],
    config: &mut AppConfig,
) -> (bool, bool) {
    let mut changed = false;
    let mut wants_refresh = false;

    if devices.is_empty() {
        ui.label(RichText::new("No pointing devices detected.").weak());
    }

    for device in devices {
        let current = config
            .device_rules
            .iter()
            .find(|rule| rule.matches(device.hardware))
            .map(|rule| rule.reverse);

        let label = device.name.clone().unwrap_or_else(|| "Unnamed".to_string());
        let hardware = format!(
            "{:04x}:{:04x}",
            device.hardware.vendor_id, device.hardware.product_id
        );
        let mut selection = current;
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(&label);
                ui.label(RichText::new(&hardware).small().monospace().weak());
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                device_rule_chip(
                    ui,
                    (device.hardware.vendor_id, device.hardware.product_id),
                    &mut selection,
                );
            });
        });
        if selection != current {
            config
                .device_rules
                .retain(|rule| !rule.matches(device.hardware));
            if let Some(reverse) = selection {
                config.device_rules.push(DeviceRule {
                    vendor_id: device.hardware.vendor_id,
                    product_id: device.hardware.product_id,
                    name: device.name.clone(),
                    reverse,
                });
            }
            changed = true;
        }
    }

    ui.horizontal(|ui| {
        // Handoff "1b" Devices tab: "padding:5px 12px".
        if styled_button(ui, "Refresh devices", egui::vec2(12.0, 5.0)).clicked() {
            wants_refresh = true;
        }
        ui.label(
            RichText::new("Rules apply to clicky wheels only, not trackpad-style scrolling.")
                .small()
                .weak(),
        );
    });

    (changed, wants_refresh)
}

/// Starts the CGEventTap on a background thread in this same process.
/// `install_and_run` itself acquires the exclusive `daemon_lock` as its
/// first step (see `platform::macos::event_tap`), so a redundant call here
/// (this window opened twice, or a headless `run`/LaunchAgent also active)
/// just costs an idle thread that returns immediately rather than risking a
/// second live CGEventTap. Never blocks the GUI thread: `install_and_run`
/// runs its own forever-blocking CFRunLoop entirely on the spawned thread.
enum TapStartOutcome {
    Running,
    StoppedImmediately,
    Failed(String),
}

fn spawn_tap_thread(shared_config: Arc<RwLock<AppConfig>>) -> TapStartOutcome {
    let (result_tx, result_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        // install_and_run only returns once it definitively fails to start
        // (lock contention, permission failure, or the CFRunLoop stops
        // some other way) - there is no ongoing "still starting" state to
        // report back beyond that first outcome, so a bounded channel send
        // of the terminal Result is enough; this thread lives for as long
        // as the tap does after that.
        let outcome = event_tap::install_and_run(shared_config);
        let _ = result_tx.send(outcome);
    });

    // A real permission/lock failure surfaces near-instantly (before the
    // CFRunLoop would ever start); a successful install blocks forever, so
    // waiting briefly distinguishes "definitely failed" from "presumably
    // running" without blocking the GUI thread for long in the failure
    // case and without blocking it at all in the success case beyond this
    // short window.
    match result_rx.recv_timeout(std::time::Duration::from_millis(200)) {
        Ok(Ok(())) => TapStartOutcome::StoppedImmediately,
        Ok(Err(error)) => TapStartOutcome::Failed(error.to_string()),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => TapStartOutcome::Running,
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => TapStartOutcome::StoppedImmediately,
    }
}

/// Pure check, independent of any widget - both permissions granted. Used
/// by `panel_contents` to drive the tap-start/retry logic on every tick
/// regardless of which tab (if any) is currently rendering the Permissions
/// section, so granting permissions while on the General or Devices tab
/// still auto-starts scroll reversal instead of requiring a visit to
/// Permissions first.
fn permissions_ready() -> bool {
    permissions::has_accessibility_trust() && permissions::has_input_monitoring_access()
}

fn permissions_panel(ui: &mut egui::Ui) -> bool {
    let rows = [
        ("Accessibility", permissions::has_accessibility_trust()),
        (
            "Input Monitoring",
            permissions::has_input_monitoring_access(),
        ),
    ];
    let mut any_missing = false;
    for (name, granted) in rows {
        ui.horizontal(|ui| {
            ui.label(name);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if granted {
                    ui.label(
                        RichText::new("Granted")
                            .color(Color32::from_rgb(0x34, 0xA8, 0x53))
                            .strong(),
                    );
                } else {
                    any_missing = true;
                    ui.label(
                        RichText::new("Required")
                            .color(Color32::from_rgb(0xE5, 0x9E, 0x2F))
                            .strong(),
                    );
                }
            });
        });
    }
    if any_missing {
        ui.label(
            RichText::new(
                "Scroll reversal cannot run without both. Add Auto Reverse.app in System Settings.",
            )
            .small()
            .weak(),
        );
        // No mockup example for the missing-permissions state (the handoff's
        // Permissions tab only shows both rows already Granted) - styled as
        // the same small bordered-chip button used elsewhere (padding:5px
        // 12px), the closest handoff analog.
        if styled_button(
            ui,
            "Open Privacy & Security settings",
            egui::vec2(12.0, 5.0),
        )
        .clicked()
        {
            let _ = std::process::Command::new("open")
                .arg("x-apple.systempreferences:com.apple.preference.security?Privacy")
                .spawn();
        }
    }
    !any_missing
}

fn footer(
    ui: &mut egui::Ui,
    store: &ConfigStore,
    load_error: &Option<String>,
    save_error: &Option<String>,
) {
    if let Some(error) = load_error {
        ui.label(
            RichText::new(format!(
                "Config could not be loaded, using defaults: {error}"
            ))
            .color(Color32::from_rgb(0xC0, 0x39, 0x2B))
            .small(),
        );
    }
    if let Some(error) = save_error {
        ui.label(
            RichText::new(format!("Saving failed: {error}"))
                .color(Color32::from_rgb(0xC0, 0x39, 0x2B))
                .small(),
        );
    }
    ui.label(
        RichText::new(format!("Config: {}", store.path().display()))
            .small()
            .monospace()
            .weak(),
    );
}

/// Debug Console body (handoff "1f"): filter tabs, search box, Live
/// indicator, Export/Clear, the event table, and the footer line. Reads a
/// fresh `debug_log::snapshot()` every call - the console has no
/// pause/resume, it always reflects the buffer's current contents, matching
/// the handoff's "Live" indicator (no separate paused state to render).
fn debug_console_contents(ui: &mut egui::Ui, state: &mut DebugConsoleState) {
    let all_events = debug_log::snapshot();

    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut state.search)
                .hint_text("Filter events…")
                .desired_width(150.0),
        );

        ui.add_space(8.0);
        debug_filter_strip(ui, &mut state.filter);

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Handoff "1f": "padding:5px 10px".
            if styled_button(ui, "Clear", egui::vec2(10.0, 5.0)).clicked() {
                debug_log::clear();
            }
            if styled_button(ui, "Export…", egui::vec2(10.0, 5.0)).clicked() {
                match export_debug_events(&filtered_events(&all_events, state)) {
                    Ok(path) => {
                        state.export_success = Some(format!("Exported to {}", path.display()));
                        state.export_error = None;
                    }
                    Err(error) => {
                        state.export_error = Some(error);
                        state.export_success = None;
                    }
                }
            }
            ui.add_space(6.0);
            status_dot(ui, Color32::from_rgb(0x34, 0xA8, 0x53), 3.0, 8.0);
            ui.label(
                RichText::new("Live")
                    .color(Color32::from_rgb(0x34, 0xA8, 0x53))
                    .strong(),
            );
        });
    });

    if let Some(error) = &state.export_error {
        ui.label(
            RichText::new(format!("Export failed: {error}"))
                .color(Color32::from_rgb(0xC0, 0x39, 0x2B))
                .small(),
        );
    }
    if let Some(success) = &state.export_success {
        ui.label(RichText::new(success).small().weak());
    }

    ui.separator();

    let events = filtered_events(&all_events, state);

    debug_table_header(ui);
    ui.separator();
    let table_height = (ui.available_height() - 40.0).max(120.0);
    egui::ScrollArea::vertical()
        .max_height(table_height)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for event in events.iter().rev() {
                debug_table_row(ui, event);
            }
        });

    ui.separator();
    ui.label(
        RichText::new(format!(
            "{} events shown · ring buffer holds the last {} · stays on this Mac, never sent over the network",
            events.len(),
            debug_log::CAPACITY
        ))
        .small()
        .weak(),
    );
}

fn debug_filter_strip(ui: &mut egui::Ui, selected: &mut DebugFilter) {
    let dark = ui.visuals().dark_mode;
    let track_color = if dark {
        Color32::from_rgb(0x2C, 0x2C, 0x2E)
    } else {
        Color32::from_rgb(0xE3, 0xE3, 0xE7)
    };
    let active_color = if dark {
        Color32::from_rgb(0x48, 0x48, 0x4A)
    } else {
        Color32::WHITE
    };
    let active_text = if dark {
        Color32::from_rgb(0xF2, 0xF2, 0xF3)
    } else {
        Color32::from_rgb(0x1D, 0x1D, 0x1F)
    };
    let inactive_text = if dark {
        Color32::from_rgb(0x9A, 0x9A, 0xA0)
    } else {
        Color32::from_rgb(0x6E, 0x6E, 0x73)
    };

    egui::Frame::new()
        .fill(track_color)
        .corner_radius(6.0)
        .inner_margin(2.0)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (filter, label) in [
                    (DebugFilter::All, "All"),
                    (DebugFilter::Reversed, "Reversed"),
                    (DebugFilter::Passed, "Passed"),
                    (DebugFilter::Ignored, "Ignored"),
                ] {
                    let is_active = *selected == filter;
                    let (color, text_color) = if is_active {
                        (active_color, active_text)
                    } else {
                        (Color32::TRANSPARENT, inactive_text)
                    };
                    let button = egui::Button::new(
                        RichText::new(label).size(11.5).strong().color(text_color),
                    )
                    .fill(color)
                    .corner_radius(5.0)
                    .min_size(egui::vec2(64.0, 22.0));
                    if ui.add(button).clicked() {
                        *selected = filter;
                    }
                }
            });
        });
}

fn debug_table_header(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        debug_cell(ui, 96.0, RichText::new("Time").small().strong().weak());
        debug_cell(ui, 180.0, RichText::new("Device").small().strong().weak());
        debug_cell(ui, 76.0, RichText::new("Axis").small().strong().weak());
        debug_cell(
            ui,
            92.0,
            RichText::new("Δ raw → out").small().strong().weak(),
        );
        debug_cell(
            ui,
            ui.available_width(),
            RichText::new("Decision").small().strong().weak(),
        );
    });
}

fn debug_table_row(ui: &mut egui::Ui, event: &debug_log::DebugEvent) {
    ui.horizontal(|ui| {
        debug_cell(
            ui,
            96.0,
            RichText::new(format_timestamp(event.timestamp_ms)).monospace(),
        );
        debug_cell(ui, 180.0, event.device_description.as_str());
        debug_cell(ui, 76.0, event.axis.label());
        debug_cell(
            ui,
            92.0,
            RichText::new(format!("{} → {}", event.raw_delta, event.output_delta)).monospace(),
        );
        let color = match event.category {
            debug_log::DecisionCategory::Reversed => Color32::from_rgb(0x34, 0xA8, 0x53),
            debug_log::DecisionCategory::Passed => Color32::GRAY,
            debug_log::DecisionCategory::Ignored => Color32::from_rgb(0xE5, 0x9E, 0x2F),
        };
        debug_cell(
            ui,
            ui.available_width(),
            RichText::new(&event.decision_text).color(color),
        );
    });
}

fn debug_cell(ui: &mut egui::Ui, width: f32, text: impl Into<egui::WidgetText>) {
    ui.add_sized([width.max(24.0), 18.0], egui::Label::new(text).truncate());
}

fn filtered_events(
    all_events: &[debug_log::DebugEvent],
    state: &DebugConsoleState,
) -> Vec<debug_log::DebugEvent> {
    all_events
        .iter()
        .filter(|event| match state.filter {
            DebugFilter::All => true,
            DebugFilter::Reversed => event.category == debug_log::DecisionCategory::Reversed,
            DebugFilter::Passed => event.category == debug_log::DecisionCategory::Passed,
            DebugFilter::Ignored => event.category == debug_log::DecisionCategory::Ignored,
        })
        .filter(|event| event.matches_search(&state.search))
        .cloned()
        .collect()
}

fn format_timestamp(timestamp_ms: u128) -> String {
    let total_ms = timestamp_ms % (24 * 60 * 60 * 1000);
    let hours = total_ms / (60 * 60 * 1000);
    let minutes = (total_ms / (60 * 1000)) % 60;
    let seconds = (total_ms / 1000) % 60;
    let millis = total_ms % 1000;
    format!("{hours:02}:{minutes:02}:{seconds:02}.{millis:03}")
}

/// Writes the given (already-filtered) rows to a CSV file under the config
/// directory's sibling "Debug Logs" folder, next to `config.toml` -
/// following the same "next to the config file, under Application Support"
/// pattern `ConfigStore::default_path` already establishes, rather than
/// inventing a second on-disk location convention or a native save panel
/// (which egui/eframe has no built-in cross-platform support for without an
/// extra file-dialog dependency).
fn export_debug_events(events: &[debug_log::DebugEvent]) -> Result<std::path::PathBuf, String> {
    let config_path = ConfigStore::default_path();
    let export_dir = config_path
        .parent()
        .ok_or_else(|| "could not determine config directory".to_string())?
        .join("Debug Logs");
    std::fs::create_dir_all(&export_dir).map_err(|error| error.to_string())?;

    let now_ms = debug_log::now_millis();
    let file_path = export_dir.join(format!("debug-events-{now_ms}.csv"));

    let mut csv = String::from("timestamp_ms,device,axis,raw_delta,output_delta,decision\n");
    for event in events {
        csv.push_str(&format!(
            "{},{},{},{},{},{}\n",
            event.timestamp_ms,
            csv_escape(&event.device_description),
            event.axis.label(),
            event.raw_delta,
            event.output_delta,
            csv_escape(&event.decision_text),
        ));
    }

    std::fs::write(&file_path, csv).map_err(|error| error.to_string())?;
    Ok(file_path)
}

fn csv_escape(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}
