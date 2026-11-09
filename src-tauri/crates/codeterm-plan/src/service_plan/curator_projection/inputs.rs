use crate::plan::{ControlLoopMode, OrchestratorRef, PlanMode, PlanStatus};

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CuratorRoundCursorRead {
    pub number: u64,
    pub ended_at: u64,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CuratorDigestScope {
    pub id: String,
    pub title: String,
    pub status: PlanStatus,
    pub mode: PlanMode,
    pub team_id: Option<String>,
    pub owner: OrchestratorRef,
    pub created_at: u64,
    pub control_mode: ControlLoopMode,
    pub running: bool,
    pub last_round: Option<CuratorRoundCursorRead>,
    pub manual_run_requested: bool,
    pub last_real_round_at: Option<u64>,
    pub last_synthesis_group_close_at: Option<u64>,
    pub owner_turn_cursor: Option<String>,
    pub owner_session_key: Option<String>,
    pub real_rounds_started: u64,
}

#[derive(serde::Serialize)]
pub struct CuratorRuntimeNotice {
    pub id: String,
    pub timestamp: u64,
    pub action_needed: bool,
    pub answered: bool,
    pub resolved: bool,
    pub title: String,
    pub body: Option<String>,
}
