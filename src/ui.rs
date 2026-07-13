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
//!   (`reverse_unknown` and the menu-bar/update placeholders). Rendering dead
//!   switches would be lying with widgets.
//! - Mouse wheel, trackpad, and Magic Mouse each have a live policy toggle;
//!   the latter two are separated by the public gesture timing classifier.
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

use std::sync::{Arc, Mutex, RwLock};

use eframe::egui::{self, Color32, RichText, ViewportCommand};

use crate::config::{AppConfig, ConfigRevision, ConfigStore, with_device_rule_selection};
use crate::device::DeviceIdentity;
use crate::platform::macos::{
    activation, daemon_lock, hid, login_item, permissions, power_events, quit_handler, tray,
};
use crate::runtime::{DEFAULT_PAUSE_DURATION, RuntimeControl};

mod debug_console;
mod local_export;
mod runtime;
mod scroll_benchmark;
mod theme;

use theme::{
    device_rule_chip, section, status_header, styled_button, styled_checkbox, styled_step_slider,
    tab_strip,
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
/// process leaves an activation request for the owner and exits cleanly.
pub fn run_settings_window() -> Result<(), String> {
    run_window(false)
}

/// Opens the same single-instance settings/runtime process with the advanced
/// Scroll Benchmark viewport visible on its first frame.
pub fn run_benchmark_window() -> Result<(), String> {
    run_window(true)
}

fn run_window(open_benchmark: bool) -> Result<(), String> {
    let ui_lock_path = daemon_lock::default_path().with_file_name("ui.lock");
    let Some(ui_instance_guard) =
        activation::acquire_or_activate(&ui_lock_path).map_err(|error| error.to_string())?
    else {
        return Ok(());
    };
    // Keep the guard in this stack frame through eframe and the park loop; its
    // Drop releases ui.lock only when this primary GUI truly ends.
    let activation_inbox = ui_instance_guard.inbox();

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
        Box::new(move |cc| {
            install_system_fonts(&cc.egui_ctx);
            Ok(Box::new(SettingsApp::load(
                activation_inbox,
                open_benchmark,
            )))
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

fn show_settings_window(ctx: &egui::Context) {
    // winit's macOS focus path ignores hidden windows, so visibility must be
    // applied first; Focus then activates the app and orders the window front.
    ctx.send_viewport_cmd(ViewportCommand::Visible(true));
    ctx.send_viewport_cmd(ViewportCommand::Focus);
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
    /// Exact TOML revision loaded by the long-lived editor. Shared with tray
    /// persistence so neither UI surface can overwrite a newer CLI write.
    config_revision: Arc<Mutex<Option<ConfigRevision>>>,
    /// Process-local pause shared by the UI, tray and tap hot path.
    runtime_control: Arc<RuntimeControl>,
    devices: Vec<hid::DeviceInfo>,
    /// Why the device list is empty, when it failed rather than genuinely
    /// finding nothing - shown inline instead of silently swallowed.
    devices_error: Option<String>,
    /// Last save failure, shown inline; None while everything persists fine.
    save_error: Option<String>,
    load_error: Option<String>,
    /// Typed lifecycle state and event channel for the in-process tap.
    tap_runtime: runtime::TapRuntime,
    /// Main-thread NSWorkspace sleep/wake observer. Installed lazily on the
    /// first eframe logic tick, alongside the other AppKit integrations.
    power_events: Option<power_events::PowerEventObserver>,
    /// Installation failure for the sleep/wake observer, shown rather than
    /// silently leaving post-wake recovery unavailable.
    power_events_error: Option<String>,
    /// AppKit tray construction failure, independent of tap lifecycle.
    tray_error: Option<String>,
    /// Failure while polling a second-launch request. A successful poll clears
    /// it, so transient filesystem errors do not remain after recovery.
    activation_error: Option<String>,
    activation_inbox: activation::ActivationInbox,
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
    /// Two-step guard for the destructive Restore defaults action.
    confirm_restore_defaults: bool,
    /// Set by the tray's "Open Debug Console…" item (`TrayAction::OpenDebugConsole`)
    /// and cleared when the console viewport's own close button is used.
    /// Drives whether `logic` calls `show_viewport_immediate` for the
    /// console this tick - see `debug_console` below.
    show_debug_console: bool,
    /// State local to the Debug Console window (handoff "1f") - filter tab,
    /// search text, and the last export/clear error, if any. Kept separate
    /// from the settings-window fields above since it is only ever touched
    /// while `show_debug_console` is true.
    debug_console: debug_console::State,
    show_scroll_benchmark: bool,
    scroll_benchmark: scroll_benchmark::State,
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
    fn load(activation_inbox: activation::ActivationInbox, show_scroll_benchmark: bool) -> Self {
        // One-shot, mirrors the old run_event_tap(): the request_* calls are
        // what actually register this binary with TCC (and pop the native
        // consent dialogs) - the has_* checks the permissions panel uses
        // are read-only and never do this. Without it, an install whose
        // only entry point is this window never appears in System
        // Settings > Privacy & Security for the user to grant.
        let permissions_ready = permissions::request_scroll_control_access();

        let store = ConfigStore::default();
        let (config, config_revision, load_error) = match store.load_or_create_snapshot() {
            Ok(snapshot) => (snapshot.config, Some(snapshot.revision), None),
            Err(error) => (AppConfig::default(), None, Some(error.to_string())),
        };

        let shared_config = Arc::new(RwLock::new(config.clone()));
        let config_revision = Arc::new(Mutex::new(config_revision));
        let runtime_control = Arc::new(RuntimeControl::default());

        let login_item_error = None;

        let mut app = Self {
            store,
            config,
            shared_config,
            config_revision,
            runtime_control,
            devices: Vec::new(),
            devices_error: None,
            save_error: None,
            load_error,
            tap_runtime: runtime::TapRuntime::default(),
            power_events: None,
            power_events_error: None,
            tray_error: None,
            activation_error: None,
            activation_inbox,
            login_item_error,
            tray: None,
            quit_handler_installed: false,
            selected_tab: if permissions_ready {
                SettingsTab::General
            } else {
                SettingsTab::Permissions
            },
            confirm_restore_defaults: false,
            show_debug_console: false,
            debug_console: debug_console::State::default(),
            show_scroll_benchmark,
            scroll_benchmark: scroll_benchmark::State::default(),
        };
        // Opening the app is now also how scroll reversal starts, not just
        // where you look at settings. If permissions are missing, leave the
        // attempt pending; `panel_contents` retries automatically when the
        // permission checks turn green.
        if app.config.enabled {
            if permissions_ready {
                app.start_tap_thread();
            } else {
                app.tap_runtime.wait_for_permissions();
            }
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
        let expected_revision = {
            let revision = self
                .config_revision
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            revision.clone()
        };
        let Some(expected_revision) = expected_revision else {
            self.save_error = Some(
                "config was not loaded successfully, so this edit was not saved; fix the config file and reopen Auto Reverse"
                    .to_string(),
            );
            self.sync_config_from_shared();
            return;
        };

        match self
            .store
            .save_if_unchanged(&self.config, &expected_revision)
        {
            Ok(revision) => {
                self.set_config_revision(revision);
                self.save_error = None;
                let mut guard = match self.shared_config.write() {
                    Ok(guard) => guard,
                    Err(poisoned) => poisoned.into_inner(),
                };
                *guard = self.config.clone();
            }
            Err(error) if error.is_config_changed() => {
                let conflict = error.to_string();
                match self.reload_external_config() {
                    Ok(true) => {
                        self.save_error = Some(format!(
                            "{conflict}. The newer external settings were reloaded; apply your edit again."
                        ));
                    }
                    Ok(false) => {
                        self.save_error = Some(conflict);
                        self.sync_config_from_shared();
                    }
                    Err(reload_error) => {
                        self.save_error = Some(format!(
                            "{conflict}; reloading the newer config also failed: {reload_error}"
                        ));
                        self.sync_config_from_shared();
                    }
                }
            }
            Err(error) => {
                self.save_error = Some(error.to_string());
                self.sync_config_from_shared();
            }
        }
    }

    fn set_config_revision(&self, revision: ConfigRevision) {
        let mut current = self
            .config_revision
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *current = Some(revision);
    }

    /// Reloads a newer external revision into both the widgets and the live
    /// event-tap config. Returns `false` when the disk revision is already the
    /// one this process knows, which distinguishes an ordinary I/O failure
    /// from a real stale-write conflict.
    fn reload_external_config(&mut self) -> Result<bool, String> {
        let snapshot = self
            .store
            .load_snapshot()
            .map_err(|error| error.to_string())?;
        let already_current = {
            let current = self
                .config_revision
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            current.as_ref() == Some(&snapshot.revision)
        };
        if already_current {
            return Ok(false);
        }

        let enabled_before = {
            let guard = match self.shared_config.read() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            guard.enabled
        };
        self.config = snapshot.config;
        {
            let mut guard = match self.shared_config.write() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            *guard = self.config.clone();
        }
        self.set_config_revision(snapshot.revision);
        self.load_error = None;

        if enabled_before != self.config.enabled {
            self.handle_enabled_changed(permissions_ready());
        }
        Ok(true)
    }

    fn start_tap_thread(&mut self) {
        self.tap_runtime.start_if_ready(
            permissions_ready(),
            Arc::clone(&self.shared_config),
            Arc::clone(&self.runtime_control),
        );
    }

    fn retry_tap_thread(&mut self) {
        self.tap_runtime.retry(
            Arc::clone(&self.shared_config),
            Arc::clone(&self.runtime_control),
        );
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
            if permissions_ready {
                self.start_tap_thread();
            } else {
                self.tap_runtime.wait_for_permissions();
            }
        } else {
            self.runtime_control.resume();
            self.tap_runtime.disabled();
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
        self.tap_runtime.poll();

        if self.power_events.is_none() && self.power_events_error.is_none() {
            match power_events::PowerEventObserver::install() {
                Ok(observer) => self.power_events = Some(observer),
                Err(error) => self.power_events_error = Some(error),
            }
        }
        let power_event = self
            .power_events
            .as_ref()
            .and_then(power_events::PowerEventObserver::poll);
        match power_event {
            Some(power_events::PowerEvent::WillSleep) | None => {}
            Some(power_events::PowerEvent::DidWake) => {
                // Devices can be connected or removed while asleep. Refresh
                // their UI snapshot once, and open a bounded tap-recovery
                // window even while the settings viewport is hidden.
                self.refresh_devices();
                if self.config.enabled {
                    self.tap_runtime.request_wake_recovery();
                }
            }
        }

        if self.config.enabled && self.tap_runtime.wake_recovery_pending() {
            self.tap_runtime.recover_after_wake(
                permissions_ready(),
                &self.shared_config,
                &self.runtime_control,
            );
        } else if !self.config.enabled {
            self.tap_runtime.disabled();
        }

        // Build the tray icon once on the main thread eframe already
        // drives; NSStatusItem/AppKit has the same constraint as eframe's
        // own window. Doing this here (first tick) rather than in `load()`
        // keeps all AppKit-touching setup on the thread eframe guarantees
        // is the right one.
        if self.tray.is_none() {
            let store_for_tray = self.store.clone();
            let revision_for_tray = Arc::clone(&self.config_revision);
            match tray::build(
                Arc::clone(&self.shared_config),
                Arc::clone(&self.runtime_control),
                move |config| {
                    let expected = {
                        let current = revision_for_tray
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        current.clone().ok_or_else(|| {
                            "config was not loaded successfully; tray edit was not saved"
                                .to_string()
                        })?
                    };
                    let revision = store_for_tray
                        .save_if_unchanged(config, &expected)
                        .map_err(|error| error.to_string())?;
                    let mut current = revision_for_tray
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    *current = Some(revision);
                    Ok(())
                },
            ) {
                Ok(handle) => self.tray = Some(handle),
                Err(error) => {
                    self.tray_error = Some(format!("tray icon failed: {error}"));
                }
            }
        }

        if let Some(tray) = &mut self.tray {
            tray.set_status(tray::TrayStatus::from_config(
                &self.config,
                self.runtime_control.is_paused(),
            ));
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

        // Process second-launch requests after close/quit-to-hide commands so
        // a concurrent relaunch wins the frame and leaves the window visible.
        match self.activation_inbox.poll() {
            Ok(should_activate) => {
                self.activation_error = None;
                if should_activate {
                    show_settings_window(ctx);
                }
            }
            Err(error) => self.activation_error = Some(error.to_string()),
        }

        match tray::poll_action() {
            Some(tray::TrayAction::OpenSettings) => {
                show_settings_window(ctx);
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
                let error = tray::take_last_save_error()
                    .unwrap_or_else(|| "tray change could not be saved".to_string());
                self.save_error = Some(match self.reload_external_config() {
                    Ok(true) => format!(
                        "{error}. A newer external config was reloaded; repeat the tray action."
                    ),
                    Ok(false) => error,
                    Err(reload_error) => {
                        format!("{error}; checking for an external update failed: {reload_error}")
                    }
                });
            }
            Some(tray::TrayAction::PauseChanged) => {}
            None => {}
        }

        if self.show_debug_console && debug_console::show_viewport(ctx, &mut self.debug_console) {
            self.show_debug_console = false;
        }
        if self.debug_console.take_benchmark_request() {
            self.show_scroll_benchmark = true;
        }
        if self.show_scroll_benchmark
            && scroll_benchmark::show_viewport(ctx, &mut self.scroll_benchmark)
        {
            self.show_scroll_benchmark = false;
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
        let mut clear_temporary_pause = false;
        let permissions_ready = permissions_ready();

        status_header(
            ui,
            &self.config,
            permissions_ready,
            self.runtime_control.remaining_pause(),
        );
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

        let paused = self.runtime_control.is_paused();
        if self.config.enabled && (permissions_ready || paused) {
            let label = if paused {
                "Resume now"
            } else {
                "Pause for 15 minutes"
            };
            if styled_button(ui, label, egui::vec2(12.0, 5.0)).clicked() {
                if paused {
                    self.runtime_control.resume();
                } else {
                    self.runtime_control.pause_for(DEFAULT_PAUSE_DURATION);
                }
            }
        }

        // Independent of which tab is rendered below: granting permissions
        // (or the config becoming enabled) must auto-start the tap on the
        // very next tick even if the user is looking at General or
        // Devices, not only when the Permissions tab happens to be open.
        if self.config.enabled {
            if permissions_ready {
                self.start_tap_thread();
            } else {
                self.tap_runtime.wait_for_permissions();
            }
        }

        // Pinned above the tabs, not inside the Permissions tab's content:
        // in the single-panel layout this replaced, a failed tap start was
        // always visible regardless of scroll position. Tab-scoping it
        // would silently hide "scroll reversal could not start" from a user
        // looking at General or Devices - the exact kind of error this
        // project's honesty rules (see module doc comment) require staying
        // visible, not buried behind a tab click.
        let tap_error = self.tap_runtime.state().error_message().map(str::to_owned);
        if let Some(error) = tap_error {
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
                && self.tap_runtime.state().can_retry()
                && styled_button(ui, "Retry starting scroll reversal", egui::vec2(12.0, 5.0))
                    .clicked()
            {
                self.retry_tap_thread();
            }
        }
        if let Some(error) = &self.tray_error {
            ui.label(
                RichText::new(error)
                    .color(Color32::from_rgb(0xC0, 0x39, 0x2B))
                    .small(),
            );
        }
        if let Some(error) = &self.activation_error {
            ui.label(
                RichText::new(format!("Window activation unavailable: {error}"))
                    .color(Color32::from_rgb(0xC0, 0x39, 0x2B))
                    .small(),
            );
        }
        if let Some(error) = &self.power_events_error {
            ui.label(
                RichText::new(format!("Sleep/wake recovery unavailable: {error}"))
                    .color(Color32::from_rgb(0xC0, 0x39, 0x2B))
                    .small(),
            );
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
                        RichText::new("Trackpad").size(13.0),
                        16.0,
                        4.0,
                    )
                    .changed();
                    changed |= styled_checkbox(
                        ui,
                        &mut self.config.reverse_magic_mouse,
                        RichText::new("Magic Mouse").size(13.0),
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

        if self.confirm_restore_defaults {
            ui.label(
                RichText::new(format!(
                    "Restore every setting and remove {} per-device rule(s)?",
                    self.config.device_rules.len()
                ))
                .small()
                .strong(),
            );
            ui.horizontal(|ui| {
                if styled_button(ui, "Cancel", egui::vec2(14.0, 6.0)).clicked() {
                    self.confirm_restore_defaults = false;
                }
                if styled_button(ui, "Restore", egui::vec2(14.0, 6.0)).clicked() {
                    self.config = AppConfig::default();
                    clear_temporary_pause = true;
                    self.confirm_restore_defaults = false;
                    changed = true;
                }
            });
        } else if styled_button(ui, "Restore defaults", egui::vec2(14.0, 6.0)).clicked() {
            self.confirm_restore_defaults = true;
        }

        if changed {
            let enabled_changed = enabled_before != self.config.enabled;
            self.save();
            // Note: there is no in-process "stop the tap thread" action
            // for the disable-only branch - the background thread reads
            // the shared config on every event and simply passes events
            // through unmodified when `enabled` is false (the same
            // pass-through behavior `scroll::transform_event` already
            // implements), so disabling here takes effect immediately
            // without needing to tear down the thread.
            if self.save_error.is_none() {
                if clear_temporary_pause {
                    self.runtime_control.resume();
                }
                if enabled_changed {
                    self.handle_enabled_changed(permissions_ready);
                }
            }
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
            .preferred_device_rule(&device.identity)
            .map(|rule| rule.reverse);
        let inherited_note = if current.is_none() {
            config.matching_device_rule(&device.identity).map(|rule| {
                let scope = if rule.is_hardware_wide() {
                    "Shared model rule"
                } else {
                    "Port fallback"
                };
                format!(
                    "{scope}: {}",
                    if rule.reverse {
                        "Reverse"
                    } else {
                        "Don't reverse"
                    }
                )
            })
        } else {
            None
        };

        let label = device.name.clone().unwrap_or_else(|| "Unnamed".to_string());
        let identity_label = compact_device_identity(&device.identity);
        let mut selection = current;
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(&label);
                ui.label(RichText::new(&identity_label).small().monospace().weak());
                if let Some(note) = &inherited_note {
                    ui.label(RichText::new(note).small().weak());
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                device_rule_chip(ui, ("device-rule", &device.identity), &mut selection);
            });
        });
        if selection != current {
            config.device_rules = with_device_rule_selection(
                &config.device_rules,
                &device.identity,
                device.name.as_deref(),
                selection,
            );
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

fn compact_device_identity(identity: &DeviceIdentity) -> String {
    let mut label = format!(
        "{:04x}:{:04x}",
        identity.hardware.vendor_id, identity.hardware.product_id
    );
    if let Some(qualifier) = identity.compact_qualifier() {
        label.push_str(&format!(" · {qualifier}"));
    } else {
        label.push_str(" · model-wide ID");
    }
    label
}

/// Pure check, independent of any widget - Accessibility granted. Used
/// by `panel_contents` to drive the tap-start/retry logic on every tick
/// regardless of which tab (if any) is currently rendering the Permissions
/// section, so granting permissions while on the General or Devices tab
/// still auto-starts scroll reversal instead of requiring a visit to
/// Permissions first.
fn permissions_ready() -> bool {
    permissions::has_scroll_control_access()
}

fn permissions_panel(ui: &mut egui::Ui) {
    let accessibility = permissions::has_accessibility_trust();
    ui.horizontal(|ui| {
        ui.label("Accessibility");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if accessibility {
                ui.label(
                    RichText::new("Granted")
                        .color(Color32::from_rgb(0x34, 0xA8, 0x53))
                        .strong(),
                );
            } else {
                ui.label(
                    RichText::new("Required")
                        .color(Color32::from_rgb(0xE5, 0x9E, 0x2F))
                        .strong(),
                );
            }
        });
    });
    if !accessibility {
        ui.label(
            RichText::new(
                "Add Auto Reverse.app in System Settings to observe and modify scroll events.",
            )
            .small()
            .weak(),
        );
        if styled_button(ui, "Open Accessibility", egui::vec2(12.0, 5.0)).clicked() {
            open_privacy_pane("Privacy_Accessibility");
        }
    }
}

fn open_privacy_pane(pane: &str) {
    let _ = std::process::Command::new("open")
        .arg(format!(
            "x-apple.systempreferences:com.apple.preference.security?{pane}"
        ))
        .spawn();
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

#[cfg(test)]
mod device_identity_label_tests {
    use std::sync::Arc;

    use crate::device::HardwareId;

    use super::*;

    fn hardware() -> HardwareId {
        HardwareId {
            vendor_id: 0x046d,
            product_id: 0xc52b,
        }
    }

    #[test]
    fn serial_label_keeps_a_bounded_distinguishing_suffix() {
        let identity =
            DeviceIdentity::new(hardware(), Some(Arc::from("1234567890abcdef")), Some(42));

        assert_eq!(
            compact_device_identity(&identity),
            "046d:c52b · serial …567890abcdef"
        );
    }

    #[test]
    fn location_fallback_is_named_as_a_port() {
        let identity = DeviceIdentity::new(hardware(), None, Some(42));

        assert_eq!(
            compact_device_identity(&identity),
            "046d:c52b · port 0x0000002a"
        );
    }
}
