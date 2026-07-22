// The library's pure core builds on any OS (cargo check --lib), but this
// binary drives a CGEventTap and is macOS-only. Without this guard a
// non-macOS build dies on a bare E0432 unresolved-import error; with it,
// the failure explains itself.
#[cfg(not(target_os = "macos"))]
compile_error!(
    "the auto-reverse binary is macOS-only; on other platforms build just the library with --lib"
);

mod cli;

use std::env;
use std::ffi::OsStr;
use std::fmt::Write as _;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::process;
use std::sync::{Arc, RwLock};

use auto_reverse::config::{AppConfig, ConfigRepairOutcome, ConfigStore, with_dynamics_rollback};
use auto_reverse::device_catalog::{DeviceState, build_device_catalog};
use auto_reverse::device_classifier;
use auto_reverse::error::{AppError, AppResult};
use auto_reverse::event_rate::{DeviceEventRate, millihertz_to_hertz};
use auto_reverse::input::ScrollEvent;
#[cfg(feature = "gui")]
use auto_reverse::platform::macos::{
    activation, daemon_lock, login_item, login_item::LoginItemStatus,
};
use auto_reverse::platform::macos::{event_tap, external_url, hid, permissions, startup};
use auto_reverse::scroll;
use auto_reverse::scroll_lab::{self, AxisMetrics, Distribution};
use auto_reverse::scroll_trace::{MAX_TRACE_BYTES, ScrollTrace, TraceError};
use auto_reverse::update_policy::{ReleaseChannel, UpdatePolicy};
use cli::{
    Command, DoctorOptions, OpenReleasesOptions, OutputFormat, SimulateOptions,
    StartupStatusOptions, TraceLabOptions, ValidationOptions,
};

fn main() {
    if let Err(error) = run() {
        eprintln!("auto-reverse[{}]: {error}", error.code());
        process::exit(1);
    }
}

fn run() -> AppResult<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    let command = match env::current_exe() {
        Ok(executable) => command_for_launch(&args, &executable)?,
        Err(_) => cli::parse_args(&args)?,
    };

    match command {
        Command::Run => run_event_tap(),
        Command::Doctor(options) => doctor(options),
        Command::Init => init_config(),
        Command::Enable => set_enabled(true),
        Command::Disable => set_enabled(false),
        Command::Toggle => toggle_enabled(),
        Command::EnableStartup => set_startup_enabled(true),
        Command::DisableStartup => set_startup_enabled(false),
        Command::ShowMenuBarIcon => show_menu_bar_icon(),
        Command::RollbackDynamics => rollback_dynamics(),
        Command::ValidateConfig(options) => validate_config(options),
        Command::RepairConfig => repair_config(),
        Command::OpenReleases(options) => open_releases(options),
        Command::PrepareUninstall => prepare_uninstall(),
        Command::StartupStatus(options) => startup_status(options),
        Command::ConfigPath => {
            println!("{}", ConfigStore::default_path().display());
            Ok(())
        }
        Command::ShowConfig => show_config(),
        Command::Simulate(options) => simulate(options),
        Command::TraceLab(options) => trace_lab(options),
        Command::Devices => list_devices(),
        Command::Benchmark => launch_benchmark(),
        Command::Ui => launch_ui(),
        Command::Help => {
            print_help();
            Ok(())
        }
    }
}

fn command_for_launch(args: &[String], executable: &Path) -> AppResult<Command> {
    if launched_from_app_bundle_without_args(args, executable) {
        return Ok(Command::Ui);
    }

    cli::parse_args(args)
}

fn launched_from_app_bundle_without_args(args: &[String], executable: &Path) -> bool {
    args.is_empty() && is_app_bundle_executable(executable)
}

fn is_app_bundle_executable(executable: &Path) -> bool {
    let components: Vec<_> = executable
        .components()
        .map(|component| component.as_os_str())
        .collect();

    components.windows(3).any(|window| {
        window[0].to_string_lossy().ends_with(".app")
            && window[1] == OsStr::new("Contents")
            && window[2] == OsStr::new("MacOS")
    })
}

