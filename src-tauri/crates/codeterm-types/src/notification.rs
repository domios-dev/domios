//! The one closed notification level set, and the one closed set of places a
//! notification may reach. Every sink reads these definitions, so a level valid
//! for one is valid for all and no surface restates either axis.

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

/// Where a notification may go. The user answers this per level, so the pair
/// (level, destination) is the whole routing decision and neither axis is
/// restated anywhere else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotifyDestination {
    /// The in-app indicator and its panel.
    Bell,
    /// The OS notification centre.
    Desktop,
    /// The opt-in Telegram push channel.
    Telegram,
}

impl NotifyDestination {
    /// The whole set, in column order. Every list of destinations anywhere
    /// derives from this constant instead of restating it.
    pub const ALL: [Self; 3] = [Self::Bell, Self::Desktop, Self::Telegram];

    /// Unknown input is a rejection, never a pass-through string.
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        Self::ALL.into_iter().find(|known| known.as_str() == raw)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bell => "bell",
            Self::Desktop => "desktop",
            Self::Telegram => "telegram",
        }
    }

    /// Human-facing copy for the settings column. Exhaustive over the set, so
    /// a destination added later cannot ship without a name.
    pub fn label(self) -> &'static str {
        match self {
            Self::Bell => "Ring the bell",
            Self::Desktop => "System notification",
            Self::Telegram => "Telegram",
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

    #[test]
    fn notification_settings_destination_all_is_the_single_source_of_the_columns() {
        assert_eq!(NotifyDestination::ALL.len(), 3);
        assert_eq!(
            NotifyDestination::wire_values(),
            NotifyDestination::ALL
                .into_iter()
                .map(NotifyDestination::as_str)
                .collect::<Vec<_>>()
        );
        let distinct: std::collections::HashSet<&str> =
            NotifyDestination::wire_values().into_iter().collect();
        assert_eq!(
            distinct.len(),
            NotifyDestination::ALL.len(),
            "destination wire values collide"
        );
    }

    #[test]
    fn notification_settings_destination_round_trips_and_rejects_unknown_input() {
        for destination in NotifyDestination::ALL {
            assert_eq!(
                NotifyDestination::parse(destination.as_str()),
                Some(destination)
            );
            assert!(
                !destination.label().trim().is_empty(),
                "{} needs user-facing copy for its column",
                destination.as_str()
            );
        }
        for rejected in ["toast", "sound", "", "   ", "Bell", "email"] {
            assert_eq!(
                NotifyDestination::parse(rejected),
                None,
                "{rejected} is outside the closed destination set"
            );
        }
    }

    #[test]
    fn notification_settings_grid_is_the_product_of_the_two_closed_axes() {
        let cells: Vec<(NotifyLevel, NotifyDestination)> = NotifyLevel::ALL
            .into_iter()
            .flat_map(|level| {
                NotifyDestination::ALL
                    .into_iter()
                    .map(move |destination| (level, destination))
            })
            .collect();
        assert_eq!(
            cells.len(),
            NotifyLevel::ALL.len() * NotifyDestination::ALL.len(),
            "the grid is ALL x ALL, never a hand-written cell list"
        );
        let distinct: std::collections::HashSet<(NotifyLevel, NotifyDestination)> =
            cells.iter().copied().collect();
        assert_eq!(distinct.len(), cells.len(), "a cell is named twice");
    }
}
