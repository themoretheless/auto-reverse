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
//! - A footnote states that an already-running `run` session keeps its old
//!   config until restarted - and the Permissions section's status row
//!   offers a "Restart" button that does exactly that, so this is never a
//!   dead end the user has to go find a terminal for.
//!
//! Opening this window also starts actual scroll reversal: `load()` spawns
//! a detached `<binary> run` child whenever `config.enabled` is true and
//! the required macOS permissions are already available.
//! Avoiding two live CGEventTaps is `run`'s own responsibility (see
//! `platform::macos::daemon_lock`) via an exclusive lock acquired before
//! anything else in `run_event_tap` - so a redundant spawn here (manual
//! `run`, a LaunchAgent, or a previous launch of this same window already
//! running) just costs a process that exits almost immediately, never a
//! second competing tap.

use std::fs::OpenOptions;
use std::process::{Command, Stdio};
use std::time::Duration;

use eframe::egui::{self, Color32, RichText};

use crate::config::{AppConfig, ConfigStore, DeviceRule};
use crate::platform::macos::{daemon_lock, hid, permissions, startup};

const WINDOW_WIDTH: f32 = 400.0;
const WINDOW_HEIGHT: f32 = 640.0;

/// Launches the settings window; blocks until it is closed.
pub fn run_settings_window() -> Result<(), String> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Auto Reverse")
            .with_inner_size([WINDOW_WIDTH, WINDOW_HEIGHT])
            .with_min_inner_size([WINDOW_WIDTH, 480.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Auto Reverse",
        options,
        Box::new(|_cc| Ok(Box::new(SettingsApp::load()))),
    )
    .map_err(|error| format!("could not open the settings window: {error}"))
}

struct SettingsApp {
    store: ConfigStore,
    config: AppConfig,
    devices: Vec<hid::DeviceInfo>,
    /// Why the device list is empty, when it failed rather than genuinely
    /// finding nothing - shown inline instead of silently swallowed.
    devices_error: Option<String>,
    /// Last save failure, shown inline; None while everything persists fine.
    save_error: Option<String>,
    load_error: Option<String>,
    /// Set when starting, stopping, or restarting the `run` daemon fails;
    /// shown inline rather than swallowed, mirroring `load_error`/`save_error`.
    daemon_error: Option<String>,
}

impl SettingsApp {
    fn load() -> Self {
        // One-shot, mirrors run_event_tap(): the request_* calls are what
        // actually register this binary with TCC (and pop the native
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

        // Opening the app is now also how scroll reversal starts, not just
        // where you look at settings. A second live CGEventTap would
        // silently double-negate every scroll event, but preventing that is
        // `run`'s own job (an exclusive daemon_lock acquired before it does
        // anything else) - see spawn_daemon's doc comment.
        let daemon_error = if config.enabled && permissions_ready {
            spawn_daemon().err()
        } else {
            None
        };

        let mut app = Self {
            store,
            config,
            devices: Vec::new(),
            devices_error: None,
            save_error: None,
            load_error,
            daemon_error,
        };
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

    fn save(&mut self) {
        self.save_error = self.store.save(&self.config).err().map(|e| e.to_string());
    }
}

impl eframe::App for SettingsApp {
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
            if let Some(error) = &self.daemon_error {
                ui.label(
                    RichText::new(format!("Daemon action failed: {error}"))
                        .color(Color32::from_rgb(0xC0, 0x39, 0x2B))
                        .small(),
                );
            }
            match daemon_status_row(ui, self.config.enabled, permissions_ready) {
                DaemonAction::Start => self.daemon_error = spawn_daemon().err(),
                DaemonAction::Restart => self.daemon_error = restart_daemon().err(),
                DaemonAction::None => {}
            }

            ui.add_space(8.0);
            ui.separator();
            footer(ui, &self.store, &self.load_error, &self.save_error);

            if ui.button("Restore defaults").clicked() {
                self.config = AppConfig::default();
                changed = true;
            }