fn run_event_tap() -> AppResult<()> {
    let store = ConfigStore::default();
    let config = store.load_or_create()?;

    println!("auto-reverse: config {}", store.path().display());
    println!("auto-reverse: {}", config_summary(&config));

    if !config.enabled {
        println!("auto-reverse: disabled in config; run `auto-reverse enable` to turn it on");
        return Ok(());
    }

    if !permissions::request_scroll_control_access() {
        permissions::print_permission_help();
        return Err(AppError::Permission(
            "Accessibility permission is not granted".to_string(),
        ));
    }

    println!(
        "auto-reverse: config changes made while this headless `run` process is running have \
         no effect until restart. An open `ui` process does not continuously watch external \
         edits either, but its next window or tray save compares the exact TOML revision: a \
         newer CLI or manual edit is reloaded instead of being silently overwritten."
    );

    // install_and_run acquires the exclusive daemon_lock itself, as the
    // very first thing it does, before touching the HID monitor or the
    // CGEventTap - this is the one gate shared with the merged `ui`
    // process's in-process tap thread, so the two launch paths can never
    // both hold a live tap. A second, redundant `run` (or a `ui` process
    // already running) observes the lock already held and returns cleanly
    // rather than installing a competing tap.
    match event_tap::install_and_run(Arc::new(RwLock::new(config)))? {
        event_tap::TapRunOutcome::AlreadyRunning => Ok(()),
        event_tap::TapRunOutcome::Stopped => Err(AppError::Platform(
            "the event tap run loop stopped unexpectedly".to_string(),
        )),
    }
}

