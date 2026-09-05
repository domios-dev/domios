use crate::multiplexer::types::{AgentRole, Tab};
use crate::plan::{PlanMode, PlanningAuthor, PlanningState};
use crate::service::plan::PlanError;
use crate::service::plan_reconcile::tab_has_live_agent;
use crate::service::ServiceContext;
use serde_json::Value;

#[derive(Clone, Debug, Default)]
pub(crate) struct AuthenticatedPlanCaller {
    pub tab_id: Option<String>,
    pub session_id: Option<String>,
    pub binding_nonce: Option<String>,
}

/// Typed planning-authority failures keep binding and role diagnostics distinct.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PlanAuthorityError {
    PlanNotFound,
    /// Live same-tab, same-session caller holding a pre-relaunch lineage nonce.
    /// Recoverable via `codeterm plan rebind` without a full agent restart.
    BindingStaleAfterRelaunch,
    /// No live binding proof: absent tab, dead/rebound session, wrong session,
    /// or a nonce with no lineage to this tab. A genuinely foreign caller lands
    /// here and is refused — the rebind never accepts it.
    BindingForeign,
    /// The caller proved it runs inside this tab, but the provider never
    /// reported a session for it, so there is no binding to authorize against
    /// and none to re-sync. Distinct from `BindingForeign` because the refusal
    /// is identical while the remedy is not: this one is repaired by fixing the
    /// provider integration, not by rebinding.
    SessionUnregistered,
    /// Binding is live but the caller is neither the Plan owner nor a draft
    /// Planner for this plan.
    RoleUnauthorized,
}

impl PlanAuthorityError {
    pub(crate) fn diagnostic_type(self) -> &'static str {
        match self {
            Self::PlanNotFound => "plan_not_found",
            Self::BindingStaleAfterRelaunch => "plan_binding_stale_after_relaunch",
            Self::BindingForeign => "plan_binding_authorization_required",
            Self::SessionUnregistered => "plan_binding_session_unregistered",
            Self::RoleUnauthorized => "plan_role_authorization_required",
        }
    }

    fn message(self) -> &'static str {
        match self {
            Self::PlanNotFound => "plan not found",
            Self::BindingStaleAfterRelaunch => {
                "the live bound planning session is required; its binding nonce is stale after a relaunch — re-sync with `codeterm plan rebind <plan>`"
            }
            Self::BindingForeign => "the live bound planning session is required",
            Self::SessionUnregistered => {
                "no agent session is registered for this tab — the provider reports one through its integration hook and none arrived. Establish the binding from this tab with `codeterm plan rebind <plan>`, then repair the integration so the next session registers itself."
            }
            Self::RoleUnauthorized => {
                "the live bound Plan owner or draft Planner session is required"
            }
        }
    }
}

impl std::fmt::Display for PlanAuthorityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

