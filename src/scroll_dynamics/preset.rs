//! Stable experimental preset vocabulary and parameters.

use std::fmt;

use serde::{Deserialize, Serialize};

use super::DynamicsError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SmoothPreset {
    #[default]
    Off,
    Precise,
    Balanced,
    Fast,
}

impl SmoothPreset {
    pub const ALL: [Self; 4] = [Self::Off, Self::Precise, Self::Balanced, Self::Fast];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Precise => "precise",
            Self::Balanced => "balanced",
            Self::Fast => "fast",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Precise => "Precise",
            Self::Balanced => "Balanced",
            Self::Fast => "Fast",
        }
    }

    pub const fn goal(self) -> &'static str {
        match self {
            Self::Off => "Exact immediate pass-through",
            Self::Precise => "Longest control window for small corrections",
            Self::Balanced => "Middle response for general wheel use",
            Self::Fast => "Shortest response with the largest immediate share",
        }
    }

    pub const fn parameters(self) -> PresetParameters {
        match self {
            Self::Off => PresetParameters {
                immediate_per_mille: 1_000,
                tail_duration_us: 0,
            },
            Self::Precise => PresetParameters {
                immediate_per_mille: 350,
                tail_duration_us: 120_000,
            },
            Self::Balanced => PresetParameters {
                immediate_per_mille: 550,
                tail_duration_us: 90_000,
            },
            Self::Fast => PresetParameters {
                immediate_per_mille: 750,
                tail_duration_us: 60_000,
            },
        }
    }
}

impl fmt::Display for SmoothPreset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresetParameters {
    pub immediate_per_mille: u16,
    pub tail_duration_us: u64,
}

impl PresetParameters {
    pub fn validate(self) -> Result<Self, DynamicsError> {
        let valid = self.immediate_per_mille <= 1_000
            && if self.tail_duration_us == 0 {
                self.immediate_per_mille == 1_000
            } else {
                self.immediate_per_mille > 0 && self.immediate_per_mille < 1_000
            };
        if !valid {
            return Err(DynamicsError::InvalidParameters {
                immediate_per_mille: self.immediate_per_mille,
                tail_duration_us: self.tail_duration_us,
            });
        }
        Ok(self)
    }

    pub(crate) fn immediate_ratio(self) -> f64 {
        f64::from(self.immediate_per_mille) / 1_000.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_have_stable_testable_parameters_and_goals() {
        let expected = [
            (SmoothPreset::Off, "off", 1_000, 0),
            (SmoothPreset::Precise, "precise", 350, 120_000),
            (SmoothPreset::Balanced, "balanced", 550, 90_000),
            (SmoothPreset::Fast, "fast", 750, 60_000),
        ];
        assert_eq!(SmoothPreset::ALL.len(), expected.len());
        for (preset, key, immediate_per_mille, tail_duration_us) in expected {
            let parameters = preset.parameters().validate().unwrap();
            assert_eq!(preset.as_str(), key);
            assert_eq!(parameters.immediate_per_mille, immediate_per_mille);
            assert_eq!(parameters.tail_duration_us, tail_duration_us);
            assert!(!preset.goal().is_empty());
        }
    }

    #[test]
    fn invalid_custom_parameters_are_rejected() {
        assert!(
            PresetParameters {
                immediate_per_mille: 1_001,
                tail_duration_us: 10,
            }
            .validate()
            .is_err()
        );
        assert!(
            PresetParameters {
                immediate_per_mille: 500,
                tail_duration_us: 0,
            }
            .validate()
            .is_err()
        );
    }
}
