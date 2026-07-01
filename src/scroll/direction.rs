use serde::{Deserialize, Serialize};

/// A scroll direction decision. This is a domain concept, not a UI
/// checkbox - the UI only ever edits the config that produces one of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScrollDirection {
    Natural,
    Reversed,
}

impl ScrollDirection {
    pub fn toggled(self) -> Self {
        match self {
            ScrollDirection::Natural => ScrollDirection::Reversed,
            ScrollDirection::Reversed => ScrollDirection::Natural,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggled_flips_both_ways() {
        assert_eq!(ScrollDirection::Natural.toggled(), ScrollDirection::Reversed);
        assert_eq!(ScrollDirection::Reversed.toggled(), ScrollDirection::Natural);
    }
}
