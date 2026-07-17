#![cfg(target_os = "macos")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use auto_reverse::config::{AppConfig, DeviceRule};
use auto_reverse::device::{DeviceKind, HardwareId};
use auto_reverse::diagnostics::{Axis, DecisionReason};
use auto_reverse::scroll_dynamics::SmoothPreset;
use auto_reverse::scroll_trace::{ScrollTrace, TraceSample};

struct CliSandbox {
    home: PathBuf,
}

impl CliSandbox {
    fn new(name: &str) -> Self {
        let id = NEXT_SANDBOX_ID.fetch_add(1, Ordering::Relaxed);
        let home = std::env::temp_dir().join(format!(
            "auto-reverse-cli-{name}-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&home).unwrap();
        Self { home }
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_auto-reverse"));
        command
            .args(args)
            .env("HOME", &self.home)
            .env_remove("AUTO_REVERSE_CONFIG")
            .env_remove("AUTO_REVERSE_LAUNCH_AGENT_DIR")
            .env_remove("XDG_CONFIG_HOME");
        command
    }

    fn command_with_config(&self, args: &[&str], config_path: &Path) -> Command {
        let mut command = self.command(args);
        command.env("AUTO_REVERSE_CONFIG", config_path);
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        run_ok(self.command(args))
    }

    fn default_config_path(&self) -> PathBuf {
        self.home
            .join("Library")
            .join("Application Support")
            .join("Auto Reverse")
            .join("config.toml")
    }

    fn launch_agent_path(&self) -> PathBuf {
        self.home
            .join("Library")
            .join("LaunchAgents")
            .join("com.auto-reverse.agent.plist")
    }
}

impl Drop for CliSandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.home);
    }
}