            if changed {
                self.save();
                if self.save_error.is_none() && enabled_changed {
                    self.daemon_error = if self.config.enabled {
                        if permissions_ready {
                            spawn_daemon().err()
                        } else {
                            Some(
                                "permissions are still missing; grant them, then press Start now"
                                    .to_string(),
                            )
                        }
                    } else {
                        stop_daemon().err()
                    };
                }
            }
        }
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

/// Starts `<current executable> run` as a detached background process.
/// Always spawns unconditionally (when the caller's `config.enabled` check
/// already passed) - deduplication against an already-running daemon is
/// `run`'s own job now: it acquires an exclusive `daemon_lock` as the very
/// first thing it does, before this spawned copy could do anything else, so
/// a redundant spawn here just costs a process that exits almost
/// immediately rather than risking a second live CGEventTap. Never blocks
/// the GUI: no `.wait()` on the calling thread. The child is still reaped
/// (on a background thread, once it eventually exits) so it cannot become a
/// permanent zombie for as long as the GUI process stays open.
///
/// stdout/stderr are redirected to the same log file the LaunchAgent uses
/// (`startup::log_path()`), so a GUI-spawned daemon and a login-launched
/// one are indistinguishable in the logs.
fn spawn_daemon() -> Result<(), String> {
    let executable = startup::current_executable().map_err(|error| error.to_string())?;
    let log_path = startup::log_path().map_err(|error| error.to_string())?;
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            format!(
                "could not create log directory {}: {error}",
                parent.display()
            )
        })?;
    }

    let log_out = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|error| format!("could not open log file {}: {error}", log_path.display()))?;
    let log_err = log_out
        .try_clone()
        .map_err(|error| format!("could not duplicate log file handle: {error}"))?;

    let child = Command::new(&executable)
        .arg("run")
        .stdin(Stdio::null())
        .stdout(log_out)
        .stderr(log_err)
        .spawn()
        .map_err(|error| {
            format!(
                "could not start the scroll-reversal daemon ({}): {error}",
                executable.display()
            )
        })?;

    // Reap on a background thread instead of dropping the handle outright -
    // an un-waited child that outlives its parent's interest in it becomes
    // a zombie in the process table for as long as this GUI process (the
    // parent) stays running. This thread just blocks in wait() for however
    // long the daemon lives (normally: forever, until the user quits it),
    // which is a single idle kernel-blocked thread, not a busy loop.
    std::thread::spawn(move || {
        let mut child = child;
        let _ = child.wait();
    });

    Ok(())
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

/// Live status of the actual `run` daemon, checked fresh every frame via
/// `daemon_lock::is_running` - separate from the Permissions panel above,
/// which only reflects whether TCC access is granted, not whether a daemon
/// is actually alive right now (e.g. it can still be missing right after
/// granting access in System Settings, since granting permission alone
/// does not retroactively start anything). Offers a button either way: to
/// retry starting it on demand (most useful right after fixing something
/// in System Settings), or to restart an already-running one so it picks
/// up config changes - neither requires quitting and reopening the app, or
/// finding a terminal.
enum DaemonAction {
    None,
    Start,
    Restart,
}

fn daemon_status_row(ui: &mut egui::Ui, enabled: bool, permissions_ready: bool) -> DaemonAction {
    if !enabled {
        return DaemonAction::None;
    }

    let mut action = DaemonAction::None;
    ui.horizontal(|ui| {
        ui.label("Scroll reversal");
        if !permissions_ready {
            ui.label(
                RichText::new("waiting for permissions").color(Color32::from_rgb(0xE5, 0x9E, 0x2F)),
            );
            return;
        }

        if daemon_lock::is_running(&daemon_lock::default_path()) {
            ui.label(RichText::new("running").color(Color32::from_rgb(0x34, 0xA8, 0x53)));
            if ui
                .small_button("Restart")
                .on_hover_text("Applies any settings changed since it last started")
                .clicked()
            {
                action = DaemonAction::Restart;
            }
        } else {
            ui.label(RichText::new("not running").color(Color32::from_rgb(0xC0, 0x39, 0x2B)));
            if ui.small_button("Start now").clicked() {
                action = DaemonAction::Start;
            }
        }
    });
    action
}

fn stop_daemon() -> Result<(), String> {
    let stopped =
        daemon_lock::terminate_and_wait(&daemon_lock::default_path(), Duration::from_secs(2));
    if stopped {
        Ok(())
    } else {
        Err("the running scroll-reversal daemon did not stop in time; try again".to_string())
    }
}

/// Stops whichever daemon currently holds the lock and starts a fresh one,
/// so it (re)reads the current config instead of whatever it loaded at its
/// own startup. A 2-second wait is generous for a process whose only
/// per-event work is a couple of CGEvent field reads - if it is still
/// alive past that, something is genuinely stuck, and starting a second
/// one anyway would risk exactly the double-CGEventTap bug this whole
/// locking scheme exists to prevent, so this reports an error instead.
fn restart_daemon() -> Result<(), String> {
    stop_daemon()?;
    spawn_daemon()
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
        RichText::new("A running `auto-reverse run` keeps its old settings until restarted.")
            .small()
            .weak(),
    );
    ui.label(
        RichText::new(format!("Config: {}", store.path().display()))
            .small()
            .weak(),
    );
}