fn doctor(options: DoctorOptions) -> AppResult<()> {
    let store = ConfigStore::default();
    let (config, config_state) = load_config_for_diagnostics(&store, options.create_config)?;

    let accessibility = permissions::has_accessibility_trust();
    let current_exe = startup::current_executable()?;
    // Deliberately not `?`: an unreadable LaunchAgent must not suppress the
    // rest of the diagnostics - permissions are what doctor exists to show.
    let startup_summary = match startup::status_for_executable(&current_exe) {
        Ok(status) => status.summary(),
        Err(error) => format!("could not determine ({error})"),
    };
    let status = if !config.enabled {
        "OFF (disabled in config)"
    } else if !accessibility {
        "NEEDS PERMISSION"
    } else {
        "ON"
    };

    println!("Auto Reverse doctor");
    println!("status: {status}");
    println!("what it's doing: {}", config.plain_english_summary());
    println!();
    println!("version: {}", env!("CARGO_PKG_VERSION"));
    println!("binary: {}", current_exe.display());
    println!("config: {}", config_state.summary(store.path()));
    println!("settings: {}", config_summary(&config));
    println!(
        "start at login: {} (config start_at_login={})",
        startup_summary, config.start_at_login
    );
    println!(
        "accessibility permission: {}",
        permissions::permission_status(accessibility)
    );
    println!(
        "device classifier: {}",
        device_classifier::CLASSIFIER_DESCRIPTION
    );
    match hid::continuous_source_hint() {
        Ok(hint) => println!("connected continuous devices: {}", hint.description()),
        Err(error) => println!("connected continuous devices: unavailable ({error})"),
    }
    println!(
        "classifier note: exclusive connected-device evidence wins; the public two-finger \
         timing heuristic is used only when both a trackpad and Magic Mouse are present"
    );
    println!(
        "known gap: reverse_unknown has no live effect yet because DeviceKind::Unknown is \
         unreachable outside `simulate`"
    );
    println!(
        "menu bar icon: {} (recovery: `auto-reverse show-menu-bar-icon` or reopen the app)",
        if config.show_menu_bar_icon {
            "shown"
        } else {
            "hidden"
        }
    );
    println!(
        "known gap: show_discrete_scroll_options is stored for planned UI behavior but is not \
         applied by the runtime yet"
    );
    let update_policy =
        UpdatePolicy::from_legacy_flags(config.check_for_updates, config.include_beta_updates);
    println!("updates: {}", update_policy.strategy_label());
    println!(
        "update channel: {} ({})",
        update_policy.channel.label(),
        update_policy.channel.url()
    );
    if update_policy.legacy_automatic_check_requested {
        println!(
            "update note: check_for_updates=true is retained for compatibility but never starts \
             a background request; use `open-releases` explicitly"
        );
    }

    if !accessibility {
        println!();
        permissions::print_permission_help();
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfigDiagnosticState {
    Existing,
    Created,
    MissingUsingDefaults,
}

impl ConfigDiagnosticState {
    fn summary(self, path: &Path) -> String {
        match self {
            Self::Existing => path.display().to_string(),
            Self::Created => format!("{} (created)", path.display()),
            Self::MissingUsingDefaults => {
                format!(
                    "{} (missing; using defaults for this report)",
                    path.display()
                )
            }
        }
    }
}

fn load_config_for_diagnostics(
    store: &ConfigStore,
    create_config: bool,
) -> AppResult<(AppConfig, ConfigDiagnosticState)> {
    if store.exists() {
        return Ok((store.load()?, ConfigDiagnosticState::Existing));
    }

    let config = AppConfig::default();
    if create_config {
        let snapshot = store.load_or_create_snapshot()?;
        Ok((snapshot.config, ConfigDiagnosticState::Created))
    } else {
        Ok((config, ConfigDiagnosticState::MissingUsingDefaults))
    }
}

fn init_config() -> AppResult<()> {
    let store = ConfigStore::default();
    if store.exists() {
        println!("config already exists: {}", store.path().display());
        return Ok(());
    }

    store.load_or_create()?;
    println!("created config: {}", store.path().display());
    notify_running_gui_reload_best_effort();
    Ok(())
}

fn validate_config(options: ValidationOptions) -> AppResult<()> {
    let store = ConfigStore::default();
    match store.inspect_existing() {
        Ok(Some(config)) => {
            match options.format {
                OutputFormat::Text => {
                    println!("config is valid: {}", store.path().display());
                    println!("config version: {}", config.config_version);
                }
                OutputFormat::Json => {
                    println!("{{");
                    println!("  \"status\": \"valid\",");
                    println!("  \"config_version\": {},", config.config_version);
                    println!(
                        "  \"path\": \"{}\"",
                        json_escape(&store.path().display().to_string())
                    );
                    println!("}}");
                }
            }
            Ok(())
        }
        Ok(None) => {
            match options.format {
                OutputFormat::Text => {
                    println!("config is missing: {}", store.path().display());
                }
                OutputFormat::Json => {
                    println!("{{");
                    println!("  \"status\": \"missing\",");
                    println!(
                        "  \"path\": \"{}\"",
                        json_escape(&store.path().display().to_string())
                    );
                    println!("}}");
                }
            }
            Ok(())
        }
        Err(error) => {
            let status = if matches!(
                error,
                AppError::ConfigParse { .. } | AppError::InvalidConfig(_)
            ) {
                "invalid"
            } else {
                "error"
            };
            match options.format {
                OutputFormat::Text => {
                    println!("config {status}: {}", store.path().display());
                    println!("code: {}", error.code());
                    println!("message: {error}");
                }
                OutputFormat::Json => {
                    println!("{{");
                    println!("  \"status\": \"{status}\",");
                    println!("  \"code\": \"{}\",", error.code());
                    println!("  \"message\": \"{}\",", json_escape(&error.to_string()));
                    println!(
                        "  \"path\": \"{}\"",
                        json_escape(&store.path().display().to_string())
                    );
                    println!("}}");
                }
            }
            Err(error)
        }
    }
}

fn repair_config() -> AppResult<()> {
    let store = ConfigStore::default();
    match store.repair_with_defaults()? {
        ConfigRepairOutcome::Unchanged { config } => {
            println!("config is already valid: {}", store.path().display());
            println!("config version: {}", config.config_version);
        }
        ConfigRepairOutcome::Created { config } => {
            println!("created default config: {}", store.path().display());
            println!("config version: {}", config.config_version);
            notify_running_gui_reload_best_effort();
        }
        ConfigRepairOutcome::Repaired {
            config,
            backup_path,
        } => {
            println!(
                "repaired config with safe defaults: {}",
                store.path().display()
            );
            println!("original bytes preserved: {}", backup_path.display());
            println!("config version: {}", config.config_version);
            notify_running_gui_reload_best_effort();
        }
    }
    Ok(())
}

fn open_releases(options: OpenReleasesOptions) -> AppResult<()> {
    let channel = match options.channel {
        Some(channel) => channel,
        None => ConfigStore::default()
            .inspect_existing()?
            .map(|config| {
                UpdatePolicy::from_legacy_flags(
                    config.check_for_updates,
                    config.include_beta_updates,
                )
                .channel
            })
            .unwrap_or(ReleaseChannel::LatestStable),
    };
    external_url::open_release_page(channel)?;
    println!("opened {}: {}", channel.label(), channel.url());
    Ok(())
}

fn set_enabled(enabled: bool) -> AppResult<()> {
    let store = ConfigStore::default();
    store.update(|config| config.enabled = enabled)?;
    notify_running_gui_reload_best_effort();
    println!(
        "auto-reverse is now {}",
        if enabled { "enabled" } else { "disabled" }
    );
    Ok(())
}

fn toggle_enabled() -> AppResult<()> {
    let store = ConfigStore::default();
    let snapshot = store.update(|config| config.enabled = !config.enabled)?;
    let enabled = snapshot.config.enabled;
    notify_running_gui_reload_best_effort();
    println!(
        "auto-reverse is now {}",
        if enabled { "enabled" } else { "disabled" }
    );
    Ok(())
}

fn rollback_dynamics() -> AppResult<()> {
    let store = ConfigStore::default();
    let snapshot = store.update(|config| *config = with_dynamics_rollback(config))?;
    notify_running_gui_reload_best_effort();
    println!(
        "experimental dynamics rolled back to {}; wheel step and unrelated settings preserved",
        snapshot.config.smooth_preset.as_str()
    );
    Ok(())
}

fn set_startup_enabled(enabled: bool) -> AppResult<()> {
    let status = if enabled {
        enable_cli_startup_exclusively()?
    } else {
        startup::disable()?
    };

    let store = ConfigStore::default();
    let config_result = store.update(|config| config.start_at_login = enabled);
    if let Err(error) = config_result {
        // The launch agent was already changed above; without this note the
        // user has no way to know config and agent now disagree.
        eprintln!(
            "auto-reverse: the launch agent was updated, but saving the config failed; \
             rerun this command to bring them back in sync"
        );
        return Err(error);
    }
    notify_running_gui_reload_best_effort();

    println!("start at login: {}", status.summary());
    if enabled {
        println!("auto-reverse will start on the next login using the current binary path");
        warn_if_dev_tree_binary();
    }
    Ok(())
}

#[cfg(feature = "gui")]
fn enable_cli_startup_exclusively() -> AppResult<startup::StartupStatus> {
    let gui_was_registered = matches!(
        login_item::status(),
        LoginItemStatus::Enabled | LoginItemStatus::RequiresApproval
    );
    let status = startup::enable_for_current_executable()?;
    if !gui_was_registered {
        return Ok(status);
    }

    if let Err(error) = login_item::unregister() {
        let rollback = startup::disable();
        let rollback_note = rollback
            .err()
            .map(|rollback_error| format!("; LaunchAgent rollback also failed: {rollback_error}"))
            .unwrap_or_default();
        return Err(AppError::Platform(format!(
            "could not replace the GUI login item with the CLI LaunchAgent: {error}{rollback_note}"
        )));
    }
    Ok(status)
}

#[cfg(not(feature = "gui"))]
fn enable_cli_startup_exclusively() -> AppResult<startup::StartupStatus> {
    startup::enable_for_current_executable()
}

fn show_menu_bar_icon() -> AppResult<()> {
    let store = ConfigStore::default();
    store.update(|config| config.show_menu_bar_icon = true)?;
    println!("menu bar icon: shown");

    match notify_existing_settings_window(true) {
        Ok(true) => println!("the running settings process was asked to reload and reopen"),
        Ok(false) => println!("the icon will be visible the next time Auto Reverse opens"),
        Err(error) => eprintln!(
            "auto-reverse: the setting was saved, but the running window could not be notified \
             ({error}); reopen Auto Reverse to apply it"
        ),
    }
    Ok(())
}

#[cfg(feature = "gui")]
fn notify_existing_settings_window(open_window: bool) -> AppResult<bool> {
    let ui_lock_path = daemon_lock::default_path().with_file_name("ui.lock");
    let action = if open_window {
        activation::ActivationAction::ReloadAndOpen
    } else {
        activation::ActivationAction::ReloadOnly
    };
    activation::request_existing_gui_if_running(&ui_lock_path, action)
}

#[cfg(not(feature = "gui"))]
fn notify_existing_settings_window(_open_window: bool) -> AppResult<bool> {
    Ok(false)
}

fn notify_running_gui_reload_best_effort() {
    if let Err(error) = notify_existing_settings_window(false) {
        eprintln!(
            "auto-reverse: the config was saved, but the running GUI could not be notified to \
             reload it ({error}); reopen Auto Reverse to apply the change"
        );
    }
}

/// Removes both startup registrations before an installer deletes the app.
/// File removal deliberately stays in `scripts/uninstall-app-bundle.sh`, where
/// the destination is verified as our bundle before recursive deletion.
fn prepare_uninstall() -> AppResult<()> {
    println!(
        "GUI login item: {}",
        disable_gui_login_item_for_uninstall()?
    );

    let startup_status = startup::disable()?;
    println!("CLI LaunchAgent: {}", startup_status.summary());

    let store = ConfigStore::default();
    if store.exists() {
        match store.update(|config| config.start_at_login = false) {
            Ok(_) => {
                println!(
                    "config retained with start_at_login=false: {}",
                    store.path().display()
                );
                notify_running_gui_reload_best_effort();
            }
            Err(error) => eprintln!(
                "auto-reverse: startup registrations were removed, but the retained config \
                 could not be updated ({error}): {}",
                store.path().display()
            ),
        }
    } else {
        println!("config: not present ({})", store.path().display());
    }

    Ok(())
}

#[cfg(feature = "gui")]
fn disable_gui_login_item_for_uninstall() -> AppResult<String> {
    match login_item::status() {
        LoginItemStatus::Enabled | LoginItemStatus::RequiresApproval => {
            login_item::unregister().map_err(|error| {
                AppError::Platform(format!("could not unregister the GUI login item: {error}"))
            })?;
            let status = login_item::status();
            if matches!(
                status,
                LoginItemStatus::Enabled | LoginItemStatus::RequiresApproval
            ) {
                return Err(AppError::Platform(format!(
                    "GUI login item remained registered after cleanup: {}",
                    status.summary()
                )));
            }
            Ok(status.summary().to_string())
        }
        status => Ok(status.summary().to_string()),
    }
}

#[cfg(not(feature = "gui"))]
fn disable_gui_login_item_for_uninstall() -> AppResult<String> {
    Ok("not available in this lean build; no GUI bundle service was changed".to_string())
}

/// A LaunchAgent pointing into target/ is fragile: unsigned and ad-hoc
/// rebuilds change the binary's identity, so TCC grants stop matching and the
/// login launch fails. Warn instead of refusing - it is still the right
/// workflow for trying the feature out.
fn warn_if_dev_tree_binary() {
    if let Ok(exe) = startup::current_executable() {
        let path = exe.display().to_string();
        if path.contains("/target/debug/") || path.contains("/target/release/") {
            println!(
                "warning: this is a build-tree binary; unsigned or ad-hoc rebuilds can \
                 invalidate its Accessibility approval. Install a consistently signed copy \
                 outside target/ for reliable login launches."
            );
        }
    }
}

fn startup_status(options: StartupStatusOptions) -> AppResult<()> {
    let report = load_startup_report()?;
    match options.format {
        OutputFormat::Text => print_startup_status(&report),
        OutputFormat::Json => print_startup_status_json(&report),
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct StartupReport {
    status: startup::StartupStatus,
    config_start_at_login: bool,
    config_exists: bool,
    config_path: String,
}

impl StartupReport {
    fn in_sync(&self) -> bool {
        self.config_start_at_login == self.status.installed
            && (!self.status.installed || self.status.configured_for_current_exe)
    }

    fn sync_warning(&self) -> Option<&'static str> {
        if self.config_start_at_login
            && self.status.installed
            && !self.status.configured_for_current_exe
        {
            return Some(
                "warning: LaunchAgent points at a different binary; run enable-startup to repair",
            );
        }

        if self.config_start_at_login != self.status.installed {
            return Some(
                "warning: config and LaunchAgent state differ; run enable-startup or disable-startup",
            );
        }

        None
    }
}

fn load_startup_report() -> AppResult<StartupReport> {
    let store = ConfigStore::default();
    let config_exists = store.exists();
    let config_start_at_login = if config_exists {
        store.load()?.start_at_login
    } else {
        AppConfig::default().start_at_login
    };
    let status = startup::status_for_current_executable()?;

    Ok(StartupReport {
        status,
        config_start_at_login,
        config_exists,
        config_path: store.path().display().to_string(),
    })
}

fn print_startup_status(report: &StartupReport) {
    println!("start at login: {}", report.status.summary());
    println!("config: {}", report.config_path);
    println!("config exists={}", report.config_exists);
    println!("config start_at_login={}", report.config_start_at_login);
    if let Some(warning) = report.sync_warning() {
        println!("{warning}");
    }
}

fn print_startup_status_json(report: &StartupReport) {
    println!("{{");
    println!("  \"installed\": {},", report.status.installed);
    println!(
        "  \"configured_for_current_exe\": {},",
        report.status.configured_for_current_exe
    );
    println!(
        "  \"agent_path\": \"{}\",",
        json_escape(&report.status.agent_path.display().to_string())
    );
    println!(
        "  \"config_start_at_login\": {},",
        report.config_start_at_login
    );
    println!("  \"config_exists\": {},", report.config_exists);
    println!(
        "  \"config_path\": \"{}\",",
        json_escape(&report.config_path)
    );
    println!("  \"in_sync\": {}", report.in_sync());
    println!("}}");
}

fn json_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            control if control.is_control() => {
                let _ = write!(escaped, "\\u{:04x}", control as u32);
            }
            other => escaped.push(other),
        }
    }
    escaped
}

