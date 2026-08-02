use crate::multiplexer::types::AgentRole;
use crate::plan::{PlanMode, PlanningAuthor, PlanningState, PlannerRef};
use serde_json::Value;
use crate::service::plan::PlanError;
use crate::service::ServiceContext;

#[derive(Clone, Debug, Default)]
pub(crate) struct AuthenticatedPlanCaller {
    pub tab_id: Option<String>,
    pub session_id: Option<String>,
    pub binding_nonce: Option<String>,
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
        let plan = self.plan_store.get(plan_id).ok_or("plan not found")?;
        let tab_id = caller.tab_id.as_deref().filter(|value| !value.is_empty());
        let session_id = caller
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
            .ok_or("the live bound planning session is required")?;
        let live = tab.agent_session_id.as_deref() == session_id
            && tab.session_binding_nonce.as_deref() == nonce
            && tab.reported_session_binding_nonce.as_deref() == nonce
            && session_id.is_some()
            && nonce.is_some();
        if !live {
            return Err("the live bound planning session is required".into());
        }
        let owner = plan.owner.tab_id.as_deref() == tab_id
            && plan
                .owner
                .session_id
                .as_deref()
                .is_none_or(|expected| Some(expected) == session_id)
            && tab.role == Some(AgentRole::Orchestrator);
        if owner {
            return Ok(PlanningAuthor::User);
        }
        let planner = matches!(
            &plan.planner,
            Some(PlannerRef::Tab { tab_id: expected_tab, session_id: Some(expected_session), .. })
                if Some(expected_tab.as_str()) == tab_id && Some(expected_session.as_str()) == session_id
        ) && plan.owner.tab_id.as_deref() == tab.parent_tab_id.as_deref()
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
        Err("the live bound Plan owner or draft Planner session is required".into())
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
    const EXECUTION_FIELDS: &[&str] = &[
        "status", "status_label", "lane", "assign_tab", "assigned_tab_id", "tab",
        "executor_role", "role_guidance", "personality", "personality_provenance",
        "attempt", "attempt_id", "attempts", "competition", "review_stage", "qa_state",
    ];
    let field = EXECUTION_FIELDS
        .iter()
        .find(|field| body.get(**field).is_some_and(|value| !value.is_null()));
    match field {
        Some(field) => Err(format!(
            "draft Planners may only edit structural task fields; '{field}' requires the Plan Orchestrator"
        )),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::multiplexer::state::MultiplexerState;
    use crate::plan::{OrchestratorRef, Plan, PlanMode, PlannerRef, TaskStatus};
    use crate::service::plan::NewTask;
    use crate::service::test_fixture::ServiceFixture;

    fn fixture() -> (
        ServiceFixture,
        String,
        AuthenticatedPlanCaller,
        AuthenticatedPlanCaller,
    ) {
        let (mut mux, _) = MultiplexerState::new();
        let workspace = mux.list_workspaces()[0].id.clone();
        let owner = mux.create_tab(&workspace).unwrap();
        mux.set_tab_role(&owner, AgentRole::Orchestrator, None)
            .unwrap();
        let planner = mux.create_tab(&workspace).unwrap();
        let team_id = mux
            .resolve_or_mint_team_for_parent(&owner)
            .unwrap()
            .team_id;
        for (tab_id, session, nonce) in [
            (&owner, "owner-session", "owner-nonce"),
            (&planner, "planner-session", "planner-nonce"),
        ] {
            let tab = mux.get_tab_mut(tab_id).unwrap();
            tab.agent_session_id = Some(session.into());
            tab.session_binding_nonce = Some(nonce.into());
            tab.reported_session_binding_nonce = Some(nonce.into());
        }
        {
            let tab = mux.get_tab_mut(&planner).unwrap();
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
        plan.planner = Some(PlannerRef::Tab {
            tab_id: planner.clone(),
            session_id: Some("planner-session".into()),
            title: None,
        });
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
            plan.tasks.iter().find(|task| task.id == task_id).unwrap().status,
            TaskStatus::Pending
        );
        assert!(fixture.service.ensure_task_dispatchable(&plan_id).is_ok());
    }

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
        let author = PlanningAuthor::Planner { tab_id: "planner".into() };
        for field in ["status", "lane", "executor_role", "assign_tab", "attempt_id"] {
            assert!(ensure_planner_task_fields_are_structural(
                &author,
                &serde_json::json!({ (field): "forbidden" })
            )
            .is_err());
        }
        assert!(ensure_planner_task_fields_are_structural(
            &author,
            &serde_json::json!({ "name": "Structural", "depends_on": [] })
        )
        .is_ok());
    }

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
}
