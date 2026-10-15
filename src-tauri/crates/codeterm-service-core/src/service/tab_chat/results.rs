#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatOffsetKind {
    Bytes,
    Items,
}

#[derive(serde::Serialize)]
pub struct ChatStartResult {
    /// Vestigial raw-text channel; chat_start returns parsed `messages` today.
    pub text: String,
    pub offset_kind: ChatOffsetKind,
    pub start_offset: u64,
    pub offset: u64,
    /// Agent provisioning is in flight; the tab boot state supplies its visible
    /// progress until the mux reports a session id.
    #[serde(default)]
    pub pending: bool,
    /// Structured providers (sqlite-backed, e.g. opencode) return parsed
    /// messages here instead of raw `text`; the frontend renders them directly
    /// and listens on `tab-chat-msg-{id}` for deltas. `None` = JSONL provider.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub messages: Option<Vec<crate::agent::chat_types::ChatMsg>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    pub source_session_id: Option<String>,
    pub session_binding_nonce: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscription_id: Option<String>,
    /// The provider named no transcript source for this bound session, so
    /// nothing is loading one: the chat view reports unavailable, never waits.
    pub source_missing: bool,
}

#[derive(serde::Serialize)]
pub struct ChatBackfillResult {
    pub text: String,
    pub offset_kind: ChatOffsetKind,
    pub start_offset: u64,
    pub end_offset: u64,
    /// Window parsed with the tab's own provider parser — same normalized
    /// shape as `ChatStartResult::messages`, ready to merge client-side.
    pub messages: Vec<crate::agent::chat_types::ChatMsg>,
    pub source_session_id: Option<String>,
    pub session_binding_nonce: Option<String>,
}