fn list_devices() -> AppResult<()> {
    let config = ConfigStore::default().load_or_create()?;
    let observations = hid::list_pointing_device_observations()?;
    let catalog = build_device_catalog(&observations, &config.device_rules);

    if catalog.is_empty() {
        println!("no mouse-usage HID devices found");
    } else {
        for state in [
            DeviceState::Connected,
            DeviceState::Remembered,
            DeviceState::Unavailable,
        ] {
            let entries: Vec<_> = catalog
                .iter()
                .filter(|entry| entry.state == state)
                .collect();
            if entries.is_empty() {
                continue;
            }
            println!("{}:", device_state_code(state));
            for entry in entries {
                let identity = entry
                    .identity
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "no stable public identity".to_string());
                let transport = entry
                    .transport
                    .as_deref()
                    .map(|value| format!(" via {value}"))
                    .unwrap_or_default();
                println!("  {identity}  {}{transport}", entry.display_name);
            }
        }
    }

    println!();
    if config.device_rules.is_empty() {
        println!(
            "no device_rules configured; add a [[device_rules]] block with vendor_id/product_id \
             plus serial_number or location_id when shown above; without a qualifier the rule is \
             shared by that whole hardware model"
        );
    } else {
        println!("configured device rules:");
        for rule in &config.device_rules {
            let direction = match rule.reverse {
                Some(true) => "reverse",
                Some(false) => "do-not-reverse",
                None => "inherit",
            };
            println!(
                "  {} direction={direction}{}{}",
                rule.selector_description(),
                rule.name
                    .as_deref()
                    .map(|n| format!("  # {n}"))
                    .unwrap_or_default(),
                rule.alias
                    .as_deref()
                    .map(|alias| format!("  alias={alias:?}"))
                    .unwrap_or_default(),
            );
        }
    }
    println!(
        "note: rules apply to discrete wheel scrolling only - trackpad and Magic Mouse \
         continuous scrolling cannot be attributed to a specific device"
    );
    match hid::continuous_source_hint() {
        Ok(hint) => println!("continuous-device classifier hint: {}", hint.description()),
        Err(error) => println!("continuous-device classifier hint unavailable: {error}"),
    }
    Ok(())
}