fn run_ok(mut command: Command) -> Output {
    let output = command.output().unwrap();
    assert_success(&output);
    output
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "command failed with {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn read_config(path: &Path) -> AppConfig {
    toml::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn config_path_uses_isolated_home_without_creating_files() {
    let sandbox = CliSandbox::new("config-path");
    let config_path = sandbox.default_config_path();

    let output = sandbox.run(&["config-path"]);

    assert_eq!(stdout(&output).trim(), config_path.to_string_lossy());
    assert!(!config_path.exists());
    assert!(!config_path.with_file_name("config.toml.lock").exists());
}

#[test]
fn doctor_no_create_reports_defaults_without_creating_config() {
    let sandbox = CliSandbox::new("doctor-no-create");
    let config_path = sandbox.default_config_path();

    let output = sandbox.run(&["doctor", "--no-create"]);
    let stdout = stdout(&output);

    assert!(stdout.contains("missing; using defaults for this report"));
    assert!(stdout.contains(&config_path.to_string_lossy().to_string()));
    assert!(!config_path.exists());
    assert!(!config_path.with_file_name("config.toml.lock").exists());
}

#[test]
fn explicit_config_override_wins_over_home() {
    let sandbox = CliSandbox::new("config-override");
    let override_path = sandbox.home.join("override").join("custom.toml");

    run_ok(sandbox.command_with_config(&["disable"], &override_path));
    let path_output = run_ok(sandbox.command_with_config(&["config-path"], &override_path));

    assert_eq!(stdout(&path_output).trim(), override_path.to_string_lossy());
    assert!(!read_config(&override_path).enabled);
    assert!(!sandbox.default_config_path().exists());
    assert!(override_path.with_file_name("custom.toml.lock").exists());
}

#[test]
fn concurrent_cli_mutations_preserve_config_and_stay_in_home() {
    let sandbox = CliSandbox::new("concurrent-writes");
    let config_path = sandbox.default_config_path();
    sandbox.run(&["init"]);

    let mut disable_command = sandbox.command(&["disable"]);
    disable_command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut startup_command = sandbox.command(&["enable-startup"]);
    startup_command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let disable = disable_command.spawn().unwrap();
    let enable_startup = startup_command.spawn().unwrap();
    let disable_output = disable.wait_with_output().unwrap();
    let startup_output = enable_startup.wait_with_output().unwrap();

    assert_success(&disable_output);
    assert_success(&startup_output);
    let config = read_config(&config_path);
    assert!(!config.enabled);
    assert!(config.start_at_login);
    assert!(sandbox.launch_agent_path().exists());

    let status = stdout(&sandbox.run(&["startup-status", "--json"]));
    assert!(status.contains("\"installed\": true"));
    assert!(status.contains("\"configured_for_current_exe\": true"));
    assert!(status.contains("\"config_start_at_login\": true"));
    assert!(status.contains("\"in_sync\": true"));
}

#[test]
fn dynamics_rollback_is_atomic_and_preserves_unrelated_settings() {
    let sandbox = CliSandbox::new("dynamics-rollback");
    let config_path = sandbox.default_config_path();
    let config = AppConfig {
        enabled: false,
        reverse_horizontal: true,
        discrete_scroll_step_size: 9,
        smooth_preset: SmoothPreset::Fast,
        device_rules: vec![DeviceRule {
            alias: Some("Desk".to_string()),
            step_size: Some(8),
            smooth_preset: Some(SmoothPreset::Balanced),
            ..DeviceRule::for_hardware(
                HardwareId {
                    vendor_id: 1,
                    product_id: 2,
                },
                None,
                false,
            )
        }],
        ..AppConfig::default()
    };
    fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();

    let output = sandbox.run(&["rollback-dynamics"]);
    assert!(stdout(&output).contains("rolled back to off"));

    let rolled_back = read_config(&config_path);
    assert!(!rolled_back.enabled);
    assert!(rolled_back.reverse_horizontal);
    assert_eq!(rolled_back.discrete_scroll_step_size, 9);
    assert_eq!(rolled_back.smooth_preset, SmoothPreset::Off);
    assert_eq!(rolled_back.device_rules[0].alias.as_deref(), Some("Desk"));
    assert_eq!(rolled_back.device_rules[0].step_size, Some(8));
    assert_eq!(rolled_back.device_rules[0].smooth_preset, None);
}

#[test]
fn trace_lab_replays_a_bounded_trace_without_creating_config() {
    let sandbox = CliSandbox::new("trace-lab");
    let trace_path = sandbox.home.join("scroll-trace.toml");
    let trace = ScrollTrace::new(vec![
        TraceSample {
            timestamp_us: 0,
            device_kind: DeviceKind::Mouse,
            continuous: false,
            axis: Axis::Vertical,
            input_delta: 1,
            observed_output_delta: -3,
            decision_reason: DecisionReason::Reversed,
        },
        TraceSample {
            timestamp_us: 10_000,
            device_kind: DeviceKind::Mouse,
            continuous: false,
            axis: Axis::Vertical,
            input_delta: -4,
            observed_output_delta: 4,
            decision_reason: DecisionReason::Reversed,
        },
        TraceSample {
            timestamp_us: 300_000,
            device_kind: DeviceKind::Mouse,
            continuous: false,
            axis: Axis::Vertical,
            input_delta: -1,
            observed_output_delta: 3,
            decision_reason: DecisionReason::Reversed,
        },
    ])
    .unwrap();
    fs::write(&trace_path, trace.to_toml().unwrap()).unwrap();
    let trace_path = trace_path.to_string_lossy().to_string();

    let output = sandbox.run(&[
        "trace-lab",
        &trace_path,
        "--baseline-gain",
        "2",
        "--clutch-gap-ms",
        "150",
    ]);
    let stdout = stdout(&output);

    assert!(stdout.contains("samples: 3 (discrete=3, continuous=0)"));
    assert!(stdout.contains("duration: 300000 us"));
    assert!(stdout.contains("clutch sessions: 2"));
    assert!(stdout.contains("direction changes: 1"));
    assert!(stdout.contains("replay matches observed: 3/3 (100.0%)"));
    assert!(stdout.contains("constant-gain baseline: 2x"));
    assert!(!sandbox.default_config_path().exists());
}

static NEXT_SANDBOX_ID: AtomicU64 = AtomicU64::new(0);