impl From<PlanAuthorityError> for String {
    fn from(error: PlanAuthorityError) -> Self {
        error.message().to_string()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RebindOutcome {
    pub tab_id: String,
    pub nonce: String,
    /// True when the live binding was actually moved onto the presented nonce;
    /// false when it was already current (idempotent re-sync).
    pub resynced: bool,
}

#[derive(Debug)]
pub(crate) enum OrchestratorLearningError {
    Authority(String),
    Store(PlanError),
}

/// Distinguish a same-session stale nonce from a foreign binding failure.
fn binding_failure_class(
    tab: &Tab,
    requested_session: Option<&str>,
    nonce: Option<&str>,
) -> PlanAuthorityError {
    let Some(nonce) = nonce else {
        return PlanAuthorityError::BindingForeign;
    };
    let session_ok = tab_has_live_agent(tab)
        && requested_session
            .is_none_or(|requested| tab.agent_session_id.as_deref() == Some(requested));
    let stale_lineage_nonce = tab.session_binding_nonce.as_deref() != Some(nonce)
        && tab.binding_nonce_in_current_session_lineage(nonce);
    if session_ok && stale_lineage_nonce {
        return PlanAuthorityError::BindingStaleAfterRelaunch;
    }
    // Presenting the tab's OWN launch nonce is proof the caller runs inside it —
    // the daemon injects that value per PTY. A foreign caller cannot reach this
    // arm, so naming the real gap leaks nothing and saves the dead-end hunt.
    if tab.agent_session_id.is_none() && tab.session_binding_nonce.as_deref() == Some(nonce) {
        return PlanAuthorityError::SessionUnregistered;
    }
    PlanAuthorityError::BindingForeign
}

impl ServiceContext {
    pub(crate) fn authorize_plan_orchestrator(
        &self,
        plan_id: &str,
        caller: &AuthenticatedPlanCaller,
    ) -> Result<(), String> {
        match self.authorize_draft_structure_mutation(plan_id, caller)? {
            PlanningAuthor::User => Ok(()),
            PlanningAuthor::Planner { .. } => {
                Err("only the live Plan Orchestrator has execution authority".into())
            }
        }
    }

    pub(crate) fn add_orchestrator_heartbeat_learning(
        &self,
        plan_id: &str,
        caller: &AuthenticatedPlanCaller,
        text: &str,
        tag: Option<&str>,
        now_ms: u64,
    ) -> Result<crate::service::plan::heartbeat::HeartbeatLearnResult, OrchestratorLearningError>
    {
        self.authorize_plan_orchestrator(plan_id, caller)
            .map_err(OrchestratorLearningError::Authority)?;
        self.plan_store
            .add_heartbeat_learning(
                Some(plan_id),
                caller.tab_id.as_deref(),
                caller.session_id.as_deref(),
                text,
                tag,
                now_ms,
            )
            .map_err(OrchestratorLearningError::Store)
    }

    pub(crate) fn authorize_draft_structure_mutation(
        &self,
        plan_id: &str,
        caller: &AuthenticatedPlanCaller,
    ) -> Result<PlanningAuthor, String> {
        self.classify_planning_caller(plan_id, caller)
            .map_err(String::from)
    }

    /// Full planning-authority verdict with the typed error preserved so the
    /// socket layer can attach a distinct diagnostic and the rebind path can
    /// tell a recoverable stale binding from a foreign caller.
    pub(crate) fn classify_planning_caller(
        &self,
        plan_id: &str,
        caller: &AuthenticatedPlanCaller,
    ) -> Result<PlanningAuthor, PlanAuthorityError> {
        let plan = self
            .plan_store
            .get(plan_id)
            .ok_or(PlanAuthorityError::PlanNotFound)?;
        let tab_id = caller.tab_id.as_deref().filter(|value| !value.is_empty());
        let requested_session_id = caller
            .session_id
            .as_deref()
            .filter(|value| !value.is_empty());
        let nonce = caller
            .binding_nonce
            .as_deref()
            .filter(|value| !value.is_empty());
        let mux = self.mux.read();
        let tab = tab_id
            .and_then(|id| mux.get_tab(id))
            .ok_or(PlanAuthorityError::BindingForeign)?;
        let session_id = requested_session_id.or(tab.agent_session_id.as_deref());
        let agent_attested = requested_session_id
            .is_none_or(|requested| Some(requested) == session_id)
            && tab.agent_session_id.as_deref() == session_id
            && tab.session_binding_nonce.as_deref() == nonce
            && tab.reported_session_binding_nonce.as_deref() == nonce
            && session_id.is_some()
            && nonce.is_some();
        // A binding the TAB attested for itself, because the provider never
        // registered a session. Only `rebind` can produce this state and only
        // from nothing, so it cannot mask a live session or be forged by a
        // caller that does not already hold this tab's launch nonce.
        let tab_attested = nonce.is_some()
            && session_id.is_none()
            && tab.agent_session_id.is_none()
            && tab.session_binding_nonce.as_deref() == nonce
            && tab.reported_session_binding_nonce.as_deref() == nonce;
        if !(agent_attested || tab_attested) {
            return Err(binding_failure_class(tab, requested_session_id, nonce));
        }
        let owner = plan.owner.tab_id.as_deref() == tab_id && tab.has_team_orchestrator_authority();
        if owner {
            return Ok(PlanningAuthor::User);
        }
        let planner = tab.role == Some(AgentRole::Manager)
            && tab.role_profile.as_deref() == Some("planner")
            && plan.team_id.as_deref() == tab.team_id.as_deref()
            && plan
                .owner
                .tab_id
                .as_deref()
                .and_then(|owner_id| mux.get_tab(owner_id))
                .is_some_and(|owner| owner.team_id == tab.team_id);
        // The Planner's binding to this plan is durable, not phase-scoped: it
        // owns structure for the plan's whole life. Approval/dispatch stay
        // Orchestrator-exclusive through `authorize_plan_orchestrator`, which
        // rejects `PlanningAuthor::Planner` outright regardless of this gate.
        if planner {
            return Ok(PlanningAuthor::Planner {
                tab_id: tab.id.clone(),
            });
        }
        Err(PlanAuthorityError::RoleUnauthorized)
    }

    /// Re-sync a nonce proven by the live tab's current-session lineage.
    pub(crate) fn rebind_stale_planning_session(
        &self,
        caller: &AuthenticatedPlanCaller,
    ) -> Result<RebindOutcome, String> {
        let tab_id = caller
            .tab_id
            .as_deref()
            .filter(|value| !value.is_empty())
            .ok_or("rebind requires the caller tab id")?;
        let nonce = caller
            .binding_nonce
            .as_deref()
            .filter(|value| !value.is_empty())
            .ok_or("rebind requires the presented binding nonce")?;
        let requested_session = caller
            .session_id
            .as_deref()
            .filter(|value| !value.is_empty());
        let mut mux = self.mux.write();
        let tab = mux
            .get_tab_mut(tab_id)
            .ok_or("the tab to rebind is no longer live")?;
        if tab.session_binding_nonce.as_deref() == Some(nonce)
            && tab.reported_session_binding_nonce.as_deref() == Some(nonce)
        {
            return Ok(RebindOutcome {
                tab_id: tab_id.to_string(),
                nonce: nonce.to_string(),
                resynced: false,
            });
        }
        // Nothing was ever registered, so there is no lineage to prove against and
        // `provable` below can never be true — that is the closed loop where the
        // repair needed what it repairs (#316). Presenting this tab's own launch
        // nonce proves the caller runs inside it, the same trust boundary every
        // tab-scoped command sits on. Bootstrap only: never over a live session,
        // never over an existing report, never for a caller claiming a session
        // id the tab does not have.
        let bootstrap = tab.agent_type.is_some()
            && tab.pty_id.is_some()
            && tab.exit_suspected_at.is_none()
            && tab.agent_session_id.is_none()
            && tab.reported_session_binding_nonce.is_none()
            && requested_session.is_none()
            && tab.session_binding_nonce.as_deref() == Some(nonce);
        if bootstrap {
            tab.reported_session_binding_nonce = Some(nonce.to_string());
            return Ok(RebindOutcome {
                tab_id: tab_id.to_string(),
                nonce: nonce.to_string(),
                resynced: true,
            });
        }
        let provable = tab_has_live_agent(tab)
            && requested_session
                .is_none_or(|requested| tab.agent_session_id.as_deref() == Some(requested))
            && tab.session_binding_nonce.as_deref() != Some(nonce)
            && tab.binding_nonce_in_current_session_lineage(nonce);
        if !provable {
            return Err(PlanAuthorityError::BindingForeign.into());
        }
        tab.remember_superseded_binding_nonce();
        tab.session_binding_nonce = Some(nonce.to_string());
        tab.reported_session_binding_nonce = Some(nonce.to_string());
        Ok(RebindOutcome {
            tab_id: tab_id.to_string(),
            nonce: nonce.to_string(),
            resynced: true,
        })
    }

    pub(crate) fn approve_and_activate_plan(
        &self,
        plan_id: &str,
        caller: &AuthenticatedPlanCaller,
        note: Option<&str>,
        now_ms: u64,
    ) -> Result<(), String> {
        self.authorize_plan_orchestrator(plan_id, caller)
            .map_err(|_| {
                "only the live Plan Orchestrator may approve and activate drafts".to_string()
            })?;
        self.plan_store
            .set_planning_state(
                plan_id,
                PlanningState::Approved,
                PlanningAuthor::User,
                note,
                now_ms,
            )
            .map_err(|error| error.to_string())
    }

    pub(crate) fn authorize_execution_action(
        &self,
        plan_id: &str,
        caller: &AuthenticatedPlanCaller,
    ) -> Result<(), String> {
        self.authorize_plan_orchestrator(plan_id, caller)?;
        let plan = self.plan_store.get(plan_id).ok_or("plan not found")?;
        if plan.planning_state != PlanningState::Approved {
            return Err("the Plan graph must be approved before execution actions".into());
        }
        Ok(())
    }

    pub(crate) fn authorize_task_execution_action(
        &self,
        task_id: &str,
        caller: &AuthenticatedPlanCaller,
    ) -> Result<(String, String), String> {
        let detail = self.plan_store.show_task(task_id).ok_or("task not found")?;
        self.authorize_execution_action(&detail.plan_id, caller)?;
        Ok((detail.plan_id, detail.task.id))
    }

    pub(crate) fn enter_execution_mode(
        &self,
        plan_id: &str,
        caller: &AuthenticatedPlanCaller,
        now_ms: u64,
    ) -> Result<(), String> {
        self.authorize_execution_action(plan_id, caller)?;
        self.plan_store
            .set_mode(plan_id, PlanMode::Execution, now_ms)
            .map_err(|error| error.to_string())
    }

    #[allow(dead_code)]
    pub(crate) fn ensure_task_dispatchable(&self, plan_id: &str) -> Result<(), PlanError> {
        let plan = self.plan_store.get(plan_id).ok_or(PlanError::NotFound)?;
        if plan.execution_tasks_unlocked() {
            Ok(())
        } else {
            Err(PlanError::TaskDispatchRequiresApproval)
        }
    }
}

pub(crate) fn ensure_planner_task_fields_are_structural(
    author: &PlanningAuthor,
    body: &Value,
) -> Result<(), String> {
    if !matches!(author, PlanningAuthor::Planner { .. }) {
        return Ok(());
    }
    // Routing (executor_role/personality/role_guidance) is planning, not
    // execution: a draft Planner writes it at task-add time, like any brief field.
    const EXECUTION_FIELDS: &[&str] = &[
        "status",
        "status_label",
        "lane",
        "assign_tab",
        "assigned_tab_id",
        "tab",
        "personality_provenance",
        "attempt",
        "attempt_id",
        "attempts",
        "competition",
        "review_stage",
        "qa_state",
    ];
    let field = EXECUTION_FIELDS
        .iter()
        .find(|field| body.get(**field).is_some_and(|value| !value.is_null()));
    match field {
        Some(field) => Err(format!(
            "draft Planners may only edit structural and routing task fields; '{field}' requires the Plan Orchestrator"
        )),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests;
