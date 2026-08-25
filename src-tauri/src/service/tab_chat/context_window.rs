//! Host-side turn composition for a chatBackend whose plugin declared host
//! context ownership. Pure: no I/O, no plugin, no model, no clock.
//!
//! **This bounds the HOST TURN, not each HTTP request.** A plugin tool loop
//! re-enters per round-trip, so one host send can be N server calls and only
//! the first is host-composed; the inner round-trips are still assembled by the
//! plugin. Anything that describes this as full host ownership is wrong.
//!
//! The unit is messages — host-transcript entries grouped into turn groups.
//! Nothing on this path could honestly back a finer unit, and none is invented.

use crate::agent::chat_types::ChatMsg;
use crate::chatbackend::{HostTurn, TurnGroup};

/// PROVISIONAL placeholder, not an evidence-backed number: the bound is left
/// unpicked pending a live probe. This declaration is the only place it is set.
pub const PROVISIONAL_GROUP_LIMIT: usize = 12;

/// Assembled-payload ceiling — not a second accounting unit, just where one
/// pathological entry fails loudly instead of being silently clipped.
pub const MAX_PAYLOAD_BYTES: usize = 1_000_000;

/// The in-flight user turn is not in the mirror yet, so it carries a fixed
/// synthetic id; being the last group, no bound can drop it.
pub const IN_FLIGHT_ENTRY_ID: &str = "host-turn-in-flight";

/// The counted unit, as a closed set of one. A second variant would be a second
/// accounting unit, and nothing on this path could honestly back one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextUnit {
    Messages,
}

/// What happened to the composed turn. Every case is recorded; none is silent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextOutcome {
    Sent,
    /// The mirror exists but could not be read; a context-free turn went out
    /// loudly rather than quietly.
    TranscriptUnavailable,
    /// The assembled payload exceeded [`MAX_PAYLOAD_BYTES`]; nothing was sent
    /// and nothing was truncated.
    PayloadTooLarge,
    /// The backend refused the payload. The host never auto-shrinks and retries.
    Rejected,
}

/// One line of the `.context.jsonl` decision sidecar: the limit in effect and
/// what it did, asserted from data the host holds — never from a rendering.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextDecision {
    pub turn: u64,
    pub unit: ContextUnit,
    pub limit: usize,
    pub groups_available: usize,
    pub groups_sent: usize,
    pub groups_dropped: usize,
    pub entries_sent: usize,
    pub dropped_entry_ids: Vec<String>,
    pub system_prompt_included: bool,
    pub system_prompt_bytes: usize,
    /// Transcript entries carrying the role-brief seed marker, excluded from
    /// `groups` because the brief travels in `HostTurn.system`. Recorded so the
    /// exclusion is a stated decision per turn, never a silent re-filter.
    pub system_prompt_seed_ids: Vec<String>,
    pub payload_bytes: usize,
    pub outcome: ContextOutcome,
    /// Set only when the composition refused or the backend rejected it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offending_entry_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// A composed turn and the record of how it was composed.
#[derive(Debug, Clone, PartialEq)]
pub struct Composition {
    pub turn: HostTurn,
    pub decision: ContextDecision,
}

impl Composition {
    pub fn is_sendable(&self) -> bool {
        self.decision.outcome != ContextOutcome::PayloadTooLarge
    }
}

/// Split a host transcript into turn groups: one `user` entry plus everything
/// that follows it until the next `user` entry, in source order.
///
/// The group is the window's atomic unit, so a `tool_call` and its
/// `tool_result` are never individually addressable and a boundary cannot fall
/// between them. Entries preceding the first `user` entry form a leading group
/// rather than being dropped.
pub fn group_transcript(entries: &[ChatMsg]) -> Vec<TurnGroup> {
    let mut groups: Vec<TurnGroup> = Vec::new();
    for entry in entries {
        if entry.kind == "user" || groups.is_empty() {
            groups.push(TurnGroup {
                entries: vec![entry.clone()],
            });
            continue;
        }
        groups
            .last_mut()
            .expect("a group exists once the first entry was pushed")
            .entries
            .push(entry.clone());
    }
    groups
}

