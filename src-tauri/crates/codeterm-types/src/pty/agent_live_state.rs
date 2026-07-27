use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimelineState {
    Working,
    Idle,
    Question,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
pub struct PaneVerdict {
    pub state: TimelineState,
    pub subtype: Option<QuestionSubtype>,
}

/// Persisted-to-frontend snapshot of a pane's live agent state. Stored on the
/// pane (runtime-only, recomputed from output) and serialized into the mux
/// snapshot so the chat view's state indicator renders it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentLiveState {
    pub state: TimelineState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtype: Option<QuestionSubtype>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_line: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_turn_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question: Option<String>,
}

impl From<AgentLiveState> for PaneVerdict {
    fn from(s: AgentLiveState) -> Self {
        PaneVerdict {
            state: s.state,
            subtype: s.subtype,
        }
    }
}

impl From<PaneVerdict> for AgentLiveState {
    fn from(v: PaneVerdict) -> Self {
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