fn device_state_code(state: DeviceState) -> &'static str {
    match state {
        DeviceState::Connected => "connected",
        DeviceState::Remembered => "remembered",
        DeviceState::Unavailable => "unavailable",
    }
}

#[cfg(feature = "gui")]
fn launch_ui() -> AppResult<()> {
    auto_reverse::ui::run_settings_window().map_err(AppError::Platform)
}

#[cfg(feature = "gui")]
fn launch_benchmark() -> AppResult<()> {
    auto_reverse::ui::run_benchmark_window().map_err(AppError::Platform)
}

#[cfg(not(feature = "gui"))]
fn launch_ui() -> AppResult<()> {
    Err(AppError::Usage(
        "this build has no GUI; rebuild without --no-default-features to enable `ui`".to_string(),
    ))
}

#[cfg(not(feature = "gui"))]
fn launch_benchmark() -> AppResult<()> {
    Err(AppError::Usage(
        "this build has no GUI; rebuild without --no-default-features to enable `benchmark`"
            .to_string(),
    ))
}

fn show_config() -> AppResult<()> {
    let store = ConfigStore::default();
    let config = store.load_or_create()?;
    let contents = toml::to_string_pretty(&config).map_err(AppError::ConfigSerialize)?;
    println!("{contents}");
    Ok(())
}

