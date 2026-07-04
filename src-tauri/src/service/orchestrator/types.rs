use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BatchOp {
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
    pub id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SpawnResult {
    pub pane_id: String,
    pub tab_id: String,
    pub pty_id: String,
    pub agent: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role_profile: Option<String>,
    /// Warning when agent cwd differs from workspace cwd
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    /// Advisory workspace pane-budget nudge (task_4d649dfd), computed via the
    /// shared `ServiceContext::workspace_pane_budget_notice` helper. Never blocks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_notice: Option<crate::service::pane_budget::PaneBudgetNotice>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SpawnAgentParams {
    pub agent: Option<String>,
    pub agent_name: Option<String>,
    pub args: Option<Vec<String>>,
    pub cwd: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub role: Option<String>,
    pub role_profile: Option<String>,
    pub parent_pane_id: Option<String>,
    /// Plan id to bind — when `role_profile` is `planner`, binds
    /// `Plan.planner` to the spawned pane and delivers the peer prompt
    /// naming the Orchestrator (task_aa01c0db).
    pub plan_id: Option<String>,
    pub can_spawn: Option<bool>,
    pub task: Option<String>,
    pub workspace_id: Option<String>,
    /// Force creation as a new tab instead of split pane (even with parent).
    pub force_tab: Option<bool>,
    /// Claim this existing blank tab instead of creating a new one — used by
    /// the new-session picker's in-place spawn so confirming transforms the
    /// picker's own tab rather than leaving it behind as an orphan.
    pub existing_tab_id: Option<String>,
    /// Skip the generic orchestrator-promotion onboarding reminder. Set by
    /// `create_orchestration_pair`, whose own `task` already carries the full
    /// peer contract — the generic reminder duplicates it AND (with a
    /// placeholder plan id the generic path can't resolve) told a live model
    /// to spawn its own redundant Planner racing the one already in flight.
    pub skip_orchestrator_onboarding: Option<bool>,
    /// Report target pane ID — for cron agents to report without joining hierarchy.
    /// With `report_target_mesh_peer` set, this is a pane id / `main` / `general`
    /// alias on that remote peer.
    pub report_target_pane_id: Option<String>,
    /// Paired mesh peer the report target lives on. Set (un-spoofably, from the
    /// authenticated relay sender) when `agent spawn --mesh PEER` lands here, so
    /// the spawned worker reports back across the mesh.
    pub report_target_mesh_peer: Option<String>,
    /// Default true; set false to spawn an agent that asks for permission.
    pub skip_permissions: Option<bool>,
    /// CODETERM_PANE_ID of the caller — recorded for report routing fallback.
    pub spawned_by_pane_id: Option<String>,
    /// Provider decides flag syntax via `AgentProvider::model_launch_args`.
    pub model: Option<String>,
    pub system_prompt: Option<String>,
    pub preset: Option<String>,
    pub resume_session_id: Option<String>,
    pub activate: Option<bool>,
    /// Host-set, never from the wire: the fixed structured-prompt server the
    /// host allocated for a `needs_structured_prompt_server` provider. Threaded
    /// into the launch command so the agent binds its SSE bus there.
    #[serde(default, skip)]
    pub structured_prompt_server: Option<crate::agent::provider::StructuredPromptServer>,
}

impl SpawnAgentParams {
    pub(crate) fn normalize_role_profile(&mut self) {
        crate::service::role_profile::normalize_spawn_params(self);
    }
}
