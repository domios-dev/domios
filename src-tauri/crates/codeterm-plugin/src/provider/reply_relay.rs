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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplyRelayOutcome {
    Error,
    Cancelled,
    ReflectionLimit,
}

impl ReplyRelayOutcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Cancelled => "cancelled",
            Self::ReflectionLimit => "reflection_limit",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplyRelayEndedTurn {
    pub user_turn_id: String,
    pub user_record_start: u64,
    pub user_text: String,
    pub complete: bool,
    pub outcome: ReplyRelayOutcome,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplyRelayUnavailable {
    pub unavailable: bool,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ReplyRelayEvidence {
    Answered(ReplyRelayTurn),
    Ended(ReplyRelayEndedTurn),
    Unavailable(ReplyRelayUnavailable),
}

impl ReplyRelayEvidence {
    pub fn is_valid(&self) -> bool {
        match self {
            Self::Answered(turn) => turn.complete,
            Self::Ended(turn) => !turn.complete,
            Self::Unavailable(state) => state.unavailable && !state.reason.trim().is_empty(),
        }
    }
}