fn simulate(options: SimulateOptions) -> AppResult<()> {
    // The live tap can never attribute a continuous event to a device, so
    // simulating that combination would let the debugging tool imply device
    // rules work for trackpad/Magic Mouse scrolling. Drop the hardware and
    // say so, rather than producing a decision that cannot happen for real.
    let identity = if options.continuous && options.identity.is_some() {
        println!(
            "note: ignoring device identity flags because --continuous true; the real tap cannot \
             attribute continuous scrolling to a device"
        );
        None
    } else {
        options.identity
    };

    let config = ConfigStore::default().load_or_create()?;
    let event = ScrollEvent {
        synthetic: options.synthetic,
        source_pid: options.source_pid,
        identity: identity.map(Arc::new),
        ..ScrollEvent::new(
            options.device_kind,
            options.delta_vertical,
            options.delta_horizontal,
            options.continuous,
        )
    };
    let decision = scroll::transform_event(&config, event);

    println!("original:    {}", decision.original);
    println!("transformed: {}", decision.transformed);
    println!("changed:     {}", decision.changed());
    println!("reversed:    {}", decision.reversed);
    println!("step_size:   {}", decision.step_size_applied);
    Ok(())
}

fn trace_lab(options: TraceLabOptions) -> AppResult<()> {
    let trace = load_trace(&options.trace_path)?;
    let store = ConfigStore::default();
    let (config, config_state) = load_config_for_diagnostics(&store, false)?;
    let report = scroll_lab::analyze(&trace, &config, options.lab)
        .map_err(|error| AppError::Usage(format!("trace-lab: {error}")))?;

    println!("Auto Reverse trace lab");
    println!("trace: {}", options.trace_path.display());
    println!("schema version: {}", report.schema_version);
    println!("config: {}", config_state.summary(store.path()));
    println!(
        "samples: {} (discrete={}, continuous={})",
        report.sample_count, report.discrete_sample_count, report.continuous_sample_count
    );
    println!("duration: {} us", report.duration_us);
    println!(
        "clutch sessions: {} (gap > {} us)",
        report.session_count, report.clutch_gap_us
    );
    println!("direction changes: {}", report.direction_change_count);
    print_distribution("input magnitude", Some(report.magnitude));
    print_distribution("event interval", report.intervals);
    if report.event_rates.is_empty() {
        println!("observed event rates: unavailable (fewer than two timestamps per device type)");
    } else {
        println!("observed event rates (not advertised polling rates):");
        for rate in &report.event_rates {
            print_event_rate(rate);
        }
    }
    println!(
        "replay matches observed: {}/{} ({:.1}%)",
        report.replay_match_count,
        report.sample_count,
        report.replay_match_percent()
    );
    println!(
        "samples requiring omitted identity/runtime context: {}",
        report.omitted_context_count
    );
    println!(
        "constant-gain baseline: {}x (discrete reversed axes only)",
        report.baseline_gain
    );
    print_axis_metrics("vertical", &report.vertical);
    print_axis_metrics("horizontal", &report.horizontal);
    Ok(())
}

