// The library's pure core builds on any OS (cargo check --lib), but this
// binary drives a CGEventTap and is macOS-only. Without this guard a
// non-macOS build dies on a bare E0432 unresolved-import error; with it,
// the failure explains itself.
#[cfg(not(target_os = "macos"))]
compile_error!(
    "the auto-reverse binary is macOS-only; on other platforms build just the library with --lib"
);

use std::env;
use std::process;

use auto_reverse::config::{AppConfig, ConfigStore};
use auto_reverse::device;
use auto_reverse::device::DeviceKind;
use auto_reverse::error::{AppError, AppResult};
use auto_reverse::input::ScrollEvent;
use auto_reverse::platform::macos::{event_tap, permissions, startup};
use auto_reverse::scroll;

fn main() {
    if let Err(error) = run() {
        eprintln!("auto-reverse: {error}");
        process::exit(1);
    }
}

fn run() -> AppResult<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("run") => run_event_tap(),
        Some("doctor") => doctor(),
        Some("init") => init_config(),
        Some("enable") => set_enabled(true),
        Some("disable") => set_enabled(false),
        Some("toggle") => toggle_enabled(),
        Some("enable-startup") => set_startup_enabled(true),
        Some("disable-startup") => set_startup_enabled(false),
        Some("startup-status") => startup_status(),
        Some("config-path") => {
            println!("{}", ConfigStore::default_path().display());
            Ok(())
        }
        Some("show-config") => show_config(),
        Some("simulate") => simulate(&args[1..]),
        Some("help" | "--help" | "-h") => {
            print_help();
            Ok(())
        }
        Some(command) => Err(AppError::Usage(format!(
            "unknown command `{command}`; run `auto-reverse help`"
        ))),
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

fn doctor() -> AppResult<()> {
    let store = ConfigStore::default();
    let config = store.load_or_create()?;

    let accessibility = permissions::has_accessibility_trust();
    let input_monitoring = permissions::has_input_monitoring_access();
    let startup_status = startup::status_for_current_executable()?;
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
    println!("config: {}", store.path().display());
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

fn startup_status() -> AppResult<()> {
    let store = ConfigStore::default();
    let config_start_at_login = if store.exists() {
        store.load()?.start_at_login
    } else {
        AppConfig::default().start_at_login
    };
    let status = startup::status_for_current_executable()?;

    println!("start at login: {}", status.summary());
    println!("config start_at_login={config_start_at_login}");
    if config_start_at_login != status.enabled {
        println!(
            "warning: config and LaunchAgent state differ; run enable-startup or disable-startup"
        );
    }
    Ok(())
}

fn show_config() -> AppResult<()> {
    let store = ConfigStore::default();
    let config = store.load_or_create()?;
    let contents = toml::to_string_pretty(&config).map_err(AppError::ConfigSerialize)?;
    println!("{contents}");
    Ok(())
}

fn simulate(args: &[String]) -> AppResult<()> {
    let mut device_kind = DeviceKind::Mouse;
    let mut delta_vertical = 1;
    let mut delta_horizontal = 0;
    let mut continuous = false;
    let mut synthetic = false;
    let mut source_pid = 0;

    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--device" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| AppError::Usage("--device needs a value".to_string()))?;
                device_kind = value.parse().map_err(AppError::Usage)?;
            }
            "--dy" | "--delta-y" => {
                index += 1;
                delta_vertical = parse_i64(args.get(index), "--dy")?;
            }
            "--dx" | "--delta-x" => {
                index += 1;
                delta_horizontal = parse_i64(args.get(index), "--dx")?;
            }
            "--continuous" => {
                index += 1;
                continuous = parse_bool(args.get(index), "--continuous")?;
            }
            "--synthetic" => {
                index += 1;
                synthetic = parse_bool(args.get(index), "--synthetic")?;
            }
            "--source-pid" => {
                index += 1;
                source_pid = parse_i64(args.get(index), "--source-pid")?;
            }
            flag => {
                return Err(AppError::Usage(format!(
                    "unknown simulate flag `{flag}`; run `auto-reverse help`"
                )));
            }
        }
        index += 1;
    }

    let config = ConfigStore::default().load_or_create()?;
    let event = ScrollEvent {
        synthetic,
        source_pid,
        ..ScrollEvent::new(device_kind, delta_vertical, delta_horizontal, continuous)
    };
    let decision = scroll::transform_event(&config, event);

    println!("original:    {}", decision.original);
    println!("transformed: {}", decision.transformed);
    println!("changed:     {}", decision.changed());
    println!("reversed:    {}", decision.reversed);
    println!("step_size:   {}", decision.step_size_applied);
    Ok(())
}

fn parse_i64(value: Option<&String>, flag: &str) -> AppResult<i64> {
    value
        .ok_or_else(|| AppError::Usage(format!("{flag} needs a value")))?
        .parse()
        .map_err(|_| AppError::Usage(format!("{flag} must be an integer")))
}

fn parse_bool(value: Option<&String>, flag: &str) -> AppResult<bool> {
    match value.map(String::as_str) {
        Some("true" | "yes" | "1") => Ok(true),
        Some("false" | "no" | "0") => Ok(false),
        Some(other) => Err(AppError::Usage(format!(
            "{flag} must be true/false, got `{other}`"
        ))),
        None => Err(AppError::Usage(format!("{flag} needs a value"))),
    }
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
           startup-status              Show LaunchAgent startup status\n\
           doctor                      Show status, config, and permissions\n\
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
           --continuous true|false\n\
           --synthetic true|false\n\
           --source-pid <integer>"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_i64_rejects_missing_value() {
        assert!(parse_i64(None, "--dy").is_err());
    }

    #[test]
    fn parse_i64_rejects_non_integer_value() {
        assert!(parse_i64(Some(&"not-a-number".to_string()), "--dy").is_err());
    }

    #[test]
    fn parse_i64_accepts_negative_integers() {
        assert_eq!(parse_i64(Some(&"-7".to_string()), "--dy").unwrap(), -7);
    }

    #[test]
    fn parse_bool_accepts_all_documented_spellings() {
        for spelling in ["true", "yes", "1"] {
            assert!(parse_bool(Some(&spelling.to_string()), "--continuous").unwrap());
        }
        for spelling in ["false", "no", "0"] {
            assert!(!parse_bool(Some(&spelling.to_string()), "--continuous").unwrap());
        }
    }

    #[test]
    fn parse_bool_rejects_anything_else() {
        assert!(parse_bool(Some(&"maybe".to_string()), "--continuous").is_err());
        assert!(parse_bool(None, "--continuous").is_err());
    }
}
