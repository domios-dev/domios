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
        if planner && !plan.planning_state.allows_execution_tasks() {
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
mod tests {
    use super::*;
    use crate::multiplexer::state::MultiplexerState;
    use crate::multiplexer::types::AgentRole;
    use crate::plan::{OrchestratorRef, Plan, PlanMode, TaskStatus};
    use crate::service::plan::NewTask;
    use crate::service::test_fixture::ServiceFixture;

    /// The closed loop: with no session ever registered there is no lineage to
    /// prove, so the repair could never run. Presenting the tab's own launch
    /// nonce establishes the binding (#316).
    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    #[test]
    fn rebind_establishes_a_first_binding_from_the_tabs_own_nonce() {
        let (fixture, _plan_id, owner, _planner) = fixture();
        let tab_id = owner.tab_id.clone().unwrap();
        {
            let mut mux = fixture.service.mux.write();
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            tab.agent_session_id = None;
            tab.reported_session_binding_nonce = None;
        }
        let caller = AuthenticatedPlanCaller {
            session_id: None,
            ..owner.clone()
        };
        let outcome = fixture
            .service
            .rebind_stale_planning_session(&caller)
            .expect("a tab presenting its own nonce can establish the first binding");
        assert!(
            outcome.resynced,
            "the binding was established, not already current"
        );
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&tab_id).unwrap();
        assert_eq!(
            tab.reported_session_binding_nonce, tab.session_binding_nonce,
            "the tab attested its own launch nonce"
        );
        assert!(
            tab.agent_session_id.is_none(),
            "no provider session may be invented — only the attestation is recorded"
        );
    }

    /// The bootstrap must never mask a live provider session.
    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    #[test]
    fn rebind_does_not_bootstrap_over_a_registered_session() {
        let (fixture, _plan_id, owner, _planner) = fixture();
        let tab_id = owner.tab_id.clone().unwrap();
        {
            let mut mux = fixture.service.mux.write();
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            tab.agent_session_id = Some("live-session".into());
            tab.reported_session_binding_nonce = None;
        }
        let caller = AuthenticatedPlanCaller {
            session_id: None,
            ..owner.clone()
        };
        assert!(
            fixture
                .service
                .rebind_stale_planning_session(&caller)
                .is_err(),
            "a tab with a registered session must go through the lineage proof"
        );
    }

    /// A caller without this tab's launch nonce is still refused.
    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    #[test]
    fn rebind_refuses_a_bootstrap_from_a_foreign_nonce() {
        let (fixture, _plan_id, owner, _planner) = fixture();
        let tab_id = owner.tab_id.clone().unwrap();
        {
            let mut mux = fixture.service.mux.write();
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            tab.agent_session_id = None;
            tab.reported_session_binding_nonce = None;
        }
        let caller = AuthenticatedPlanCaller {
            session_id: None,
            binding_nonce: Some("not-this-tabs-nonce".into()),
            ..owner.clone()
        };
        assert!(
            fixture
                .service
                .rebind_stale_planning_session(&caller)
                .is_err(),
            "the launch nonce is the whole proof — without it there is none"
        );
    }

    /// After the bootstrap the write gate accepts the tab, which is the point.
    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    #[test]
    fn a_tab_attested_binding_authorizes_plan_writes() {
        let (fixture, plan_id, owner, _planner) = fixture();
        let tab_id = owner.tab_id.clone().unwrap();
        {
            let mut mux = fixture.service.mux.write();
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            tab.agent_session_id = None;
            tab.reported_session_binding_nonce = None;
        }
        let caller = AuthenticatedPlanCaller {
            session_id: None,
            ..owner.clone()
        };
        assert!(
            fixture
                .service
                .authorize_draft_structure_mutation(&plan_id, &caller)
                .is_err(),
            "before the bootstrap the write is refused"
        );
        fixture
            .service
            .rebind_stale_planning_session(&caller)
            .expect("bootstrap");
        fixture
            .service
            .authorize_draft_structure_mutation(&plan_id, &caller)
            .expect("after the bootstrap the same caller may write");
    }

    /// A tab whose provider never registered a session is a dead end today: the
    /// gate refuses it exactly like a foreign caller, and `plan rebind` — the
    /// documented repair — needs the very binding that is missing. The refusal
    /// stays; the diagnosis has to say which of the two it is (#316).
    #[test]
    fn an_unregistered_session_is_named_apart_from_a_foreign_caller() {
        let (mut mux, tab_id) = MultiplexerState::new();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("claude".into());
        tab.pty_id = Some("pty-1".into());
        tab.session_binding_nonce = Some("nonce-1".into());
        // The hook never reported one, so there is nothing to authorize against.
        tab.agent_session_id = None;
        tab.reported_session_binding_nonce = None;
        let tab = &*tab;

        assert_eq!(
            binding_failure_class(tab, None, Some("nonce-1")),
            PlanAuthorityError::SessionUnregistered,
            "a tab presenting its own launch nonce is not a foreign caller"
        );
    }

    /// The arm is reached only by proof of running inside the tab, so a caller
    /// with someone else's nonce learns nothing new.
    #[test]
    fn a_foreign_nonce_still_reads_as_foreign() {
        let (mut mux, tab_id) = MultiplexerState::new();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("claude".into());
        tab.pty_id = Some("pty-1".into());
        tab.session_binding_nonce = Some("nonce-1".into());
        tab.agent_session_id = None;
        let tab = &*tab;

        assert_eq!(
            binding_failure_class(tab, None, Some("someone-elses-nonce")),
            PlanAuthorityError::BindingForeign,
        );
        assert_eq!(
            binding_failure_class(tab, None, None),
            PlanAuthorityError::BindingForeign
        );
    }

    /// The remedy has to name the integration, because rebinding cannot fix it.
    #[test]
    fn the_unregistered_diagnosis_points_away_from_rebind() {
        let error = PlanAuthorityError::SessionUnregistered;
        assert_eq!(error.diagnostic_type(), "plan_binding_session_unregistered");
        let message = error.to_string();
        assert!(
            message.contains("integration"),
            "must name where a session comes from: {message}"
        );
        assert!(
            message.contains("rebind"),
            "must say rebind is not the repair: {message}"
        );
    }

    fn fixture() -> (
        ServiceFixture,
        String,
        AuthenticatedPlanCaller,
        AuthenticatedPlanCaller,
    ) {
        let (mut mux, _) = MultiplexerState::new();
        let workspace = mux.list_workspaces()[0].id.clone();
        let owner = mux.create_tab(&workspace).unwrap();
        mux.set_orchestrator_role(&owner).unwrap();
        let planner = mux.create_tab(&workspace).unwrap();
        let team_id = mux.resolve_or_mint_team_for_parent(&owner).unwrap().team_id;
        for (tab_id, session, nonce) in [
            (&owner, "owner-session", "owner-nonce"),
            (&planner, "planner-session", "planner-nonce"),
        ] {
            let tab = mux.get_tab_mut(tab_id).unwrap();
            tab.agent_session_id = Some(session.into());
            tab.session_binding_nonce = Some(nonce.into());
            tab.reported_session_binding_nonce = Some(nonce.into());
            // Make the tab pass `tab_has_live_agent` so the stale-after-relaunch
            // proof (which requires a genuinely live tab) is exercised.
            tab.agent_type = Some("provider-neutral".into());
            tab.chat_view = true;
            tab.pty_id = Some(format!("pty-{tab_id}"));
        }
        {
            let tab = mux.get_tab_mut(&planner).unwrap();
            tab.role = Some(AgentRole::Manager);
            tab.role_profile = Some("planner".into());
            tab.parent_tab_id = Some(owner.clone());
            tab.team_id = Some(team_id.clone());
        }
        let fixture = ServiceFixture::new(mux);
        let mut plan = Plan::new(
            "Draft graph".into(),
            fixture.temp.path().display().to_string(),
            OrchestratorRef {
                tab_id: Some(owner.clone()),
                session_id: Some("owner-session".into()),
                title: None,
            },
        );
        plan.team_id = Some(team_id);
        fixture.service.plan_store.save(&mut plan, 1).unwrap();
        let caller = |tab_id: String, session: &str, nonce: &str| AuthenticatedPlanCaller {
            tab_id: Some(tab_id),
            session_id: Some(session.into()),
            binding_nonce: Some(nonce.into()),
        };
        (
            fixture,
            plan.id,
            caller(planner, "planner-session", "planner-nonce"),
            caller(owner, "owner-session", "owner-nonce"),
        )
    }

    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    #[test]
    fn draft_before_approval_is_planner_writable_but_not_dispatchable() {
        let (fixture, plan_id, planner, _) = fixture();
        assert!(matches!(
            fixture
                .service
                .authorize_draft_structure_mutation(&plan_id, &planner),
            Ok(PlanningAuthor::Planner { .. })
        ));
        fixture
            .service
            .plan_store
            .add_section(&plan_id, "Phase", "Draft", None, &[], None, None, 2)
            .unwrap();
        fixture
            .service
            .plan_store
            .add_task(
                &plan_id,
                NewTask {
                    name: "Draft task".into(),
                    kind: "implement".into(),
                    ..Default::default()
                },
                3,
            )
            .unwrap();
        assert!(matches!(
            fixture.service.ensure_task_dispatchable(&plan_id),
            Err(PlanError::TaskDispatchRequiresApproval)
        ));
    }

    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    #[test]
    fn bound_nonce_resolves_live_planner_session_when_shell_env_lacks_session_id() {
        let (fixture, plan_id, mut planner, _) = fixture();
        planner.session_id = None;
        assert!(matches!(
            fixture
                .service
                .authorize_draft_structure_mutation(&plan_id, &planner),
            Ok(PlanningAuthor::Planner { .. })
        ));
    }

    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    #[test]
    fn orchestrator_approval_atomically_activates_draft_graph() {
        let (fixture, plan_id, planner, owner) = fixture();
        let task_id = fixture
            .service
            .plan_store
            .add_task(
                &plan_id,
                NewTask {
                    name: "Draft task".into(),
                    kind: "implement".into(),
                    ..Default::default()
                },
                2,
            )
            .unwrap();
        assert!(fixture
            .service
            .approve_and_activate_plan(&plan_id, &planner, None, 3)
            .is_err());
        fixture
            .service
            .approve_and_activate_plan(&plan_id, &owner, None, 4)
            .unwrap();
        let plan = fixture.service.plan_store.get(&plan_id).unwrap();
        assert_eq!(plan.planning_state, PlanningState::Approved);
        assert_eq!(plan.mode, PlanMode::Approved);
        assert_eq!(plan.planning_log.len(), 1);
        assert_eq!(
            plan.tasks
                .iter()
                .find(|task| task.id == task_id)
                .unwrap()
                .status,
            TaskStatus::Pending
        );
        assert!(fixture.service.ensure_task_dispatchable(&plan_id).is_ok());
    }

    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    #[test]
    fn draft_execution_actions_and_mode_activation_fail_before_approval() {
        let (fixture, plan_id, _, owner) = fixture();
        let task_id = fixture
            .service
            .plan_store
            .add_task(
                &plan_id,
                NewTask {
                    name: "Assignment target".into(),
                    kind: "implement".into(),
                    ..Default::default()
                },
                2,
            )
            .unwrap();
        assert!(fixture
            .service
            .authorize_execution_action(&plan_id, &owner)
            .is_err());
        assert!(fixture
            .service
            .enter_execution_mode(&plan_id, &owner, 2)
            .is_err());
        assert!(fixture
            .service
            .authorize_task_execution_action(&task_id, &owner)
            .is_err());
        assert_eq!(
            fixture.service.plan_store.get(&plan_id).unwrap().mode,
            PlanMode::Planning
        );
    }

    #[test]
    fn planner_task_updates_reject_execution_lifecycle_fields() {
        let author = PlanningAuthor::Planner {
            tab_id: "planner".into(),
        };
        for field in [
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
        ] {
            let refusal = ensure_planner_task_fields_are_structural(
                &author,
                &serde_json::json!({ (field): "forbidden" }),
            )
            .expect_err("execution lifecycle field must stay Orchestrator-owned");
            assert!(refusal.contains(field), "refusal must name the field");
        }
        assert!(ensure_planner_task_fields_are_structural(
            &author,
            &serde_json::json!({ "name": "Structural", "depends_on": [] })
        )
        .is_ok());
    }

    /// Routing IS planning: a draft Planner cuts the task and its worker
    /// persona in one call, so these fields are outside the refusal list.
    #[test]
    fn planner_task_writes_accept_routing_fields() {
        let author = PlanningAuthor::Planner {
            tab_id: "planner".into(),
        };
        for field in ["executor_role", "personality", "role_guidance"] {
            assert!(
                ensure_planner_task_fields_are_structural(
                    &author,
                    &serde_json::json!({ (field): "routing" })
                )
                .is_ok(),
                "draft Planner must be able to set {field}"
            );
        }
        assert!(ensure_planner_task_fields_are_structural(
            &author,
            &serde_json::json!({
                "name": "Routed",
                "executor_role": "prompt-methodology-dev",
                "personality": "Nora",
                "role_guidance": "You are Nora.",
            })
        )
        .is_ok());
    }

    /// The refusal list is Planner-scoped only. The Plan Orchestrator classifies
    /// as `User`, so its authority over every field is unchanged.
    #[test]
    fn orchestrator_authority_over_task_fields_is_unrestricted() {
        let author = PlanningAuthor::User;
        for field in [
            "status",
            "lane",
            "assign_tab",
            "attempt_id",
            "qa_state",
            "executor_role",
            "personality",
            "role_guidance",
            "personality_provenance",
        ] {
            assert!(ensure_planner_task_fields_are_structural(
                &author,
                &serde_json::json!({ (field): "allowed" })
            )
            .is_ok());
        }
    }

    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    #[test]
    fn task_take_and_drop_require_approved_graph_and_live_orchestrator_identity() {
        let (fixture, plan_id, _, owner) = fixture();
        let task_id = fixture
            .service
            .plan_store
            .add_task(
                &plan_id,
                NewTask {
                    name: "Take or drop".into(),
                    kind: "implement".into(),
                    ..Default::default()
                },
                2,
            )
            .unwrap();
        assert!(fixture
            .service
            .authorize_task_execution_action(&task_id, &owner)
            .is_err());
        fixture
            .service
            .approve_and_activate_plan(&plan_id, &owner, None, 3)
            .unwrap();
        let stale = AuthenticatedPlanCaller {
            binding_nonce: Some("replayed-nonce".into()),
            ..owner.clone()
        };
        assert!(fixture
            .service
            .authorize_task_execution_action(&task_id, &stale)
            .is_err());
        assert!(fixture
            .service
            .authorize_task_execution_action(&task_id, &owner)
            .is_ok());
    }

    fn is_binding_rule(error: PlanAuthorityError) -> bool {
        matches!(
            error,
            PlanAuthorityError::BindingStaleAfterRelaunch | PlanAuthorityError::BindingForeign
        )
    }

    /// Relaunch = resume the SAME session on a fresh nonce; the prior nonce is
    /// retired into lineage tagged with the (unchanged) current session.
    fn relaunch_tab(fixture: &ServiceFixture, tab_id: &str, new_nonce: &str) {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(tab_id).unwrap();
        tab.remember_superseded_binding_nonce();
        tab.session_binding_nonce = Some(new_nonce.into());
        tab.reported_session_binding_nonce = Some(new_nonce.into());
    }

    /// Genuine session transition: retire the current nonce under the OLD session,
    /// then move the live tab onto a brand-new session id + nonce.
    fn transition_session(
        fixture: &ServiceFixture,
        tab_id: &str,
        new_session: &str,
        new_nonce: &str,
    ) {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(tab_id).unwrap();
        tab.remember_superseded_binding_nonce();
        tab.agent_session_id = Some(new_session.into());
        tab.session_binding_nonce = Some(new_nonce.into());
        tab.reported_session_binding_nonce = Some(new_nonce.into());
    }

    fn current_binding(fixture: &ServiceFixture, tab_id: &str) -> (Option<String>, Option<String>) {
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(tab_id).unwrap();
        (
            tab.session_binding_nonce.clone(),
            tab.reported_session_binding_nonce.clone(),
        )
    }

    fn sessionless(caller: &AuthenticatedPlanCaller) -> AuthenticatedPlanCaller {
        AuthenticatedPlanCaller {
            session_id: None,
            ..caller.clone()
        }
    }

    // Mirror the W3-C task gate so both authority paths are proven on one input.
    fn w3c_task_binding_matches(
        fixture: &ServiceFixture,
        caller: &AuthenticatedPlanCaller,
    ) -> bool {
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(caller.tab_id.as_deref().unwrap()).unwrap();
        let nonce = caller.binding_nonce.as_deref();
        tab.agent_session_id.as_deref() == caller.session_id.as_deref()
            && tab.session_binding_nonce.as_deref() == nonce
            && tab.reported_session_binding_nonce.as_deref() == nonce
    }

    #[test]
    fn lineage_is_session_scoped_bounded_and_deduped() {
        let (mut mux, tab_id) = MultiplexerState::new();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_session_id = Some("S".into());
        tab.session_binding_nonce = Some("n0".into());
        tab.remember_superseded_binding_nonce();
        tab.session_binding_nonce = Some("n1".into());
        tab.remember_superseded_binding_nonce();
        tab.session_binding_nonce = Some("n0".into());
        tab.remember_superseded_binding_nonce();
        // Under the SAME current session, prior and current nonces prove lineage.
        assert!(tab.binding_nonce_in_current_session_lineage("n0"));
        assert!(tab.binding_nonce_in_current_session_lineage("n1"));
        assert!(!tab.binding_nonce_in_current_session_lineage("never-issued"));
        assert_eq!(
            tab.prior_session_binding_nonces
                .iter()
                .filter(|entry| entry.nonce == "n0")
                .count(),
            1,
            "re-adopting a nonce must not duplicate its lineage entry"
        );
        // A genuine transition: the SAME nonces no longer prove lineage, because
        // their recorded session != the tab's new current session.
        tab.agent_session_id = Some("S2".into());
        tab.session_binding_nonce = Some("n2".into());
        assert!(!tab.binding_nonce_in_current_session_lineage("n0"));
        assert!(!tab.binding_nonce_in_current_session_lineage("n1"));
        // No current session proves nothing.
        tab.agent_session_id = None;
        assert!(!tab.binding_nonce_in_current_session_lineage("n2"));
        // Bounded under a single session.
        tab.agent_session_id = Some("S".into());
        for index in 0..20 {
            tab.session_binding_nonce = Some(format!("m{index}"));
            tab.remember_superseded_binding_nonce();
        }
        assert!(tab.prior_session_binding_nonces.len() <= 8);
        assert!(!tab.binding_nonce_in_current_session_lineage("m0"));
        assert!(tab.binding_nonce_in_current_session_lineage("m19"));
    }

    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    #[test]
    fn sessionless_relaunch_caller_recovers_mutation_approve_dispatch_through_rebind() {
        let (fixture, plan_id, _, owner) = fixture();
        let tab_id = owner.tab_id.clone().unwrap();
        relaunch_tab(&fixture, &tab_id, "owner-nonce-relaunched");
        // The real relaunched CLI environment has the nonce but no session id;
        // daemon-tagged lineage proves continuity.
        let caller = sessionless(&owner);
        assert!(matches!(
            fixture.service.classify_planning_caller(&plan_id, &caller),
            Err(PlanAuthorityError::BindingStaleAfterRelaunch)
        ));
        let outcome = fixture
            .service
            .rebind_stale_planning_session(&caller)
            .expect("a provable same-session relaunch caller can rebind without a session id");
        assert!(outcome.resynced);
        // Mutation, approve, and dispatch all funnel through the authority gate,
        // so the single rebind recovers every one of them without a restart.
        assert!(matches!(
            fixture.service.classify_planning_caller(&plan_id, &caller),
            Ok(PlanningAuthor::User)
        ));
        assert!(fixture
            .service
            .approve_and_activate_plan(&plan_id, &caller, None, 5)
            .is_ok());
        assert!(fixture
            .service
            .authorize_execution_action(&plan_id, &caller)
            .is_ok());
        // A second rebind by the now-current caller is an idempotent no-op.
        let repeat = fixture
            .service
            .rebind_stale_planning_session(&caller)
            .expect("idempotent rebind");
        assert!(!repeat.resynced);
    }

    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    #[test]
    fn sessionless_stale_nonce_is_refused_after_a_genuine_session_transition() {
        let (fixture, plan_id, _, owner) = fixture();
        let tab_id = owner.tab_id.clone().unwrap();
        // owner-nonce is retired under the ORIGINAL session, then the tab moves to
        // a genuinely NEW session — the retired nonce is now cross-session.
        transition_session(&fixture, &tab_id, "owner-session-2", "owner-nonce-2");
        let caller = sessionless(&owner); // still presents the old owner-nonce
        assert!(matches!(
            fixture.service.classify_planning_caller(&plan_id, &caller),
            Err(PlanAuthorityError::BindingForeign)
        ));
        let before = current_binding(&fixture, &tab_id);
        assert!(fixture
            .service
            .rebind_stale_planning_session(&caller)
            .is_err());
        // A refusal must never mutate the live current/reported binding.
        assert_eq!(current_binding(&fixture, &tab_id), before);
    }

    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    #[test]
    fn mismatched_provided_session_is_refused_even_with_a_lineage_nonce() {
        let (fixture, plan_id, _, owner) = fixture();
        let tab_id = owner.tab_id.clone().unwrap();
        relaunch_tab(&fixture, &tab_id, "owner-nonce-relaunched");
        // A provided-but-wrong session is refused: it is not laundered, and it is
        // not silently dropped in favour of the lineage proof either.
        let wrong_session = AuthenticatedPlanCaller {
            session_id: Some("some-other-session".into()),
            ..owner.clone()
        };
        assert!(matches!(
            fixture
                .service
                .classify_planning_caller(&plan_id, &wrong_session),
            Err(PlanAuthorityError::BindingForeign)
        ));
        let before = current_binding(&fixture, &tab_id);
        assert!(fixture
            .service
            .rebind_stale_planning_session(&wrong_session)
            .is_err());
        assert_eq!(current_binding(&fixture, &tab_id), before);
    }

    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    #[test]
    fn foreign_and_cross_tab_callers_are_refused_and_rebind_rejects_them() {
        let (fixture, plan_id, planner, owner) = fixture();
        relaunch_tab(&fixture, owner.tab_id.as_deref().unwrap(), "owner-nonce-2");
        // A nonce this tab never held has no lineage — foreign, and unrebindable.
        let foreign = AuthenticatedPlanCaller {
            binding_nonce: Some("never-issued-nonce".into()),
            ..owner.clone()
        };
        assert!(matches!(
            fixture.service.classify_planning_caller(&plan_id, &foreign),
            Err(PlanAuthorityError::BindingForeign)
        ));
        assert!(fixture
            .service
            .rebind_stale_planning_session(&foreign)
            .is_err());
        // Another tab cannot launder the owner's stale nonce because its lineage
        // never held that nonce.
        let cross_tab = AuthenticatedPlanCaller {
            tab_id: planner.tab_id.clone(),
            session_id: Some("planner-session".into()),
            binding_nonce: Some("owner-nonce".into()),
        };
        assert!(matches!(
            fixture
                .service
                .classify_planning_caller(&plan_id, &cross_tab),
            Err(PlanAuthorityError::BindingForeign)
        ));
        assert!(fixture
            .service
            .rebind_stale_planning_session(&cross_tab)
            .is_err());
    }

    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    #[test]
    fn binding_rule_and_role_rule_are_mechanically_distinct() {
        let (fixture, plan_id, planner, owner) = fixture();
        // Role rule: a live, correctly-bound draft Planner editing an APPROVED
        // plan is refused by the ROLE rule (the #298 TRAP's other cause).
        fixture
            .service
            .approve_and_activate_plan(&plan_id, &owner, None, 3)
            .unwrap();
        let role_err = match fixture.service.classify_planning_caller(&plan_id, &planner) {
            Err(error) => error,
            Ok(_) => panic!("a Planner editing an approved plan must fail the role rule"),
        };
        assert_eq!(role_err, PlanAuthorityError::RoleUnauthorized);
        // Binding rule: a foreign nonce is refused by the BINDING rule.
        let foreign = AuthenticatedPlanCaller {
            binding_nonce: Some("foreign".into()),
            ..owner.clone()
        };
        let binding_err = match fixture.service.classify_planning_caller(&plan_id, &foreign) {
            Err(error) => error,
            Ok(_) => panic!("a foreign nonce must fail the binding rule"),
        };
        // Mechanism, not prose: distinct variants AND distinct diagnostic types.
        assert!(is_binding_rule(binding_err));
        assert!(!is_binding_rule(role_err));
        assert_ne!(role_err.diagnostic_type(), binding_err.diagnostic_type());
    }

    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    #[test]
    fn one_rebind_heals_both_the_plan_gate_and_the_w3c_task_gate() {
        let (fixture, plan_id, _, owner) = fixture();
        relaunch_tab(&fixture, owner.tab_id.as_deref().unwrap(), "owner-nonce-3");
        // Before rebind BOTH gates reject the same stale caller — one coherent
        // stale-binding verdict, not two fighting predicates.
        let before = match fixture.service.classify_planning_caller(&plan_id, &owner) {
            Err(error) => error,
            Ok(_) => panic!("stale caller must be rejected before rebind"),
        };
        assert!(is_binding_rule(before));
        assert!(!w3c_task_binding_matches(&fixture, &owner));
        fixture
            .service
            .rebind_stale_planning_session(&owner)
            .unwrap();
        // After ONE rebind BOTH gates accept the same caller.
        assert!(matches!(
            fixture.service.classify_planning_caller(&plan_id, &owner),
            Ok(PlanningAuthor::User)
        ));
        assert!(w3c_task_binding_matches(&fixture, &owner));
    }
}