fn load_trace(path: &Path) -> AppResult<ScrollTrace> {
    let file = File::open(path).map_err(|error| AppError::io("open trace", path, error))?;
    let mut contents = String::new();
    file.take((MAX_TRACE_BYTES + 1) as u64)
        .read_to_string(&mut contents)
        .map_err(|error| AppError::io("read trace", path, error))?;
    if contents.len() > MAX_TRACE_BYTES {
        return Err(AppError::Trace {
            path: path.to_path_buf(),
            source: Box::new(TraceError::TooManyBytes {
                actual: contents.len(),
                maximum: MAX_TRACE_BYTES,
            }),
        });
    }
    ScrollTrace::from_toml(&contents).map_err(|source| AppError::Trace {
        path: path.to_path_buf(),
        source: Box::new(source),
    })
}

fn print_distribution(label: &str, distribution: Option<Distribution>) {
    match distribution {
        Some(distribution) => println!(
            "{label}: min={} p50={} p95={} max={}",
            distribution.min, distribution.p50, distribution.p95, distribution.max
        ),
        None => println!("{label}: unavailable (one event timestamp)"),
    }
}

fn print_axis_metrics(label: &str, metrics: &AxisMetrics) {
    println!(
        "{label}: samples={} input[signed={}, abs={}] observed[signed={}, abs={}] \
         replay[signed={}, abs={}] baseline[signed={}, abs={}]",
        metrics.sample_count,
        metrics.input_signed_distance,
        metrics.input_absolute_distance,
        metrics.observed_signed_distance,
        metrics.observed_absolute_distance,
        metrics.replayed_signed_distance,
        metrics.replayed_absolute_distance,
        metrics.baseline_signed_distance,
        metrics.baseline_absolute_distance,
    );
}

fn print_event_rate(rate: &DeviceEventRate) {
    println!(
        "  {}: timestamps={} p50={:.1}Hz p95={:.1}Hz max={:.1}Hz bins[<30={},30-60={},60-120={},120-240={},240+={}]",
        rate.device_kind,
        rate.timestamp_count,
        millihertz_to_hertz(rate.rates_millihz.p50),
        millihertz_to_hertz(rate.rates_millihz.p95),
        millihertz_to_hertz(rate.rates_millihz.max),
        rate.histogram.below_30_hz,
        rate.histogram.from_30_to_60_hz,
        rate.histogram.from_60_to_120_hz,
        rate.histogram.from_120_to_240_hz,
        rate.histogram.at_least_240_hz,
    );
}

fn config_summary(config: &AppConfig) -> String {
    // Field labels intentionally match AppConfig's real field names exactly
    // (not shortened) so this line can be grepped/cross-referenced against
    // config/schema.rs and the "known gap" notes above without a mental
    // rename.
    format!(
        "enabled={}, reverse_vertical={}, reverse_horizontal={}, reverse_mouse={}, \
         reverse_trackpad={}, reverse_magic_mouse={}, reverse_unknown={}, \
         discrete_scroll_step_size={}, start_at_login={}, show_menu_bar_icon={}, \
         reverse_only_raw_input={}, device_rules={}",
        config.enabled,
        config.reverse_vertical,
        config.reverse_horizontal,
        config.reverse_mouse,
        config.reverse_trackpad,
        config.reverse_magic_mouse,
        config.reverse_unknown,
        config.discrete_scroll_step_size,
        config.start_at_login,
        config.show_menu_bar_icon,
        config.reverse_only_raw_input,
        config.device_rules.len(),
    )
}

