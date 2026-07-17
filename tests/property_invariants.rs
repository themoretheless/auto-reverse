//! Deterministic property/fuzz coverage for the three highest-risk pure parsers/models.

use auto_reverse::config::{
    AppConfig, DeviceRule, TransferError, export_document, preview_import_document,
};
use auto_reverse::device::DeviceKind;
use auto_reverse::device::HardwareId;
use auto_reverse::diagnostics::{Axis, DecisionReason};
use auto_reverse::scroll_dynamics::{
    DISTANCE_EPSILON_POINTS, DynamicsPhase, ScrollDynamics2D, ScrollVector, SmoothPreset,
};
use auto_reverse::scroll_trace::{ScrollTrace, TraceSample};

const CASES: u64 = 512;

#[derive(Clone, Copy)]
struct SplitMix64(u64);

impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        value ^ (value >> 31)
    }

    fn bounded(&mut self, upper_exclusive: u64) -> u64 {
        self.next() % upper_exclusive
    }

    fn boolean(&mut self) -> bool {
        self.next() & 1 == 1
    }
}

#[test]
fn seeded_trace_parser_fuzz_never_panics_and_successes_round_trip() {
    const ALPHABET: &[u8] =
        b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_-=+[]{}\"'.,# \n\t";

    for seed in 0..CASES {
        let mut rng = SplitMix64(seed);
        let length = rng.bounded(1_024) as usize;
        let input = (0..length)
            .map(|_| ALPHABET[rng.bounded(ALPHABET.len() as u64) as usize] as char)
            .collect::<String>();

        if let Ok(trace) = ScrollTrace::from_toml(&input) {
            let encoded = trace.to_toml().unwrap();
            assert_eq!(ScrollTrace::from_toml(&encoded).unwrap(), trace);
        }
    }
}

#[test]
fn generated_trace_documents_round_trip_for_many_shapes() {
    for seed in 0..CASES {
        let mut rng = SplitMix64(seed ^ 0x0A11_CE55);
        let count = rng.bounded(32) as usize + 1;
        let mut timestamp_us = 0;
        let mut samples = Vec::with_capacity(count);
        for index in 0..count {
            if index > 0 {
                timestamp_us += rng.bounded(50_000);
            }
            let input_delta = rng.bounded(81) as i64 - 40;
            samples.push(TraceSample {
                timestamp_us,
                device_kind: match rng.bounded(4) {
                    0 => DeviceKind::Mouse,
                    1 => DeviceKind::Trackpad,
                    2 => DeviceKind::MagicMouse,
                    _ => DeviceKind::Unknown,
                },
                continuous: rng.boolean(),
                axis: if rng.boolean() {
                    Axis::Vertical
                } else {
                    Axis::Horizontal
                },
                input_delta,
                observed_output_delta: input_delta.saturating_neg(),
                decision_reason: DecisionReason::Reversed,
            });
        }

        let trace = ScrollTrace::new(samples).unwrap();
        let encoded = trace.to_toml().unwrap();
        assert_eq!(ScrollTrace::from_toml(&encoded).unwrap(), trace);
    }
}

#[test]
fn seeded_config_parser_fuzz_never_panics_or_silently_accepts_unknown_keys() {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789_-=+[]\"'.,# \n\t";
    let current = AppConfig::default();

    for seed in 0..CASES {
        let mut rng = SplitMix64(seed ^ 0xC0F1_600D);
        let length = rng.bounded(1_024) as usize;
        let input = (0..length)
            .map(|_| ALPHABET[rng.bounded(ALPHABET.len() as u64) as usize] as char)
            .collect::<String>();
        let _ = preview_import_document(&input, &current);

        let unknown = format!("config_version = 1\nseed_{seed} = {}\n", rng.boolean());
        let error = preview_import_document(&unknown, &current).unwrap_err();
        assert!(matches!(
            error,
            TransferError::UnknownFields(fields) if fields == [format!("seed_{seed}")]
        ));
        assert!(unknown.contains(&format!("seed_{seed}")));
    }
}

#[test]
fn generated_current_and_v0_configs_migrate_and_round_trip() {
    for seed in 0..CASES {
        let mut rng = SplitMix64(seed ^ 0x51A7_E001);
        let rule_count = rng.bounded(4) as usize;
        let device_rules = (0..rule_count)
            .map(|index| {
                DeviceRule::for_hardware(
                    HardwareId {
                        vendor_id: 0x1000 + index as u32,
                        product_id: rng.bounded(u16::MAX.into()) as u32 + 1,
                    },
                    None,
                    rng.boolean(),
                )
            })
            .collect();
        let config = AppConfig {
            enabled: rng.boolean(),
            reverse_vertical: rng.boolean(),
            reverse_horizontal: rng.boolean(),
            reverse_mouse: rng.boolean(),
            reverse_trackpad: rng.boolean(),
            reverse_magic_mouse: rng.boolean(),
            reverse_unknown: rng.boolean(),
            discrete_scroll_step_size: rng.bounded(21) as i64,
            smooth_preset: SmoothPreset::ALL[rng.bounded(4) as usize],
            start_at_login: rng.boolean(),
            reverse_only_raw_input: rng.boolean(),
            device_rules,
            ..AppConfig::default()
        };

        let current_document = export_document(&config).unwrap();
        let current = preview_import_document(&current_document, &config).unwrap();
        assert_eq!(current.candidate, config);

        let mut legacy_value: toml::Value = toml::from_str(&current_document).unwrap();
        legacy_value
            .as_table_mut()
            .unwrap()
            .remove("config_version");
        let legacy_document = toml::to_string(&legacy_value).unwrap();
        let migrated = preview_import_document(&legacy_document, &config).unwrap();
        assert_eq!(migrated.migration.source_version, 0);
        assert_eq!(migrated.candidate, config);
    }
}

#[test]
fn generated_two_axis_sessions_conserve_distance_and_finish_idle() {
    for seed in 0..CASES {
        let mut rng = SplitMix64(seed ^ 0x0D1A_01C5);
        for preset in SmoothPreset::ALL {
            let mut dynamics = ScrollDynamics2D::new(preset);
            let count = rng.bounded(32) + 1;
            let mut timestamp_us = 0;
            let mut input = ScrollVector::ZERO;
            let mut output = ScrollVector::ZERO;

            for _ in 0..count {
                timestamp_us += rng.bounded(20_000) + 1;
                let vertical = (rng.bounded(8_000) + 1) as f64 / 128.0;
                let horizontal = -((rng.bounded(8_000) + 1) as f64 / 128.0);
                input.vertical_points += vertical;
                input.horizontal_points += horizontal;
                let emitted = dynamics
                    .handle_event(
                        timestamp_us,
                        ScrollVector {
                            vertical_points: vertical,
                            horizontal_points: horizontal,
                        },
                        false,
                    )
                    .unwrap();
                output.vertical_points += emitted.delta.vertical_points;
                output.horizontal_points += emitted.delta.horizontal_points;
            }

            let tail = dynamics.sample(timestamp_us + 200_000).unwrap();
            output.vertical_points += tail.delta.vertical_points;
            output.horizontal_points += tail.delta.horizontal_points;

            let tolerance = DISTANCE_EPSILON_POINTS * (count as f64 + 2.0);
            assert!((output.vertical_points - input.vertical_points).abs() <= tolerance);
            assert!((output.horizontal_points - input.horizontal_points).abs() <= tolerance);
            assert_eq!(dynamics.vertical_state().phase, DynamicsPhase::Idle);
            assert_eq!(dynamics.horizontal_state().phase, DynamicsPhase::Idle);
        }
    }
}
