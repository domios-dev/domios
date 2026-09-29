use serde::{Deserialize, Serialize};

use super::SessionMessage;

/// Where a provider keeps the searchable text of its sessions. Core picks its
/// read strategy from this and never from the provider id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionContentSource {
    /// Sessions carry no searchable content.
    None,
    /// Each session owns `SessionMetadata::file_path`, parsed by
    /// `parse_session_messages`. Message text is stored as JSON string values,
    /// so a `session_content_hint_term` appears verbatim in the raw bytes.
    SessionFile,
    /// Sessions share one store (`file_path` is the store, not the session);
    /// `read_session_content` reads one session's messages from it.
    SharedStore,
}

/// Bounds for one `read_session_content` call.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionContentQuery {
    /// Newest-first ceiling on stored bytes the provider may scan for this session.
    pub max_bytes: u64,
    /// ASCII-lowercased terms; a message missing any of them may be omitted.
    /// Core still matches every returned message itself.
    pub hint_terms: Vec<String>,
}

/// One session's searchable messages from a shared store.
#[derive(Debug, Clone, Default)]
pub struct SessionContent {
    pub messages: Vec<SessionMessage>,
    /// The session holds more than `max_bytes`; older content went unscanned.
    pub truncated: bool,
}

/// Whether `term` is safe as a raw-byte or SQL `LIKE` prefilter: JSON encoders
/// never escape these characters, so a stored message containing it holds it verbatim.
pub fn session_content_hint_term(term: &str) -> bool {
    !term.is_empty()
        && term
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.:@#$%+=~*'!?,;()[]{}|^`".contains(c))
}
