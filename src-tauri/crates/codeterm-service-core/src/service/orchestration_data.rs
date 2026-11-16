use serde::Serialize;

use crate::multiplexer::types::{AgentRole, PendingPrompt};

use crate::service::plan::PlanError;

pub use codeterm_wire::api_contracts::TabFailure;

#[derive(Debug, Clone, Serialize)]
pub struct TabStatusInfo {
    pub tab_id: String,
    pub status: String,
    pub agent_type: Option<String>,
    pub agent_session_id: Option<String>,
    pub role: Option<AgentRole>,
    pub role_profile: Option<String>,
    pub parent_tab_id: Option<String>,
    pub can_spawn: bool,
    pub depth: u8,
    pub children: Vec<String>,
    pub cwd: Option<String>,
    pub title: String,
    pub team_id: Option<String>,
    pub pending_prompt: Option<PendingPrompt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<TabFailure>,
    /// The effective permission state, not the launch request: no questions when true.
    pub full_access: bool,
}

#[derive(Debug)]
pub enum TeamPlanCreationError {
    Team(String),
    Plan(PlanError),
    SessionBindingPending,
    SessionUnregistered,
}

#[derive(Debug, Clone, Default)]
pub struct TeamPlanCaller {
    pub tab_id: Option<String>,
    pub session_id: Option<String>,
    pub binding_nonce: Option<String>,
}

#[derive(Clone, Copy)]
pub enum WorkerTurnSignal {
    ProviderStop,
    IdleTransition,
}

impl WorkerTurnSignal {
    pub fn source(self) -> &'static str {
        match self {
            Self::ProviderStop => "provider-stop",
            Self::IdleTransition => "idle-transition",
        }
    }
}

pub fn epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}
