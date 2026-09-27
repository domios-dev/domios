//! The one closed notification level set, and the one closed set of places a
//! notification may reach. Every sink reads these definitions, so a level valid
//! for one is valid for all and no surface restates either axis.

use serde::{Deserialize, Serialize};

use crate::pty::PromptIdentity;

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

/// Anything that may or may not be an offered choice. Implemented by both the
/// caller-facing and the daemon-facing action type so the choice-set rules are
/// stated once for both.
pub trait OfferedChoice {
    fn answer_label(&self) -> Option<&str>;
}

/// What one notification button does, as the daemon stores and emits it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NotificationAction {
    /// Opens a URL.
    Url { label: String, value: String },
    /// Closes the card and does nothing else.
    Dismiss { label: String },
    /// One offered choice. The label is its entire payload, so answering can
    /// only ever carry the label back to the tab recorded on the notification.
    Answer { label: String },
    /// A structured prompt choice resolved against the current prompt on the
    /// captured asking tab, without routing text through the agent's chat.
    RespondPromptOption {
        label: String,
        value: String,
        identity: PromptIdentity,
    },
}

/// What a CALLER may ask a notification button to do.
///
/// There is deliberately no invoke variant here. A value of this type has
/// nowhere to put a command name, so a caller-supplied action does not name a
/// command that some later surface has to refuse - it cannot name one at all.
/// This is the provenance seam: every transport that accepts an action set
/// parses into this type and can name only user-facing actions.
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

/// Total, and exhaustive over the caller set.
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
            Self::Url { label, .. }
            | Self::Dismiss { label }
            | Self::Answer { label }
            | Self::RespondPromptOption { label, .. } => label,
        }
    }

    /// The label of an offered choice, and nothing for any other kind.
    pub fn answer_label(&self) -> Option<&str> {
        match self {
            Self::Answer { label } | Self::RespondPromptOption { label, .. } => Some(label),
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
    #[serde(default)]
    pub answered_via: Option<String>,
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
            answered_via: None,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answer_slot_reads_legacy_rows_without_answer_channel() {
        let slot: AnswerSlot = serde_json::from_value(serde_json::json!({
            "asked_from_tab": "tab",
            "answered": "Proceed",
            "delivered": true
        }))
        .unwrap();

        assert_eq!(slot.answered.as_deref(), Some("Proceed"));
        assert_eq!(slot.answered_via, None);
    }

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
    fn notification_settings_local_surfaces_are_the_only_named_columns() {
        assert_eq!(NotifyDestination::LOCAL.len(), 2);
        for surface in NotifyDestination::LOCAL {
            assert!(!surface.is_channel(), "a local surface is not a channel");
            assert_eq!(
                NotifyDestination::parse_local(surface.as_str()),
                Some(surface)
            );
        }
        // A channel name is not a surface, and core refuses to guess at one
        // rather than widening the parse into a pass-through.
        for outside in ["toast", "sound", "", "   ", "Bell", "email"] {
            assert_eq!(NotifyDestination::parse_local(outside), None);
        }
    }

    /// Core carries a channel it was handed and can neither name nor caption
    /// it. That is the whole point: the roster decides membership.
    #[test]
    fn notification_settings_a_channel_column_is_carried_never_named() {
        let invented = "a-service-core-has-never-heard-of";
        let column = NotifyDestination::Channel(invented);
        assert!(column.is_channel());
        assert_eq!(column.as_str(), invented);
        assert_eq!(
            NotifyDestination::parse_local(invented),
            None,
            "resolving a channel is the registry's job, not a parse in core"
        );
    }

    #[test]
    fn notification_settings_grid_is_the_product_of_the_level_set_and_the_columns() {
        let resolved = ["one-channel", "another-channel"];
        let columns = NotifyDestination::columns(resolved);
        assert_eq!(
            columns.len(),
            NotifyDestination::LOCAL.len() + resolved.len()
        );
        // A channel that took a surface's wire value would silently shadow that
        // surface's cell, so the whole column set must stay distinct.
        let distinct: std::collections::HashSet<&str> =
            columns.iter().map(|column| column.as_str()).collect();
        assert_eq!(
            distinct.len(),
            columns.len(),
            "a column wire value collides"
        );

        let cells: Vec<(NotifyLevel, NotifyDestination<'_>)> = NotifyLevel::ALL
            .into_iter()
            .flat_map(|level| columns.iter().map(move |column| (level, *column)))
            .collect();
        assert_eq!(
            cells.len(),
            NotifyLevel::ALL.len() * columns.len(),
            "the grid is levels x resolved columns, never a hand-written cell list"
        );
        let distinct: std::collections::HashSet<(NotifyLevel, NotifyDestination<'_>)> =
            cells.iter().copied().collect();
        assert_eq!(distinct.len(), cells.len(), "a cell is named twice");
    }
}
