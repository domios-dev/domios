//! Rich-message types for the UI-mode chat tail pipeline.
//!
//! Distinct from `SessionMessage` (used by the session browser / restore
//! pipeline) — these carry the full polymorphic block structure each
//! provider's JSONL packs into one entry: text, thinking, tool_use,
//! tool_result. The chat UI renders each block kind on its own; codeterm
//! core doesn't interpret block payloads, it just shuttles them.

use serde::{Deserialize, Serialize};

/// A single content block inside a chat message. Polymorphic — claude's
/// JSONL packs `text`, `thinking`, `tool_use`, `tool_result` into the
/// same `message.content[]` array, each with its own shape. We carry
/// the raw `data` JSON so the frontend can render whatever is in it
/// without locking a schema across providers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageBlock {
    /// "text" | "thinking" | "tool_use" | "tool_result"
    pub kind: String,
    /// Block-specific payload. For "text"/"thinking" it's the string in
    /// `{text: "..."}`. For "tool_use" it's `{id, name, input}`. For
    /// "tool_result" it's `{tool_use_id, content, is_error}`.
    pub data: serde_json::Value,
}

/// One chat message (one JSONL line that's a user/assistant entry).
/// Richer than `SessionMessage` — keeps tool calls + thinking blocks
/// so the chat UI can render them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RichMessage {
    /// "user" | "assistant"
    pub role: String,
    pub blocks: Vec<MessageBlock>,
    /// Unix seconds — null if JSONL lacked a timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ts: Option<i64>,
    /// Provider session id (claude's `session_id` / `sessionId` field on
    /// the entry). Lets the UI cross-reference with codeterm's pane
    /// session id when those diverge.
    #[serde(rename = "sessionId", skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// True for system-meta entries (e.g. claude's `isMeta: true`).
    /// UI may collapse them.
    #[serde(default, rename = "isMeta")]
    pub is_meta: bool,
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
