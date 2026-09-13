//! Rich-message types for the UI-mode chat tail pipeline.
//!
//! Distinct from `SessionMessage` (used by the session browser / restore
//! pipeline) — these carry the full polymorphic block structure each
//! provider's JSONL packs into one entry: text, thinking, tool_use,
//! tool_result. The chat UI renders each block kind on its own; codeterm
//! core doesn't interpret block payloads, it just shuttles them.
//!
//! Deliberately minimal: `role`, `ts`, and an array of opaque blocks.
//! Anything provider-specific (claude's `isMeta`, codex's reasoning
//! tags, custom display hints) goes INSIDE a block's `data` field, so
//! the wire contract doesn't grow per-provider knobs.

use serde::{Deserialize, Serialize};

/// A provider-agnostic interactive prompt surfaced to the chat UI. Populated
/// by the enrich pipeline (`AgentProvider::parse_interactive_events`), never
/// from raw JSONL parsing. New kinds (Confirm, Select, …) plug in here so call
/// sites that already render `Vec<InteractiveElement>` need no change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InteractiveElement {
    Question(InteractiveQuestion),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractiveQuestion {
    /// Stable widget key: `{tool_use_id}-{question_index}`.
    pub id: String,
    /// Source AskUserQuestion tool_use id — answer-routing correlation.
    pub tool_use_id: String,
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub header: Option<String>,
    pub options: Vec<InteractiveOption>,
    pub multi_select: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractiveOption {
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageBlock {
    /// "text" | "thinking" | "tool_use" | "tool_result" | ...
    /// Plugin-defined string; the frontend renderer dispatches on it.
    pub kind: String,
    /// Block-specific payload. For "text"/"thinking" it's `{text: "..."}`.
    /// For "tool_use" it's `{id, name, input}`. For "tool_result" it's
    /// `{tool_use_id, content, is_error}`. Codeterm core treats this as
    /// opaque — plugins decide structure.
    #[serde(default)]
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChatTimelineMarker {
    Queued,
    ExternalEdit { path: String },
    ContextCompaction,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSessionIntent {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub goal: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RichMessage {
    /// Free-form role string. Common values: "user", "assistant",
    /// "system". Plugin chooses; frontend renderer maps to styling.
    pub role: String,
    #[serde(default)]
    pub blocks: Vec<MessageBlock>,
    /// Unix seconds — null if the source row lacked a timestamp.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub ts: Option<i64>,
    /// Source row uuid — the stable id base for flattened `ChatMsg`s so the
    /// streaming UI merges in place instead of duplicating across polls.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub uuid: Option<String>,
    /// Claude `isMeta` rows (skill context, command meta) render as system
    /// messages rather than user turns.
    #[serde(
        rename = "isMeta",
        alias = "is_meta",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub is_meta: Option<bool>,
    /// Provider source row classification preserved for normalization.
    #[serde(
        rename = "sourceType",
        alias = "source_type",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub source_type: Option<String>,
    #[serde(
        rename = "sourceSubtype",
        alias = "source_subtype",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub source_subtype: Option<String>,
    #[serde(
        rename = "timelineMarker",
        alias = "timeline_marker",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub timeline_marker: Option<ChatTimelineMarker>,
    #[serde(
        rename = "sessionIntent",
        alias = "session_intent",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub session_intent: Option<ChatSessionIntent>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub level: Option<String>,
    #[serde(
        rename = "isCompactSummary",
        alias = "is_compact_summary",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub is_compact_summary: Option<bool>,
    #[serde(
        rename = "isVisibleInTranscriptOnly",
        alias = "is_visible_in_transcript_only",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub is_visible_in_transcript_only: Option<bool>,
    /// Raw ISO timestamp string passed straight through to the frontend.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub timestamp: Option<String>,
    /// Interactive prompts extracted from this message by the provider. Filled
    /// by the enrich pipeline, not by JSONL parsing.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub interactive: Vec<InteractiveElement>,
}

/// Result of a single delta read on a session JSONL. `new_offset` is the
/// byte offset to pass back on the next call to resume from. `messages`
/// is what was parsed from `[prev_offset .. new_offset)`. Empty
/// `messages` with `new_offset == prev_offset` means nothing new —
/// caller sleeps a bit and retries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDelta {
    pub new_offset: u64,
    pub messages: Vec<RichMessage>,
}

/// An image (or file) attachment on a user message, mirroring the frontend
/// `ChatAttachment`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatAttachment {
    pub id: String,
    pub mime_type: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub path: Option<String>,
}

/// A single chat message shaped like the frontend `ChatMessage`
/// (`NormalizedChatMessage`), so the browser renders it with no extra
/// mapping. Produced by providers that parse a non-JSONL store in Rust
/// (e.g. opencode's sqlite) AND by the JSONL flatten path
/// (`rich_to_chat_msgs`) — the single normalized shape for all providers.
/// `id` must be stable across polls so the UI merges an updated message in
/// place instead of duplicating it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMsg {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub timestamp: Option<String>,
    /// Monotonic source-order ordinal (JSONL rich-message index / structured row
    /// position). The client's stable transcript-sort tiebreaker when timestamps
    /// are coarse (codex floors to seconds) or absent (cursor emits none).
    #[serde(default)]
    pub seq: i64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_edits: Option<serde_json::Value>,
    /// Human description supplied by the tool call source; never inferred.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_description: Option<String>,
    /// Short action title derived only from the call's own data.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_title: Option<String>,
    /// Flow identity the source proves; `tool_flow` rows open or close it.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_flow_id: Option<String>,
    /// Set when the action re-reports a command another call in its flow represents.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_echo: Option<bool>,
    /// Per-file diffs with the exact hunks the provider's own edit record supplied.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub file_diffs: Option<serde_json::Value>,
    /// Provider-neutral web or image result metadata (query, result count, local path), passed through verbatim.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_rich_result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_name: Option<String>,
    /// Short, human-readable summary of the tool input (file path, command, …).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_args: Option<String>,
    /// Raw tool input (provider-shaped) so the chat can render diffs/args.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_input: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_error: Option<bool>,
    /// Images attached to a user message.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub attachments: Option<Vec<ChatAttachment>>,
    /// Standalone image payload (assistant image block).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub image_data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub image_mime_type: Option<String>,
    /// System-message provenance (skill_context, <cmd>_meta, meta, …).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub system_subtype: Option<String>,
    #[serde(
        rename = "timelineMarker",
        alias = "timeline_marker",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub timeline_marker: Option<ChatTimelineMarker>,
    #[serde(
        rename = "sessionIntent",
        alias = "session_intent",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub session_intent: Option<ChatSessionIntent>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub from_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub from_tab: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub from_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub from_mesh: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub inbound_kind: Option<String>,
    /// XML tag name for local-command entries.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tag_name: Option<String>,
    /// Default-collapsed in the UI (tool calls/results, thinking).
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub collapsed: bool,
    /// Interactive prompts attached to this message (matched by tool_use id).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub interactive: Vec<InteractiveElement>,
}

/// Result of one structured-chat read. `cursor` is an opaque bookmark
/// (opencode: max part `time_created`) to pass back next poll. Empty
/// `messages` means nothing new.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StructuredChat {
    pub messages: Vec<ChatMsg>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub done: Option<bool>,
}
