use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeviceKind {
    Mouse,
    Trackpad,
    MagicMouse,
    Unknown,
}

impl DeviceKind {
    pub fn is_mouse_like(self) -> bool {
        matches!(self, Self::Mouse | Self::MagicMouse)
    }
}

pub fn conservative_kind_from_continuity(continuous: bool) -> DeviceKind {
    if continuous {
        DeviceKind::Trackpad
    } else {
        DeviceKind::Mouse
    }
}

impl fmt::Display for DeviceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mouse => write!(f, "mouse"),
            Self::Trackpad => write!(f, "trackpad"),
            Self::MagicMouse => write!(f, "magic-mouse"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

impl FromStr for DeviceKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "mouse" => Ok(Self::Mouse),
            "trackpad" => Ok(Self::Trackpad),
            "magic-mouse" | "magic_mouse" => Ok(Self::MagicMouse),
            "unknown" => Ok(Self::Unknown),
            other => Err(format!(
                "unknown device kind `{other}`; expected mouse, trackpad, magic-mouse or unknown"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollPhase {
    Normal,
    Momentum,
    Start,
    End,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceObservation {
    pub continuous: bool,
    pub touching: Option<u8>,
    pub touch_elapsed_ms: Option<u64>,
    pub phase: ScrollPhase,
}

impl SourceObservation {
    pub fn from_continuity(continuous: bool) -> Self {
        Self {
            continuous,
            touching: None,
            touch_elapsed_ms: None,
            phase: ScrollPhase::Unknown,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SourceClassifier {
    last_source: DeviceKind,
}

impl Default for SourceClassifier {
    fn default() -> Self {
        Self {
            last_source: DeviceKind::Mouse,
        }
    }
}

impl SourceClassifier {
    pub fn classify(&mut self, observation: SourceObservation) -> DeviceKind {
        let source = if !observation.continuous {
            DeviceKind::Mouse
        } else if observation.touching.unwrap_or(0) >= 2
            && observation
                .touch_elapsed_ms
                .is_some_and(|elapsed| elapsed < 222)
        {
            DeviceKind::Trackpad
        } else if observation.phase == ScrollPhase::Normal
            && observation
                .touch_elapsed_ms
                .is_some_and(|elapsed| elapsed > 333)
        {
            DeviceKind::Mouse
        } else {
            self.last_source
        };

        self.last_source = source;
        source
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_continuous_scroll_is_mouse() {
        let mut classifier = SourceClassifier::default();

        assert_eq!(
            classifier.classify(SourceObservation::from_continuity(false)),
            DeviceKind::Mouse
        );
    }

    #[test]
    fn recent_two_finger_continuous_scroll_is_trackpad() {
        let mut classifier = SourceClassifier::default();

        assert_eq!(
            classifier.classify(SourceObservation {
                continuous: true,
                touching: Some(2),
                touch_elapsed_ms: Some(100),
                phase: ScrollPhase::Normal,
            }),
            DeviceKind::Trackpad
        );
    }

    #[test]
    fn ambiguous_continuous_scroll_uses_previous_source() {
        let mut classifier = SourceClassifier::default();
        classifier.classify(SourceObservation {
            continuous: true,
            touching: Some(2),
            touch_elapsed_ms: Some(100),
            phase: ScrollPhase::Normal,
        });

        assert_eq!(
            classifier.classify(SourceObservation::from_continuity(true)),
            DeviceKind::Trackpad
        );
    }
}
