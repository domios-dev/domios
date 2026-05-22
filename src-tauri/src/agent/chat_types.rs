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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RichMessage {
    /// Free-form role string. Common values: "user", "assistant",
    /// "system". Plugin chooses; frontend renderer maps to styling.
    pub role: String,
    #[serde(default)]
    pub blocks: Vec<MessageBlock>,
    /// Unix seconds — null if the source row lacked a timestamp.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub ts: Option<i64>,
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