/// Whether an entry is the role-brief seed `openSession` writes into the
/// transcript. Structural: the host-injected marker kind, never prompt copy —
/// a message that merely quotes the marker inline is ordinary conversation.
pub fn is_system_prompt_seed(entry: &ChatMsg) -> bool {
    crate::message_markers::parse_host_injected(&entry.content)
        .is_some_and(|marker| marker.kind == "system_prompt")
}

/// Compose one host turn: the role brief in its own field, the last `limit`
/// whole turn groups, and the in-flight user text as the final group.
///
/// `limit` counts GROUPS, not raw entries. It is clamped to at least one so the
/// in-flight turn is never the thing a bound drops; the clamped value is what
/// the decision records as the limit in effect.
///
/// The brief's seed entry is excluded from `groups` — it is the same text as
/// `HostTurn.system`, and forwarding both would send the brief twice. The
/// exclusion is recorded in `systemPromptSeedIds`; when no brief was supplied,
/// the seed's payload is promoted into `system` rather than dropped.
pub fn compose(system: &str, transcript: &[ChatMsg], limit: usize, in_flight: &str) -> Composition {
    let limit = limit.max(1);
    let (seeds, conversation): (Vec<&ChatMsg>, Vec<&ChatMsg>) = transcript
        .iter()
        .partition(|entry| is_system_prompt_seed(entry));
    let system = if system.is_empty() {
        seeds
            .last()
            .map(|seed| seed_brief(seed))
            .unwrap_or_default()
    } else {
        system.to_string()
    };
    let conversation = conversation.into_iter().cloned().collect::<Vec<_>>();

    let mut groups = group_transcript(&conversation);
    // A lone user entry: the plugin echoes exactly this final group back into
    // its transcript, so any other shape stops the user's message rendering.
    groups.push(TurnGroup {
        entries: vec![in_flight_entry(in_flight)],
    });

    let groups_available = groups.len();
    let groups_dropped = groups_available.saturating_sub(limit);
    let dropped_entry_ids = groups
        .iter()
        .take(groups_dropped)
        .flat_map(|group| group.entries.iter().map(|entry| entry.id.clone()))
        .collect::<Vec<_>>();
    let kept = groups.split_off(groups_dropped);

    let entries_sent = kept.iter().map(|group| group.entries.len()).sum();
    let payload_bytes = kept
        .iter()
        .flat_map(|group| group.entries.iter())
        .map(|entry| entry.content.len())
        .sum::<usize>();
    let oversized = payload_bytes > MAX_PAYLOAD_BYTES;

    Composition {
        decision: ContextDecision {
            turn: 0,
            unit: ContextUnit::Messages,
            limit,
            groups_available,
            groups_sent: kept.len(),
            groups_dropped,
            entries_sent,
            dropped_entry_ids,
            system_prompt_included: !system.is_empty(),
            system_prompt_bytes: system.len(),
            system_prompt_seed_ids: seeds.iter().map(|seed| seed.id.clone()).collect(),
            payload_bytes,
            outcome: if oversized {
                ContextOutcome::PayloadTooLarge
            } else {
                ContextOutcome::Sent
            },
            offending_entry_id: oversized.then(|| largest_entry_id(&kept)).flatten(),
            error: None,
        },
        turn: HostTurn {
            system: system.to_string(),
            groups: kept,
        },
    }
}

/// The brief carried inside a seed entry's marker payload.
fn seed_brief(entry: &ChatMsg) -> String {
    crate::message_markers::parse_host_injected(&entry.content)
        .map(|marker| marker.payload)
        .unwrap_or_default()
}

fn in_flight_entry(text: &str) -> ChatMsg {
    ChatMsg {
        id: IN_FLIGHT_ENTRY_ID.into(),
        kind: "user".into(),
        content: text.to_string(),
        ..ChatMsg::default()
    }
}

fn largest_entry_id(groups: &[TurnGroup]) -> Option<String> {
    groups
        .iter()
        .flat_map(|group| group.entries.iter())
        .max_by_key(|entry| entry.content.len())
        .map(|entry| entry.id.clone())
}

#[cfg(test)]
mod tests;
