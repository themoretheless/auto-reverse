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
    daemon_lock, event_tap, hid, login_item, permissions, quit_handler, tray,
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
        Box::new(|_cc| Ok(Box::new(SettingsApp::load()))),
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
            match tray::build() {
                Ok(handle) => self.tray = Some(handle),
                Err(error) => {
                    self.tap_error = Some(match &self.tap_error {
                        Some(existing) => format!("{existing}; also: tray icon failed: {error}"),
                        None => format!("tray icon failed: {error}"),
                    })
                }
            }
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
            None => {}
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
    fn panel_contents(&mut self, ui: &mut egui::Ui) {
        {
            ui.spacing_mut().item_spacing.y = 8.0;
            let mut changed = false;

            status_header(ui, &self.config);
            ui.add_space(4.0);

            // The single most-used control, directly under the status.
            let enabled_before = self.config.enabled;
            changed |= ui
                .checkbox(&mut self.config.enabled, "Reverse scrolling")
                .changed();
            let enabled_changed = enabled_before != self.config.enabled;

            ui.add_space(8.0);
            ui.separator();

            section(ui, "What gets reversed");
            ui.add_enabled_ui(self.config.enabled, |ui| {
                changed |= ui
                    .checkbox(&mut self.config.reverse_mouse, "Mouse wheel")
                    .changed();
                changed |= ui
                    .checkbox(
                        &mut self.config.reverse_trackpad,
                        "Trackpad (includes Magic Mouse)",
                    )
                    .changed();
            });

            section(ui, "Directions");
            ui.add_enabled_ui(self.config.enabled, |ui| {
                changed |= ui
                    .checkbox(&mut self.config.reverse_vertical, "Vertical")
                    .changed();
                changed |= ui
                    .checkbox(&mut self.config.reverse_horizontal, "Horizontal")
                    .changed();
            });

            section(ui, "Wheel step size");
            ui.add_enabled_ui(self.config.enabled && self.config.reverse_mouse, |ui| {
                changed |= ui
                    .add(egui::Slider::new(
                        &mut self.config.discrete_scroll_step_size,
                        0..=20,
                    ))
                    .changed();
                ui.label(
                    RichText::new("Lines per wheel notch. 0 keeps the system speed.")
                        .small()
                        .weak(),
                );
            });

            ui.add_space(8.0);
            ui.separator();
            section(ui, "Per-device rules");
            let (rules_changed, wants_refresh) = device_rules(ui, &self.devices, &mut self.config);
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

            ui.add_space(8.0);
            ui.separator();
            section(ui, "Permissions");
            let permissions_ready = permissions_panel(ui);
            if self.config.enabled && permissions_ready && !self.tap_start_attempted {
                self.start_tap_thread();
            }
            if let Some(error) = &self.tap_error {
                ui.label(
                    RichText::new(format!("Scroll reversal could not start: {error}"))
                        .color(Color32::from_rgb(0xC0, 0x39, 0x2B))
                        .small(),
                );
                if permissions_ready
                    && self.config.enabled
                    && ui.small_button("Retry starting scroll reversal").clicked()
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
            section(ui, "Start at login");
            self.login_item_row(ui);

            ui.add_space(8.0);
            ui.separator();
            footer(ui, &self.store, &self.load_error, &self.save_error);

            if ui.button("Restore defaults").clicked() {
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
                if ui.small_button("Turn off").clicked() {
                    self.login_item_error = login_item::unregister().err();
                }
            }
            login_item::LoginItemStatus::NotRegistered | login_item::LoginItemStatus::NotFound => {
                if ui.small_button("Turn on").clicked() {
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
        // A painted circle, not the "●" glyph: egui's default font renders
        // that codepoint as a square, which reads as a broken icon.
        let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
        ui.painter().circle_filled(rect.center(), 6.0, dot_color);
        ui.label(RichText::new(status_word).size(18.0).strong());
    });
    ui.label(RichText::new(config.plain_english_summary()).weak());
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(8.0);
    ui.label(RichText::new(title).strong());
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
        ui.horizontal(|ui| {
            ui.label(&label);
            ui.label(
                RichText::new(format!(
                    "{:04x}:{:04x}",
                    device.hardware.vendor_id, device.hardware.product_id
                ))
                .small()
                .weak(),
            );
            let mut selection = current;
            egui::ComboBox::from_id_salt((device.hardware.vendor_id, device.hardware.product_id))
                .selected_text(match selection {
                    None => "Default",
                    Some(true) => "Reverse",
                    Some(false) => "Don't reverse",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut selection, None, "Default");
                    ui.selectable_value(&mut selection, Some(true), "Reverse");
                    ui.selectable_value(&mut selection, Some(false), "Don't reverse");
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
        });
    }

    ui.horizontal(|ui| {
        if ui.small_button("Refresh devices").clicked() {
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
            if granted {
                ui.label(RichText::new("granted").color(Color32::from_rgb(0x34, 0xA8, 0x53)));
            } else {
                any_missing = true;
                ui.label(RichText::new("required").color(Color32::from_rgb(0xE5, 0x9E, 0x2F)));
            }
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
        if ui
            .small_button("Open Privacy & Security settings")
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
            .weak(),
    );
}
