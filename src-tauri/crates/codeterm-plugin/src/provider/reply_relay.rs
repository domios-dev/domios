use serde::{Deserialize, Serialize};

/// Provider-authoritative top-level turn completion, including all automatic continuations.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplyRelayTurn {
    pub user_turn_id: String,
    /// UTF-8 byte start of the original user record in this session history.
    pub user_record_start: u64,
    pub user_text: String,
    pub assistant_turn_id: String,
    pub answer: String,
    pub complete: bool,
}
