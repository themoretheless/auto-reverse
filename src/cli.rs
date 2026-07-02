use auto_reverse::device::{DeviceKind, HardwareId};
use auto_reverse::error::{AppError, AppResult};

pub const BOOL_HELP_VALUES: &str = "true|false|yes|no|1|0";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Text,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Run,
    Doctor(DoctorOptions),
    Init,
    Enable,
    Disable,
    Toggle,
    EnableStartup,
    DisableStartup,
    StartupStatus(StartupStatusOptions),
    ConfigPath,
    ShowConfig,
    Simulate(SimulateOptions),
    Devices,
    Ui,
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DoctorOptions {
    pub create_config: bool,
}

impl Default for DoctorOptions {
    fn default() -> Self {
        Self {
            create_config: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartupStatusOptions {
    pub format: OutputFormat,
}

impl Default for StartupStatusOptions {
    fn default() -> Self {
        Self {
            format: OutputFormat::Text,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimulateOptions {
    pub device_kind: DeviceKind,
    pub delta_vertical: i64,
    pub delta_horizontal: i64,
    pub continuous: bool,
    pub synthetic: bool,
    pub source_pid: i64,
    pub hardware: Option<HardwareId>,
}

impl Default for SimulateOptions {
    fn default() -> Self {
        Self {
            device_kind: DeviceKind::Mouse,
            delta_vertical: 1,
            delta_horizontal: 0,
            continuous: false,
            synthetic: false,
            source_pid: 0,
            hardware: None,
        }
    }
}

pub fn parse_args(args: &[String]) -> AppResult<Command> {
    match args.first().map(String::as_str) {
        None | Some("run") => Ok(Command::Run),
        Some("doctor") => parse_doctor(&args[1..]),
        Some("init") => Ok(Command::Init),
        Some("enable") => Ok(Command::Enable),
        Some("disable") => Ok(Command::Disable),
        Some("toggle") => Ok(Command::Toggle),
        Some("enable-startup") => Ok(Command::EnableStartup),
        Some("disable-startup") => Ok(Command::DisableStartup),
        Some("startup-status") => parse_startup_status(&args[1..]),
        Some("config-path") => Ok(Command::ConfigPath),
        Some("show-config") => Ok(Command::ShowConfig),
        Some("simulate") => parse_simulate(&args[1..]).map(Command::Simulate),
        Some("devices") => Ok(Command::Devices),
        Some("ui") => Ok(Command::Ui),
        Some("help" | "--help" | "-h") => Ok(Command::Help),
        Some(command) => Err(AppError::Usage(format!(
            "unknown command `{command}`; run `auto-reverse help`"
        ))),
    }
}

fn parse_doctor(args: &[String]) -> AppResult<Command> {
    let mut options = DoctorOptions::default();
    for arg in args {
        match arg.as_str() {
            "--no-create" => options.create_config = false,
            "--create" => options.create_config = true,
            "--help" | "-h" => return Ok(Command::Help),
            flag => {
                return Err(AppError::Usage(format!(
                    "unknown doctor flag `{flag}`; run `auto-reverse help`"
                )));
            }
        }
    }
    Ok(Command::Doctor(options))
}

fn parse_startup_status(args: &[String]) -> AppResult<Command> {
    let mut options = StartupStatusOptions::default();
    for arg in args {
        match arg.as_str() {
            "--json" => options.format = OutputFormat::Json,
            "--text" => options.format = OutputFormat::Text,
            "--help" | "-h" => return Ok(Command::Help),
            flag => {
                return Err(AppError::Usage(format!(
                    "unknown startup-status flag `{flag}`; run `auto-reverse help`"
                )));
            }
        }
    }
    Ok(Command::StartupStatus(options))
}

fn parse_simulate(args: &[String]) -> AppResult<SimulateOptions> {
    let mut options = SimulateOptions::default();
    let mut vendor_id: Option<u32> = None;
    let mut product_id: Option<u32> = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--device" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| AppError::Usage("--device needs a value".to_string()))?;
                options.device_kind = value.parse().map_err(AppError::Usage)?;
            }
            "--dy" | "--delta-y" => {
                index += 1;
                options.delta_vertical = parse_i64(args.get(index), "--dy")?;
            }
            "--dx" | "--delta-x" => {
                index += 1;
                options.delta_horizontal = parse_i64(args.get(index), "--dx")?;
            }
            "--continuous" => {
                index += 1;
                options.continuous = parse_bool(args.get(index), "--continuous")?;
            }
            "--synthetic" => {
                index += 1;
                options.synthetic = parse_bool(args.get(index), "--synthetic")?;
            }
            "--source-pid" => {
                index += 1;
                options.source_pid = parse_i64(args.get(index), "--source-pid")?;
            }
            "--vendor-id" => {
                index += 1;
                vendor_id = Some(parse_u32(args.get(index), "--vendor-id")?);
            }
            "--product-id" => {
                index += 1;
                product_id = Some(parse_u32(args.get(index), "--product-id")?);
            }
            flag => {
                return Err(AppError::Usage(format!(
                    "unknown simulate flag `{flag}`; run `auto-reverse help`"
                )));
            }
        }
        index += 1;
    }

    options.hardware = match (vendor_id, product_id) {
        (Some(vendor_id), Some(product_id)) => Some(HardwareId {
            vendor_id,
            product_id,
        }),
        (None, None) => None,
        _ => {
            return Err(AppError::Usage(
                "--vendor-id and --product-id must be given together".to_string(),
            ));
        }
    };

    Ok(options)
}

fn parse_i64(value: Option<&String>, flag: &str) -> AppResult<i64> {
    value
        .ok_or_else(|| AppError::Usage(format!("{flag} needs a value")))?
        .parse()
        .map_err(|_| AppError::Usage(format!("{flag} must be an integer")))
}

/// Accepts both decimal and 0x-prefixed hex, since `devices` prints IDs in
/// hex and lsusb-style docs quote them that way.
fn parse_u32(value: Option<&String>, flag: &str) -> AppResult<u32> {
    let value = value.ok_or_else(|| AppError::Usage(format!("{flag} needs a value")))?;
    let parsed = match value.strip_prefix("0x").or_else(|| value.strip_prefix("0X")) {
        Some(hex) => u32::from_str_radix(hex, 16),
        None => value.parse(),
    };
    parsed.map_err(|_| AppError::Usage(format!("{flag} must be an integer like 1133 or 0x046d")))
}

fn parse_bool(value: Option<&String>, flag: &str) -> AppResult<bool> {
    match value.map(String::as_str) {
        Some("true" | "yes" | "1") => Ok(true),
        Some("false" | "no" | "0") => Ok(false),
        Some(other) => Err(AppError::Usage(format!(
            "{flag} must be one of {BOOL_HELP_VALUES}, got `{other}`"
        ))),
        None => Err(AppError::Usage(format!("{flag} needs a value"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

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

    #[test]
    fn doctor_no_create_is_explicit() {
        let command = parse_args(&strings(&["doctor", "--no-create"])).unwrap();

        assert_eq!(
            command,
            Command::Doctor(DoctorOptions {
                create_config: false
            })
        );
    }

    #[test]
    fn startup_status_accepts_json_format() {
        let command = parse_args(&strings(&["startup-status", "--json"])).unwrap();

        assert_eq!(
            command,
            Command::StartupStatus(StartupStatusOptions {
                format: OutputFormat::Json
            })
        );
    }

    #[test]
    fn simulate_parses_every_flag() {
        let command = parse_args(&strings(&[
            "simulate",
            "--device",
            "trackpad",
            "--dy",
            "-12",
            "--dx",
            "3",
            "--continuous",
            "yes",
            "--synthetic",
            "1",
            "--source-pid",
            "42",
        ]))
        .unwrap();

        assert_eq!(
            command,
            Command::Simulate(SimulateOptions {
                device_kind: DeviceKind::Trackpad,
                delta_vertical: -12,
                delta_horizontal: 3,
                continuous: true,
                synthetic: true,
                source_pid: 42,
                hardware: None,
            })
        );
    }

    #[test]
    fn simulate_parses_hex_and_decimal_hardware_ids() {
        let command =
            parse_args(&strings(&["simulate", "--vendor-id", "0x046d", "--product-id", "1"]))
                .unwrap();

        assert_eq!(
            command,
            Command::Simulate(SimulateOptions {
                hardware: Some(HardwareId {
                    vendor_id: 0x046d,
                    product_id: 1,
                }),
                ..SimulateOptions::default()
            })
        );
    }

    #[test]
    fn simulate_rejects_one_hardware_id_without_the_other() {
        assert!(parse_args(&strings(&["simulate", "--vendor-id", "0x046d"])).is_err());
        assert!(parse_args(&strings(&["simulate", "--product-id", "0xc52b"])).is_err());
    }

    #[test]
    fn devices_and_ui_parse() {
        assert_eq!(parse_args(&strings(&["devices"])).unwrap(), Command::Devices);
        assert_eq!(parse_args(&strings(&["ui"])).unwrap(), Command::Ui);
    }

    #[test]
    fn unknown_subcommand_is_usage_error() {
        assert!(parse_args(&strings(&["wat"])).is_err());
    }
}
