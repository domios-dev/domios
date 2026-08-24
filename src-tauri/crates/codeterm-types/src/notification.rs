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



/// What one notification button does. A closed set: an unknown kind is a parse
/// rejection, never a string that reaches a click handler.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NotificationAction {
    /// Calls a Tauri command with caller-supplied arguments. Constructed by the
    /// daemon only; nothing an agent sends may reach this variant.
    Invoke {
        label: String,
        value: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        args: Option<serde_json::Value>,
    },
    /// Opens a URL.
    Url { label: String, value: String },
    /// Closes the card and does nothing else.
    Dismiss { label: String },
    /// One offered choice. The label is its entire payload — the variant has no
    /// field for a command name or arguments, so answering can only ever carry
    /// the label back to the tab recorded on the notification.
    Answer { label: String },
}

/// Every action on the wire, before its kind is known. Parsing goes through
/// this one shape so an `answer` that smuggles a command is refused rather than
/// silently trimmed.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ActionWire {
    #[serde(rename = "type")]
    kind: String,
    label: String,
    #[serde(default)]
    value: Option<String>,
    #[serde(default)]
    args: Option<serde_json::Value>,
}

fn require_value(kind: &str, value: Option<String>) -> Result<String, String> {
    value
        .filter(|candidate| !candidate.trim().is_empty())
        .ok_or_else(|| format!("Notification action '{kind}' requires a value"))
}

/// A powerless kind must stay powerless: naming a command or arguments on one
/// is a parse error, not a field quietly dropped on the floor.
fn reject_command(
    kind: &str,
    value: &Option<String>,
    args: &Option<serde_json::Value>,
) -> Result<(), String> {
    if value.is_some() || args.is_some() {
        return Err(format!(
            "Notification action '{kind}' carries no command name and no arguments"
        ));
    }
    Ok(())
}

/// The one parse seam for every action kind. An unknown kind is refused here,
/// and a powerless kind that named a command is refused rather than trimmed.
fn action_from_wire(wire: ActionWire) -> Result<NotificationAction, String> {
    let ActionWire {
        kind,
        label,
        value,
        args,
    } = wire;
    if label.trim().is_empty() {
        return Err("Notification action label must not be empty".to_string());
    }
    match kind.as_str() {
        "invoke" => Ok(NotificationAction::Invoke {
            value: require_value(&kind, value)?,
            label,
            args,
        }),
        "url" => {
            if args.is_some() {
                return Err("Notification action 'url' carries no arguments".to_string());
            }
            Ok(NotificationAction::Url {
                value: require_value(&kind, value)?,
                label,
            })
        }
        "dismiss" => {
            reject_command(&kind, &value, &args)?;
            Ok(NotificationAction::Dismiss { label })
        }
        "answer" => {
            reject_command(&kind, &value, &args)?;
            Ok(NotificationAction::Answer { label })
        }
        other => Err(format!(
            "Unknown notification action kind: {other} (expected one of invoke, url, dismiss, answer)"
        )),
    }
}

impl<'de> Deserialize<'de> for NotificationAction {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = ActionWire::deserialize(deserializer)?;
        action_from_wire(wire).map_err(serde::de::Error::custom)
    }
}

impl NotificationAction {
    pub fn label(&self) -> &str {
        match self {
            Self::Invoke { label, .. }
            | Self::Url { label, .. }
            | Self::Dismiss { label }
            | Self::Answer { label } => label,
        }
    }

    /// The label of an offered choice, and nothing for any other kind.
    pub fn answer_label(&self) -> Option<&str> {
        match self {
            Self::Answer { label } => Some(label),
            _ => None,
        }
    }
}

/// A choice set no user could tell apart is refused: a blank label names
/// nothing, and two equal labels name the same thing twice.
pub fn validate_answer_labels(actions: &[NotificationAction]) -> Result<(), String> {
    let mut seen: Vec<String> = Vec::new();
    for label in actions.iter().filter_map(NotificationAction::answer_label) {
        let normalized = label.trim().to_lowercase();
        if normalized.is_empty() {
            return Err("A choice label must not be empty".to_string());
        }
        if seen.contains(&normalized) {
            return Err(format!("Duplicate choice label: {label}"));
        }
        seen.push(normalized);
    }
    Ok(())
}

/// Build the offered choices. Labels are the whole payload of a choice, so they
/// are vetted here rather than at the surface that renders them.
pub fn answer_actions(labels: &[String]) -> Result<Vec<NotificationAction>, String> {
    let actions: Vec<NotificationAction> = labels
        .iter()
        .map(|label| NotificationAction::Answer {
            label: label.trim().to_string(),
        })
        .collect();
    validate_answer_labels(&actions)?;
    Ok(actions)
}

/// The answer slot of one notification. `asked_from_tab` is captured when the
/// question is sent; nothing here is resolved when an answer is tapped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnswerSlot {
    /// The asking tab, recorded at send time. Cleared when that tab closes.
    pub asked_from_tab: Option<String>,
    /// The one chosen label. Set once and never overwritten.
    pub answered: Option<String>,
}

impl AnswerSlot {
    pub fn asked_from(tab_id: Option<&str>) -> Self {
        Self {
            asked_from_tab: tab_id.map(str::to_string),
            answered: None,
        }
    }

    /// Unanswered, with no tab left to carry an answer: the card says so
    /// instead of offering buttons that would do nothing.
    pub fn is_undeliverable(&self) -> bool {
        self.answered.is_none() && self.asked_from_tab.is_none()
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
