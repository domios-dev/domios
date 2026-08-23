//! The one closed notification category set. Every sink that may interrupt the
//! user reads this definition, so a category valid for one is valid for all.

use serde::{Deserialize, Serialize};

/// The complete set of interrupt-eligible categories. Routine progress has no
/// variant, so it is unnameable rather than merely undelivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NotifyCategory {
    Milestone,
    Blocker,
    Failure,
}

impl NotifyCategory {
    /// The whole set, in wire order. Every list of categories anywhere derives
    /// from this constant instead of restating it.
    pub const ALL: [Self; 3] = [Self::Milestone, Self::Blocker, Self::Failure];

    /// Unknown input is a rejection, never a pass-through string.
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        Self::ALL.into_iter().find(|known| known.as_str() == raw)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Milestone => "milestone",
            Self::Blocker => "blocker",
            Self::Failure => "failure",
        }
    }

    /// Wire values of the whole set, derived rather than restated.
    pub fn wire_values() -> Vec<&'static str> {
        Self::ALL.into_iter().map(Self::as_str).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_variant_round_trips_through_its_wire_value() {
        for category in NotifyCategory::ALL {
            assert_eq!(NotifyCategory::parse(category.as_str()), Some(category));
        }
        assert_eq!(NotifyCategory::ALL.len(), NotifyCategory::wire_values().len());
    }

    #[test]
    fn progress_and_empty_input_name_nothing() {
        for rejected in ["progress", "info", "", "   ", "Milestone", "urgent"] {
            assert_eq!(
                NotifyCategory::parse(rejected),
                None,
                "{rejected} is outside the closed set"
            );
        }
    }
}
