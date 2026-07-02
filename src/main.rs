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
use std::fmt::Write as _;
use std::path::Path;
use std::process;

use auto_reverse::config::{AppConfig, ConfigStore};
use auto_reverse::device;
use auto_reverse::error::{AppError, AppResult};
use auto_reverse::input::ScrollEvent;
use auto_reverse::platform::macos::{event_tap, permissions, startup};
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
    match cli::parse_args(&args)? {
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
        Command::Help => {
            print_help();
            Ok(())
        }
    }
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
        "auto-reverse: config changes made while this is running have no effect until restart"
    );

    event_tap::install_and_run(config)
}

fn doctor(options: DoctorOptions) -> AppResult<()> {
    let store = ConfigStore::default();
    let (config, config_state) = load_config_for_diagnostics(&store, options.create_config)?;

    let accessibility = permissions::has_accessibility_trust();
    let input_monitoring = permissions::has_input_monitoring_access();
    let current_exe = startup::current_executable()?;
    let startup_status = startup::status_for_executable(&current_exe)?;
    let status = if !config.enabled {
        "OFF (disabled in config)"
    } else if !accessibility || !input_monitoring {
        "NEEDS PERMISSION"
    } else {
        "ON"
    };

    println!("Auto Reverse doctor");
    println!("status: {status}");
    println!("what it's doing: {}", plain_english_summary(&config));
    println!();
    println!("version: {}", env!("CARGO_PKG_VERSION"));
    println!("binary: {}", current_exe.display());
    println!("config: {}", config_state.summary(store.path()));
    println!("settings: {}", config_summary(&config));
    println!(
        "start at login: {} (config start_at_login={})",
        startup_status.summary(),
        config.start_at_login
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
         show_discrete_scroll_options are reserved for a future menu-bar app and have no effect \
         in this CLI-only build"
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

fn plain_english_summary(config: &AppConfig) -> String {
    if !config.enabled {
        return "not reversing anything right now (disabled)".to_string();
    }

    let mut targets = Vec::new();
    if config.reverse_mouse {
        targets.push("a physical mouse wheel");
    }
    if config.reverse_trackpad {
        targets.push("trackpad scrolling (this also covers a real Magic Mouse - see below)");
    }
    if targets.is_empty() {
        return "enabled, but no device is currently set to reverse".to_string();
    }

    let axes = match (config.reverse_vertical, config.reverse_horizontal) {
        (true, true) => "vertical and horizontal",
        (true, false) => "vertical",
        (false, true) => "horizontal",
        (false, false) => "no axis - nothing will actually flip",
    };
    format!("reversing {axes} scroll for {}", targets.join(" and "))
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
    let mut config = store.load_or_create()?;
    config.start_at_login = enabled;
    store.save(&config)?;

    println!("start at login: {}", status.summary());
    if enabled {
        println!("auto-reverse will start on the next login using the current binary path");
    }
    Ok(())
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
        self.config_start_at_login == self.status.enabled
            && (!self.status.enabled || self.status.configured_for_current_exe)
    }

    fn sync_warning(&self) -> Option<&'static str> {
        if self.config_start_at_login
            && self.status.enabled
            && !self.status.configured_for_current_exe
        {
            return Some(
                "warning: LaunchAgent points at a different binary; run enable-startup to repair",
            );
        }

        if self.config_start_at_login != self.status.enabled {
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
    println!("  \"enabled\": {},", report.status.enabled);
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

fn show_config() -> AppResult<()> {
    let store = ConfigStore::default();
    let config = store.load_or_create()?;
    let contents = toml::to_string_pretty(&config).map_err(AppError::ConfigSerialize)?;
    println!("{contents}");
    Ok(())
}

fn simulate(options: SimulateOptions) -> AppResult<()> {
    let config = ConfigStore::default().load_or_create()?;
    let event = ScrollEvent {
        synthetic: options.synthetic,
        source_pid: options.source_pid,
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
         discrete_scroll_step_size={}, start_at_login={}, reverse_only_raw_input={}",
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
    )
}

fn print_help() {
    println!(
        "Auto Reverse\n\
         \n\
         Everyday commands:\n\
           run                         Start the macOS scroll event tap\n\
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
           --source-pid <integer>"
    );
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn startup_report(
        config_start_at_login: bool,
        enabled: bool,
        configured_for_current_exe: bool,
    ) -> StartupReport {
        StartupReport {
            status: startup::StartupStatus {
                enabled,
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
}
