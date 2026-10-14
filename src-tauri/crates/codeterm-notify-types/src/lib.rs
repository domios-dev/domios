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

/// A surface a notification can be spoken of by name. `Bell` and `Desktop` are
/// drawn by this app, so they are named here; a push channel is carried as the
/// service id a registry resolved, and core routes to it without ever holding a
/// variant, a constant, or a match arm that spells one.
///
/// Naming a surface is not the same as routing to one. `Bell` is still named —
/// it is how an agent offering to switch the bell on is refused by name — but
/// nothing routes on it: the panel is the complete log and takes every record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NotifyDestination<'a> {
    /// The in-app indicator and its panel.
    Bell,
    /// The OS notification centre.
    Desktop,
    /// A resolved push channel, addressed by the same service id
    /// `codeterm bridge send --service <id>` takes.
    Channel(&'a str),
}

impl<'a> NotifyDestination<'a> {
    /// The surfaces this app draws itself, in column order — every local surface
    /// core knows by name, which is a larger set than the columns settings
    /// offers: `Bell` is here so it can be recognised and refused, never routed
    /// to. Push channels are deliberately absent: there is no closed set here.
    pub const LOCAL: [Self; 2] = [Self::Bell, Self::Desktop];

    /// A local surface by wire value. A name that is not a surface is *not*
    /// widened into a channel: core has no roster to check it against, so
    /// claiming to know it would be a guess.
    pub fn parse_local(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        Self::LOCAL.into_iter().find(|known| known.as_str() == raw)
    }

    pub fn as_str(self) -> &'a str {
        match self {
            Self::Bell => "bell",
            Self::Desktop => "desktop",
            Self::Channel(service) => service,
        }
    }

    pub fn is_channel(self) -> bool {
        matches!(self, Self::Channel(_))
    }

    /// The grid's columns: the local surfaces first, then whichever channels
    /// the caller resolved. Nothing here knows how many there are.
    pub fn columns(channels: impl IntoIterator<Item = &'a str>) -> Vec<Self> {
        Self::LOCAL
            .into_iter()
            .chain(channels.into_iter().map(Self::Channel))
            .collect()
    }
}
