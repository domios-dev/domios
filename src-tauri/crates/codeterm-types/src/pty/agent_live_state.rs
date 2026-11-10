use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum TimelineState {
    Working,
    Idle,
    Question,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum QuestionSubtype {
    ModelChoice,
    ModelPicker,
    Permission,
    Auth,
    RateLimit,
    Update,
    Error,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TabVerdict {
    pub state: TimelineState,
    pub subtype: Option<QuestionSubtype>,
}

/// Persisted-to-frontend snapshot of a tab's live agent state. Stored on the
/// tab (runtime-only, recomputed from output) and serialized into the mux
/// snapshot so the chat view's state indicator renders it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AgentLiveState {
    pub state: TimelineState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub subtype: Option<QuestionSubtype>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub status_line: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub phase: Option<String>,
    /// The live turn's elapsed time while working; the finished turn's duration while idle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub elapsed_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub last_turn_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub question: Option<String>,
}

impl From<AgentLiveState> for TabVerdict {
    fn from(s: AgentLiveState) -> Self {
        TabVerdict {
            state: s.state,
            subtype: s.subtype,
        }
    }
}

impl From<TabVerdict> for AgentLiveState {
    fn from(v: TabVerdict) -> Self {
        AgentLiveState {
            state: v.state,
            subtype: v.subtype,
            status_line: None,
            phase: None,
            elapsed_ms: None,
            last_turn_summary: None,
            question: None,
        }
    }
}
