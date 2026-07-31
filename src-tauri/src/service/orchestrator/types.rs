use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SpawnIdentityProvenance {
    #[default]
    Suggested,
    UserSupplied,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BatchOp {
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
    pub id: Option<String>,
}

/// Distinct provenance of a launch's reasoning effort. Mirrors the model
/// provenance discipline: each source stays separate and truthful, never
/// conflated. `actual` stays `None` until authoritative runtime evidence
/// arrives — a launch flag is a request, not proof of the running effort.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ReasoningEffortProvenance {
    /// Effort recorded on the routing/plan task (dispatch intent).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub planned: Option<String>,
    /// Effort the caller explicitly asked for at spawn.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested: Option<String>,
    /// Effort that passed capability validation and reached the launch flags.
    /// Equals `requested` when supported; validation rejects rather than downgrade.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved: Option<String>,
    /// Runtime-confirmed effort. `None` until the provider reports it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual: Option<String>,
}

impl ReasoningEffortProvenance {
    pub fn is_empty(&self) -> bool {
        self.planned.is_none()
            && self.requested.is_none()
            && self.resolved.is_none()
            && self.actual.is_none()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SpawnResult {
    pub tab_id: String,
    pub pty_id: String,
    pub agent: String,
    /// Legacy launch-model field. Never runtime proof; `actual_model` is set
    /// only from provider-owned session evidence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub planned_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_resolved_model: Option<String>,
    pub actual_model: Option<String>,
    /// Resolved (validated) reasoning effort that reached the launch, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    /// Full planned/requested/resolved/actual effort provenance.
    #[serde(skip_serializing_if = "ReasoningEffortProvenance::is_empty")]
    pub reasoning_effort_provenance: ReasoningEffortProvenance,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role_profile: Option<String>,
    /// Warning when agent cwd differs from workspace cwd
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_fallback: Option<crate::service::spawn::ModelFallbackNotice>,
    /// Advisory workspace tab-budget nudge (task_4d649dfd), computed via the
    /// shared `ServiceContext::workspace_tab_budget_notice` helper. Never blocks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_notice: Option<crate::service::tab_budget::TabBudgetNotice>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SpawnAgentParams {
    pub agent: Option<String>,
    pub agent_name: Option<String>,
    pub args: Option<Vec<String>>,
    pub cwd: Option<String>,
    /// Exact cwd freshly created by the current spawn operation. This is host
    /// provenance, never accepted from a CLI/REST payload.
    #[serde(default, skip_deserializing)]
    pub provisioned_cwd: Option<String>,
    pub title: Option<String>,
    pub title_provenance: Option<SpawnIdentityProvenance>,
    pub description: Option<String>,
    pub role: Option<String>,
    pub role_profile: Option<String>,
    pub parent_tab_id: Option<String>,
    /// Plan id to bind — when `role_profile` is `planner`, binds
    /// `Plan.planner` to the spawned tab and delivers the peer prompt
    /// naming the Orchestrator (task_aa01c0db).
    pub plan_id: Option<String>,
    pub can_spawn: Option<bool>,
    pub task: Option<String>,
    #[serde(default, skip)]
    pub(crate) starter_prompt: crate::agent::provider::StarterPrompt,
    /// Bound task id (CODETERM_TASK_ID).
    pub task_id: Option<String>,
    /// Named plan task to bind when spawning `--role Worker` (`--plan-task`).
    pub plan_task: Option<String>,
    pub workspace_id: Option<String>,
    /// Force creation as a new tab instead of split tab (even with parent).
    pub force_tab: Option<bool>,
    /// Claim this existing blank tab instead of creating a new one — used by
    /// the new-session picker's in-place spawn so confirming transforms the
    /// picker's own tab rather than leaving it behind as an orphan.
    pub existing_tab_id: Option<String>,
    /// Skip legacy onboarding work for a paired Orchestrator. Its complete
    /// identity and counterpart context travel in the single launch payload.
    pub skip_orchestrator_onboarding: Option<bool>,
    /// Report target tab ID — for cron agents to report without joining hierarchy.
    /// With `report_target_mesh_peer` set, this is a tab id / `main` / `general`
    /// alias on that remote peer.
    pub report_target_tab_id: Option<String>,
    /// Paired mesh peer the report target lives on. Set (un-spoofably, from the
    /// authenticated relay sender) when `agent spawn --mesh PEER` lands here, so
    /// the spawned worker reports back across the mesh.
    pub report_target_mesh_peer: Option<String>,
    /// Default true; set false to spawn an agent that asks for permission.
    pub skip_permissions: Option<bool>,
    /// CODETERM_TAB_ID of the caller — recorded for report routing fallback.
    pub spawned_by_tab_id: Option<String>,
    /// Provider decides flag syntax via `AgentProvider::model_launch_args`.
    pub model: Option<String>,
    /// Plan/task routing model before a launch-time model override.
    pub planned_model: Option<String>,
    /// Explicitly requested reasoning effort. Capability-validated against the
    /// provider before launch; an unsupported value is rejected structurally
    /// (no silent downgrade). Provider maps it to launch flags via
    /// `AgentProvider::reasoning_effort_launch_args`.
    pub reasoning_effort: Option<String>,
    pub system_prompt: Option<String>,
    #[serde(default, skip)]
    pub(crate) manager_bootstrap: Option<crate::service::role_identity::ManagerBootstrapInputs>,
    pub preset: Option<String>,
    pub resume_session_id: Option<String>,
    pub charter: Option<String>,
    pub tick_interval_secs: Option<u64>,
    pub nudge_cooldown_secs: Option<u64>,
    pub max_actions_per_tick: Option<u32>,
    pub activate: Option<bool>,
    /// Audience axis for input-request notifications. `Automation` marks a tab
    /// whose prompts have no human owner (bench/probe tabs) — they never bell.
    #[serde(default)]
    pub audience: crate::multiplexer::types::TabAudience,
    /// Host-set, never from the wire: the fixed structured-prompt server the
    /// host allocated for a `needs_structured_prompt_server` provider. Threaded
    /// into the launch command so the agent binds its SSE bus there.
    #[serde(default, skip)]
    pub structured_prompt_server: Option<crate::agent::provider::StructuredPromptServer>,
}

impl SpawnAgentParams {
    pub(crate) fn normalize_role_profile(&mut self) -> Result<(), String> {
        crate::service::role_profile::normalize_spawn_params(self)
    }
}
