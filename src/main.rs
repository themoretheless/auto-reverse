use std::env;
use std::process;

use auto_reverse::config::{AppConfig, ConfigStore};
use auto_reverse::device::DeviceKind;
use auto_reverse::error::{AppError, AppResult};
use auto_reverse::event_tap;
use auto_reverse::input::ScrollEvent;
use auto_reverse::permissions;
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

    if !permissions::is_trusted() {
        permissions::print_permission_help();
        return Err(AppError::Platform(
            "Accessibility permission is not granted".to_string(),
        ));
    }

    event_tap::install_and_run(config)
}

fn doctor() -> AppResult<()> {
    let store = ConfigStore::default();
    let config = store.load_or_create()?;

    println!("Auto Reverse doctor");
    println!("version: {}", env!("CARGO_PKG_VERSION"));
    println!("config: {}", store.path().display());
    println!("settings: {}", config_summary(&config));
    println!(
        "accessibility permission: {}",
        if permissions::is_trusted() {
            "granted"
        } else {
            "required"
        }
    );
    println!("input monitoring permission: checked when installing the event tap");
    println!("current macOS classifier: physical wheel = mouse, continuous scroll = trackpad-like");
    println!("known gap: Magic Mouse and trackpad are not separated until gesture tracking lands");
    Ok(())
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
            flag => {
                return Err(AppError::Usage(format!(
                    "unknown simulate flag `{flag}`; run `auto-reverse help`"
                )));
            }
        }
        index += 1;
    }

    let config = ConfigStore::default().load_or_create()?;
    let event = ScrollEvent::new(device_kind, delta_vertical, delta_horizontal, continuous);
    let decision = scroll::transform_event(&config, event);

    println!("original:    {:?}", decision.original);
    println!("transformed: {:?}", decision.transformed);
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
    format!(
        "enabled={}, vertical={}, horizontal={}, mouse={}, trackpad={}, magic_mouse={}, step_size={}",
        config.enabled,
        config.reverse_vertical,
        config.reverse_horizontal,
        config.reverse_mouse,
        config.reverse_trackpad,
        config.reverse_magic_mouse,
        config.discrete_scroll_step_size
    )
}

fn print_help() {
    println!(
        "Auto Reverse\n\
         \n\
         Commands:\n\
           run                         Start the macOS scroll event tap\n\
           doctor                      Show config, permission and platform status\n\
           init                        Create the default config if it does not exist\n\
           enable                      Enable scroll reversing in config\n\
           disable                     Disable scroll reversing in config\n\
           toggle                      Toggle scroll reversing in config\n\
           config-path                 Print the config file path\n\
           show-config                 Print the current config as TOML\n\
           simulate [flags]            Run one scroll event through the rules\n\
           help                        Show this help\n\
         \n\
         Simulate flags:\n\
           --device mouse|trackpad|magic-mouse|unknown\n\
           --dy <integer>\n\
           --dx <integer>\n\
           --continuous true|false"
    );
}
