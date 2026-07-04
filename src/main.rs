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
use std::path::Path;
use std::process;
use std::sync::{Arc, RwLock};

use auto_reverse::config::{AppConfig, ConfigStore};
use auto_reverse::device;
use auto_reverse::error::{AppError, AppResult};
use auto_reverse::input::ScrollEvent;
use auto_reverse::platform::macos::{event_tap, hid, permissions, startup};
use auto_reverse::scroll;
use cli::{Command, DoctorOptions, OutputFormat, SimulateOptions, StartupStatusOptions};

fn main() {
    if let Err(error) = run() {
        eprintln!("auto-reverse: {error}");
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
        Command::StartupStatus(options) => startup_status(options),
        Command::ConfigPath => {
            println!("{}", ConfigStore::default_path().display());
            Ok(())
        }
        Command::ShowConfig => show_config(),
        Command::Simulate(options) => simulate(options),
        Command::Devices => list_devices(),
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

    if !permissions::request_missing_permissions() {
        permissions::print_permission_help();
        return Err(AppError::Platform(
            "Accessibility or Input Monitoring permission is not granted".to_string(),
        ));
    }

    println!(
        "auto-reverse: config changes made while this is running have no effect until restart \
         (this headless `run` process does not watch the config file for changes; the merged \
         `ui` process does, via its shared in-memory config)"
    );

    // install_and_run acquires the exclusive daemon_lock itself, as the
    // very first thing it does, before touching the HID monitor or the
    // CGEventTap - this is the one gate shared with the merged `ui`
    // process's in-process tap thread, so the two launch paths can never
    // both hold a live tap. A second, redundant `run` (or a `ui` process
    // already running) observes the lock already held and returns cleanly
    // rather than installing a competing tap.
    event_tap::install_and_run(Arc::new(RwLock::new(config)))
}

fn doctor(options: DoctorOptions) -> AppResult<()> {
    let store = ConfigStore::default();
    let (config, config_state) = load_config_for_diagnostics(&store, options.create_config)?;

    let accessibility = permissions::has_accessibility_trust();
    let input_monitoring = permissions::has_input_monitoring_access();
    let current_exe = startup::current_executable()?;
    // Deliberately not `?`: an unreadable LaunchAgent must not suppress the
    // rest of the diagnostics - permissions are what doctor exists to show.
    let startup_summary = match startup::status_for_executable(&current_exe) {
        Ok(status) => status.summary(),
        Err(error) => format!("could not determine ({error})"),
    };
    let status = if !config.enabled {
        "OFF (disabled in config)"
    } else if !accessibility || !input_monitoring {
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
        "input monitoring permission: {}",
        permissions::permission_status(input_monitoring)
    );
    println!("device classifier: {}", device::CLASSIFIER_DESCRIPTION);
    println!(
        "known gap: reverse_magic_mouse and reverse_unknown have no effect yet - the classifier \
         above can only ever produce Mouse or Trackpad, so a real Magic Mouse is governed by \
         reverse_trackpad and DeviceKind::Unknown is unreachable outside `simulate`"
    );
    println!(
        "known gap: show_menu_bar_icon, check_for_updates, include_beta_updates, and \
         show_discrete_scroll_options are stored for planned UI/updater behavior but are not \
         applied by the runtime yet"
    );

    if !accessibility || !input_monitoring {
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
        store.save(&config)?;
        Ok((config, ConfigDiagnosticState::Created))
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

    store.save(&AppConfig::default())?;
    println!("created config: {}", store.path().display());
    Ok(())
}

fn set_enabled(enabled: bool) -> AppResult<()> {
    let store = ConfigStore::default();
    let mut config = store.load_or_create()?;
    config.enabled = enabled;
    store.save(&config)?;
    println!(
        "auto-reverse is now {}",
        if enabled { "enabled" } else { "disabled" }
    );
    Ok(())
}

fn toggle_enabled() -> AppResult<()> {
    let store = ConfigStore::default();
    let mut config = store.load_or_create()?;
    config.enabled = !config.enabled;
    let enabled = config.enabled;
    store.save(&config)?;
    println!(
        "auto-reverse is now {}",
        if enabled { "enabled" } else { "disabled" }
    );
    Ok(())
}

fn set_startup_enabled(enabled: bool) -> AppResult<()> {
    let status = if enabled {
        startup::enable_for_current_executable()?
    } else {
        startup::disable()?
    };

    let store = ConfigStore::default();
    let config_result = store.load_or_create().and_then(|mut config| {
        config.start_at_login = enabled;
        store.save(&config)
    });
    if let Err(error) = config_result {
        // The launch agent was already changed above; without this note the
        // user has no way to know config and agent now disagree.
        eprintln!(
            "auto-reverse: the launch agent was updated, but saving the config failed; \
             rerun this command to bring them back in sync"
        );
        return Err(error);
    }

    println!("start at login: {}", status.summary());
    if enabled {
        println!("auto-reverse will start on the next login using the current binary path");
        warn_if_dev_tree_binary();
    }
    Ok(())
}

/// A LaunchAgent pointing into target/ is fragile: every rebuild changes
/// the binary's identity, so the TCC grants stop matching and the login
/// launch fails. Warn instead of refusing - it is still the right workflow
/// for trying the feature out.
fn warn_if_dev_tree_binary() {
    if let Ok(exe) = startup::current_executable() {
        let path = exe.display().to_string();
        if path.contains("/target/debug/") || path.contains("/target/release/") {
            println!(
                "warning: this is a build-tree binary; every rebuild invalidates its \
                 Accessibility/Input Monitoring approval, so the login launch will fail \
                 until re-approved. Consider installing a stable copy outside target/."
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
    let devices = hid::list_pointing_devices()?;

    if devices.is_empty() {
        println!("no mouse-usage HID devices found");
    } else {
        println!("connected pointing devices:");
        for device in &devices {
            let rule = config
                .device_rules
                .iter()
                .find(|rule| rule.matches(device.hardware));
            let rule_note = match rule {
                Some(rule) if rule.reverse => "  [rule: reverse]",
                Some(_) => "  [rule: do not reverse]",
                None => "",
            };
            println!(
                "  {}  {}{}{}",
                device.hardware,
                device.name.as_deref().unwrap_or("(unnamed)"),
                device
                    .transport
                    .as_deref()
                    .map(|t| format!(" via {t}"))
                    .unwrap_or_default(),
                rule_note,
            );
        }
    }

    println!();
    if config.device_rules.is_empty() {
        println!(
            "no device_rules configured; add a [[device_rules]] block with vendor_id/product_id \
             from the list above to pin one device's direction"
        );
    } else {
        println!("configured device rules:");
        for rule in &config.device_rules {
            println!(
                "  vendor_id=0x{:04x} product_id=0x{:04x} reverse={}{}",
                rule.vendor_id,
                rule.product_id,
                rule.reverse,
                rule.name
                    .as_deref()
                    .map(|n| format!("  # {n}"))
                    .unwrap_or_default(),
            );
        }
    }
    println!(
        "note: rules apply to discrete wheel scrolling only - trackpad and Magic Mouse \
         continuous scrolling cannot be attributed to a specific device"
    );
    Ok(())
}

#[cfg(feature = "gui")]
fn launch_ui() -> AppResult<()> {
    auto_reverse::ui::run_settings_window().map_err(AppError::Platform)
}

#[cfg(not(feature = "gui"))]
fn launch_ui() -> AppResult<()> {
    Err(AppError::Usage(
        "this build has no GUI; rebuild without --no-default-features to enable `ui`".to_string(),
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
    let hardware = if options.continuous && options.hardware.is_some() {
        println!(
            "note: ignoring --vendor-id/--product-id because --continuous true; the real tap \
             cannot attribute continuous scrolling to a device"
        );
        None
    } else {
        options.hardware
    };

    let config = ConfigStore::default().load_or_create()?;
    let event = ScrollEvent {
        synthetic: options.synthetic,
        source_pid: options.source_pid,
        hardware,
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

fn config_summary(config: &AppConfig) -> String {
    // Field labels intentionally match AppConfig's real field names exactly
    // (not shortened) so this line can be grepped/cross-referenced against
    // config/schema.rs and the "known gap" notes above without a mental
    // rename.
    format!(
        "enabled={}, reverse_vertical={}, reverse_horizontal={}, reverse_mouse={}, \
         reverse_trackpad={}, reverse_magic_mouse={}, reverse_unknown={}, \
         discrete_scroll_step_size={}, start_at_login={}, reverse_only_raw_input={}, \
         device_rules={}",
        config.enabled,
        config.reverse_vertical,
        config.reverse_horizontal,
        config.reverse_mouse,
        config.reverse_trackpad,
        config.reverse_magic_mouse,
        config.reverse_unknown,
        config.discrete_scroll_step_size,
        config.start_at_login,
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
           startup-status [--json]     Show LaunchAgent startup status\n\
           doctor [--no-create]        Show status, config, and permissions\n\
           help                        Show this help\n\
         \n\
         Advanced commands:\n\
           devices                     List connected pointing devices and per-device rules\n\
           init                        Create the default config if it does not exist\n\
           config-path                 Print the config file path\n\
           show-config                 Print the current config as TOML\n\
           simulate [flags]            Debugging tool: run one synthetic scroll event\n\
                                       through the rules without touching real hardware\n\
         \n\
         Simulate flags:\n\
           --device mouse|trackpad|magic-mouse|unknown\n\
           --dy <integer>\n\
           --dx <integer>\n\
           --continuous true|false|yes|no|1|0\n\
           --synthetic true|false|yes|no|1|0\n\
           --source-pid <integer>\n\
           --vendor-id <integer|0xHEX>   (with --product-id: test a device rule)\n\
           --product-id <integer|0xHEX>"
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
