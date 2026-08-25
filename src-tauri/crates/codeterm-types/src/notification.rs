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

/// The fixed set of commands the daemon may put behind a notification button.
/// This is the allowlist, expressed as a type: an `Invoke` action names one of
/// these and nothing else, and no wire payload anywhere deserializes into it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonCommand {
    /// Accept or decline an offered provider update.
    RespondProviderUpdatePrompt,
}

impl DaemonCommand {
    /// The whole set, in wire order. Every list of daemon commands derives from
    /// this constant instead of restating it.
    pub const ALL: [Self; 1] = [Self::RespondProviderUpdatePrompt];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::RespondProviderUpdatePrompt => "respond_provider_update_prompt",
        }
    }

    /// Read a daemon-written command name back. Unknown input is a rejection,
    /// so this can only ever yield a member of the set above.
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        Self::ALL.into_iter().find(|known| known.as_str() == raw)
    }
}

/// Anything that may or may not be an offered choice. Implemented by both the
/// caller-facing and the daemon-facing action type so the choice-set rules are
/// stated once for both.
pub trait OfferedChoice {
    fn answer_label(&self) -> Option<&str>;
}

/// What one notification button does, as the daemon stores and emits it.
///
/// `Invoke` is the daemon's alone: it names a [`DaemonCommand`] rather than a
/// string, and this type has no `Deserialize` impl at all, so no payload on any
/// transport can produce one. Caller input parses into [`CallerAction`], which
/// has no invoke variant to begin with.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NotificationAction {
    /// Calls one allowlisted Tauri command. Daemon-constructed only.
    Invoke {
        label: String,
        command: DaemonCommand,
        #[serde(skip_serializing_if = "Option::is_none")]
        args: Option<serde_json::Value>,
    },
    /// Opens a URL.
    Url { label: String, value: String },
    /// Closes the card and does nothing else.
    Dismiss { label: String },
    /// One offered choice. The label is its entire payload, so answering can
    /// only ever carry the label back to the tab recorded on the notification.
    Answer { label: String },
}

/// What a CALLER may ask a notification button to do.
///
/// There is deliberately no invoke variant here. A value of this type has
/// nowhere to put a command name, so a caller-supplied action does not name a
/// command that some later surface has to refuse - it cannot name one at all.
/// This is the provenance seam: every transport that accepts an action set
/// parses into this type, and the only way from here into
/// [`NotificationAction`] is the total conversion below, which has no `Invoke`
/// arm to reach.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CallerAction {
    Url { label: String, value: String },
    Dismiss { label: String },
    Answer { label: String },
}

impl CallerAction {
    /// The whole caller-facing set, in wire order. `invoke` is absent by
    /// construction; every list of caller kinds derives from this constant.
    pub const KINDS: [&'static str; 3] = ["url", "dismiss", "answer"];

    pub fn label(&self) -> &str {
        match self {
            Self::Url { label, .. } | Self::Dismiss { label } | Self::Answer { label } => label,
        }
    }
}

impl OfferedChoice for CallerAction {
    fn answer_label(&self) -> Option<&str> {
        match self {
            Self::Answer { label } => Some(label),
            _ => None,
        }
    }
}

/// Total, and exhaustive over the caller set: there is no arm that produces
/// `Invoke`, so no caller-supplied action can become one.
impl From<CallerAction> for NotificationAction {
    fn from(action: CallerAction) -> Self {
        match action {
            CallerAction::Url { label, value } => Self::Url { label, value },
            CallerAction::Dismiss { label } => Self::Dismiss { label },
            CallerAction::Answer { label } => Self::Answer { label },
        }
    }
}

/// Adopt a caller-supplied action set as stored actions. The one crossing from
/// caller provenance to daemon storage, and it cannot mint an invoke target.
pub fn adopt_caller_actions(actions: Vec<CallerAction>) -> Vec<NotificationAction> {
    actions.into_iter().map(NotificationAction::from).collect()
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

/// The one parse seam for caller-supplied actions. `invoke` is not a kind here:
/// it falls through to the unknown-kind arm exactly like an invented word,
/// because the type being built has no field a command name could live in.
fn caller_action_from_wire(wire: ActionWire) -> Result<CallerAction, String> {
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
        "url" => {
            if args.is_some() {
                return Err("Notification action 'url' carries no arguments".to_string());
            }
            Ok(CallerAction::Url {
                value: require_value(&kind, value)?,
                label,
            })
        }
        "dismiss" => {
            reject_command(&kind, &value, &args)?;
            Ok(CallerAction::Dismiss { label })
        }
        "answer" => {
            reject_command(&kind, &value, &args)?;
            Ok(CallerAction::Answer { label })
        }
        other => Err(format!(
            "Unknown notification action kind: {other} (expected one of {})",
            CallerAction::KINDS.join(", ")
        )),
    }
}

impl<'de> Deserialize<'de> for CallerAction {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = ActionWire::deserialize(deserializer)?;
        caller_action_from_wire(wire).map_err(serde::de::Error::custom)
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

impl OfferedChoice for NotificationAction {
    fn answer_label(&self) -> Option<&str> {
        NotificationAction::answer_label(self)
    }
}

/// A choice set no user could tell apart is refused: a blank label names
/// nothing, and two equal labels name the same thing twice. Stated once for
/// both action types, so the caller and daemon sides cannot drift apart.
pub fn validate_answer_labels<A: OfferedChoice>(actions: &[A]) -> Result<(), String> {
    let mut seen: Vec<String> = Vec::new();
    for label in actions.iter().filter_map(OfferedChoice::answer_label) {
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
pub fn answer_actions(labels: &[String]) -> Result<Vec<CallerAction>, String> {
    let actions: Vec<CallerAction> = labels
        .iter()
        .map(|label| CallerAction::Answer {
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
    /// Whether the standing answer reached the asking tab. Recording an answer
    /// and carrying it back are separate facts, so a refused send cannot render
    /// as answered-and-delivered.
    #[serde(default)]
    pub delivered: bool,
}

impl AnswerSlot {
    pub fn asked_from(tab_id: Option<&str>) -> Self {
        Self {
            asked_from_tab: tab_id.map(str::to_string),
            answered: None,
            delivered: false,
        }
    }

    /// Unanswered, with no tab left to carry an answer: the card says so
    /// instead of offering buttons that would do nothing.
    pub fn is_undeliverable(&self) -> bool {
        self.answered.is_none() && self.asked_from_tab.is_none()
    }

    /// An answer stands that never reached the tab that asked.
    pub fn is_undelivered(&self) -> bool {
        self.answered.is_some() && !self.delivered
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
        assert_eq!(
            distinct.len(),
            NotifyLevel::ALL.len(),
            "wire values collide"
        );
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
        for rejected in [
            "progress",
            "info",
            "",
            "   ",
            "Verbose",
            "urgent",
            "action needed",
        ] {
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