fn print_help() {
    println!(
        "Auto Reverse\n\
         \n\
         Everyday commands:\n\
           run                         Start the macOS scroll event tap, headless (no window)\n\
           ui                          Open the settings window; also starts scroll reversal\n\
                                       in this same process (a background thread, sharing a\n\
                                       live-reloadable config with the window) and shows a\n\
                                       menu-bar icon for as long as the process runs\n\
           enable                      Turn scroll reversing on\n\
           disable                     Turn scroll reversing off\n\
           toggle                      Flip scroll reversing on/off\n\
           enable-startup              Start Auto Reverse at login\n\
           disable-startup             Stop starting Auto Reverse at login\n\
           show-menu-bar-icon          Restore a hidden menu-bar icon and reopen settings\n\
           startup-status [--json]     Show LaunchAgent startup status\n\
           doctor [--no-create]        Show status, config, and permissions\n\
           help                        Show this help\n\
         \n\
         Advanced commands:\n\
           prepare-uninstall           Remove startup registrations before app deletion\n\
           rollback-dynamics           Emergency rollback of global/per-device smooth presets\n\
           devices                     List connected pointing devices and per-device rules\n\
           benchmark                   Open the interactive scroll benchmark\n\
           init                        Create the default config if it does not exist\n\
           validate-config [--json]    Validate without creating config or lock files\n\
           repair-config              Preserve an invalid config and replace it with defaults\n\
           config-path                 Print the config file path\n\
           show-config                 Print the current config as TOML\n\
           open-releases [--latest|--all]\n\
                                       Open the canonical GitHub releases page; no background\n\
                                       update requests are made\n\
           simulate [flags]            Debugging tool: run one synthetic scroll event\n\
                                       through the rules without touching real hardware\n\
           trace-lab <trace.toml>       Replay a privacy trace and compare transfer metrics\n\
             [--baseline-gain 1..100] [--clutch-gap-ms 1..60000]\n\
         \n\
         Simulate flags:\n\
           --device mouse|trackpad|magic-mouse|unknown\n\
           --dy <integer>\n\
           --dx <integer>\n\
           --continuous true|false|yes|no|1|0\n\
           --synthetic true|false|yes|no|1|0\n\
           --source-pid <integer>\n\
           --vendor-id <integer|0xHEX>   (with --product-id: test a device rule)\n\
           --product-id <integer|0xHEX>\n\
           --serial-number <text>         (optional exact-device qualifier)\n\
           --location-id <integer|0xHEX>  (optional connection-port fallback)"
    );
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn startup_report(
        config_start_at_login: bool,
        installed: bool,
        configured_for_current_exe: bool,
    ) -> StartupReport {
        StartupReport {
            status: startup::StartupStatus {
                installed,
                agent_path: PathBuf::from("/tmp/com.auto-reverse.agent.plist"),
                configured_for_current_exe,
            },
            config_start_at_login,
            config_exists: true,
            config_path: "/tmp/config.toml".to_string(),
        }
    }

    #[test]
    fn startup_report_requires_current_binary_when_enabled() {
        let report = startup_report(true, true, false);

        assert!(!report.in_sync());
        assert_eq!(
            report.sync_warning(),
            Some("warning: LaunchAgent points at a different binary; run enable-startup to repair")
        );
    }

    #[test]
    fn startup_report_disabled_config_and_missing_agent_are_in_sync() {
        let report = startup_report(false, false, false);

        assert!(report.in_sync());
        assert_eq!(report.sync_warning(), None);
    }

    #[test]
    fn app_bundle_without_args_launches_ui() {
        assert_eq!(
            command_for_launch(
                &[],
                Path::new("/Applications/Auto Reverse.app/Contents/MacOS/auto-reverse")
            )
            .unwrap(),
            Command::Ui
        );
    }

    #[test]
    fn app_bundle_with_args_keeps_cli_command() {
        assert_eq!(
            command_for_launch(
                &[String::from("doctor")],
                Path::new("/Applications/Auto Reverse.app/Contents/MacOS/auto-reverse")
            )
            .unwrap(),
            Command::Doctor(DoctorOptions::default())
        );
    }

    #[test]
    fn terminal_without_args_keeps_headless_run_default() {
        assert_eq!(
            command_for_launch(&[], Path::new("/usr/local/bin/auto-reverse")).unwrap(),
            Command::Run
        );
    }
}
