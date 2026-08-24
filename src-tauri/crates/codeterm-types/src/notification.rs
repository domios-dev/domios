//! The one closed notification level set. Every sink that may deliver to the
//! user reads this definition, so a level valid for one is valid for all.

use serde::{Deserialize, Serialize};

/// What a notification is, on the single axis a caller may name. Progress is a
/// nameable level here rather than an unnameable absence; whether a level
/// interrupts is destination configuration, not membership in this set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotifyLevel {
    /// Progress. Does not stop the agent.
    Verbose,
    /// The agent is waiting on the user.
    ActionNeeded,
    /// It broke and the agent stopped.
    Failure,
}

impl NotifyLevel {
    /// The whole set, in wire order. Every list of levels anywhere derives from
    /// this constant instead of restating it.
    pub const ALL: [Self; 3] = [Self::Verbose, Self::ActionNeeded, Self::Failure];

    /// Unknown input is a rejection, never a pass-through string.
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        Self::ALL.into_iter().find(|known| known.as_str() == raw)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Verbose => "verbose",
            Self::ActionNeeded => "action_needed",
            Self::Failure => "failure",
        }
    }

    /// Human-facing copy, derived from the wire value so no surface restates it.
    pub fn label(self) -> String {
        let wire = self.as_str().replace('_', " ");
        let mut chars = wire.chars();
        match chars.next() {
            Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            None => wire,
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
    fn notify_level_all_is_the_single_source_of_the_set() {
        assert_eq!(NotifyLevel::ALL.len(), 3);
        // Derivation, not a restated triple: the wire list is ALL mapped.
        assert_eq!(
            NotifyLevel::wire_values(),
            NotifyLevel::ALL
                .into_iter()
                .map(NotifyLevel::as_str)
                .collect::<Vec<_>>()
        );
        let distinct: std::collections::HashSet<&str> =
            NotifyLevel::wire_values().into_iter().collect();
        assert_eq!(distinct.len(), NotifyLevel::ALL.len(), "wire values collide");
    }

    #[test]
    fn notify_level_round_trips_and_rejects_the_retired_names() {
        for level in NotifyLevel::ALL {
            assert_eq!(NotifyLevel::parse(level.as_str()), Some(level));
        }
        for retired in ["milestone", "blocker", "Milestone", "Blocker"] {
            assert_eq!(
                NotifyLevel::parse(retired),
                None,
                "{retired} is a retired name and must not parse"
            );
        }
    }

    #[test]
    fn notify_level_rejects_unknown_and_empty_input() {
        for rejected in ["progress", "info", "", "   ", "Verbose", "urgent", "action needed"] {
            assert_eq!(
                NotifyLevel::parse(rejected),
                None,
                "{rejected} is outside the closed set"
            );
        }
    }

    #[test]
    fn notify_level_labels_are_derived_from_wire_values() {
        assert_eq!(NotifyLevel::ActionNeeded.label(), "Action needed");
        for level in NotifyLevel::ALL {
            assert_eq!(
                level.label().to_lowercase().replace(' ', "_"),
                level.as_str(),
                "label must be derived from the wire value"
            );
        }
    }
}
