//! A bound Planner's authority over task STRUCTURE is durable, and stops exactly
//! at execution. Both halves live here so the widening and the boundary that
//! bounds it are read together: the first test passing while the second fails
//! would mean the check was deleted rather than taught.

use super::{verify_owner_epoch_for_task_mutation, JsonRpcError, TaskAuthorityRefusal};
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::AgentRole;
use crate::plan::{OrchestratorRef, Plan, TaskStatus};
use crate::service::plan::NewTask;
use crate::service::test_fixture::ServiceFixture;
use crate::service::ServiceState;
use serde_json::{json, Value};
use std::sync::Arc;

const PLANNER_SESSION: &str = "planner-session";
const PLANNER_NONCE: &str = "planner-nonce";
const OWNER_SESSION: &str = "owner-session";
const OWNER_NONCE: &str = "owner-nonce";
/// Non-zero: the epoch a task carries once it has been dispatched even once.
/// Zero is the state that used to grant Planner structure edits by accident.
const DISPATCHED_EPOCH: u64 = 4;

struct PlannerFixture {
    /// Held for its TempDir and app lifetime. `service` is the handle the seam
    /// actually takes: `ServiceState` is `Arc<ServiceContext>` and the fixture
    /// stores the bare context, so the Arc is built once here rather than at
    /// every call site.
    _fixture: ServiceFixture,
    service: ServiceState,
    plan_id: String,
    planner_tab: String,
}

/// Plan owned by a live Orchestrator, with a live Planner tab in the same Team:
/// `Manager` + `role_profile = planner`, the exact shape
/// `classify_planning_caller` authorizes as `PlanningAuthor::Planner`.
fn planner_fixture() -> PlannerFixture {
    let (mut mux, _) = MultiplexerState::new();
    let workspace = mux.list_workspaces()[0].id.clone();
    let owner_tab = mux.create_tab(&workspace).unwrap();
    mux.set_orchestrator_role(&owner_tab).unwrap();
    let team_id = mux
        .resolve_or_mint_team_for_parent(&owner_tab)
        .unwrap()
        .team_id;
    {
        let tab = mux.get_tab_mut(&owner_tab).unwrap();
        tab.agent_session_id = Some(OWNER_SESSION.into());
        tab.session_binding_nonce = Some(OWNER_NONCE.into());
        tab.reported_session_binding_nonce = Some(OWNER_NONCE.into());
    }
    let planner_tab = mux.create_tab(&workspace).unwrap();
    {
        let tab = mux.get_tab_mut(&planner_tab).unwrap();
        tab.role = Some(AgentRole::Manager);
        tab.role_profile = Some("planner".into());
        tab.team_id = Some(team_id.clone());
        tab.parent_tab_id = Some(owner_tab.clone());
        tab.agent_session_id = Some(PLANNER_SESSION.into());
        tab.session_binding_nonce = Some(PLANNER_NONCE.into());
        tab.reported_session_binding_nonce = Some(PLANNER_NONCE.into());
    }
    let fixture = ServiceFixture::new(mux);
    let mut plan = Plan::new(
        "Planner authority".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef {
            tab_id: Some(owner_tab.clone()),
            session_id: Some(OWNER_SESSION.into()),
            title: None,
        },
    );
    plan.team_id = Some(team_id);
    fixture.service.plan_store.save(&mut plan, 1).unwrap();
    let service = Arc::new(fixture.service.clone());
    PlannerFixture {
        _fixture: fixture,
        service,
        plan_id: plan.id,
        planner_tab,
    }
}

/// A task the Planner authored that has since been dispatched at least once, so
/// its owner epoch is non-zero and the seam's zero-epoch shortcut cannot apply.
fn dispatched_task(harness: &PlannerFixture, status: TaskStatus) -> String {
    let store = &harness.service.plan_store;
    let task_id = store
        .add_task(
            &harness.plan_id,
            NewTask {
                name: "Authored by the Planner".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let mut plan = store.get(&harness.plan_id).unwrap();
    let task = plan
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .unwrap();
    task.status = status;
    task.interruption.owner_epoch = DISPATCHED_EPOCH;
    task.interruption.owner_tab_id = Some("worker-tab".into());
    task.interruption.owner_session_id = Some("worker-session".into());
    store.save(&mut plan, 3).unwrap();
    task_id
}

fn planner_update(harness: &PlannerFixture, task_id: &str, fields: Value) -> Option<Value> {
    let mut params = json!({
        "plan_id": harness.plan_id,
        "task_id": task_id,
        "caller_tab_id": harness.planner_tab,
        "session_id": PLANNER_SESSION,
        "session_binding_nonce": PLANNER_NONCE,
        "owner_epoch": DISPATCHED_EPOCH,
    });
    let object = params.as_object_mut().unwrap();
    for (key, value) in fields.as_object().unwrap() {
        object.insert(key.clone(), value.clone());
    }
    Some(params)
}

fn refusal_type(error: &JsonRpcError) -> String {
    error
        .data
        .as_ref()
        .and_then(|data| data.get("type"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// A structural edit: sections, body and test plan, on a task the Planner does
/// not own. Both statuses, because the pending case is what proves the refusal
/// was never about protecting in-flight work.
#[test]
fn planner_updates_task_structure_on_a_dispatched_task() {
    for status in [TaskStatus::Pending, TaskStatus::InProgress] {
        let harness = planner_fixture();
        let task_id = dispatched_task(&harness, status.clone());
        let params = planner_update(
            &harness,
            &task_id,
            json!({
                "sections": "sec_one",
                "body": "restated so the Worker knows what done means",
                "test_plan": "./build.sh test-one plan lane_persist",
                "acceptance_criteria": ["the stored lane equals the reported one"],
            }),
        );
        verify_owner_epoch_for_task_mutation("plan.task.update", &params, &harness.service)
            .unwrap_or_else(|error| {
                panic!("the bound Planner must edit structure on a {status:?} task: {error:?}")
            });
    }
}

/// Without this, the previous test could be satisfied by deleting the check.
/// Every field here is one a Planner must still be refused for, asserted by the
/// refusal's `data.type` constant rather than by its prose.
#[test]
fn planner_is_still_refused_for_status_assignment_and_execution_fields() {
    let refused: &[Value] = &[
        json!({ "status": "in_progress" }),
        json!({ "status": "done" }),
        json!({ "status_label": "blocked on QA" }),
        json!({ "tab": "some-worker-tab" }),
        json!({ "assigned_tab_id": "some-worker-tab" }),
        json!({ "assign_tab": "some-worker-tab" }),
        json!({ "peer": "Mac-Air" }),
        json!({ "lane": { "worktree_path": "/lanes/elsewhere" } }),
        json!({ "review_stage": "qa" }),
        json!({ "qa_state": "running" }),
        json!({ "competition": { "enabled": true } }),
    ];
    for fields in refused {
        let harness = planner_fixture();
        let task_id = dispatched_task(&harness, TaskStatus::InProgress);
        let params = planner_update(&harness, &task_id, fields.clone());
        let error = verify_owner_epoch_for_task_mutation(
            "plan.task.update",
            &params,
            &harness.service,
        )
        .unwrap_err();
        assert_eq!(
            refusal_type(&error),
            TaskAuthorityRefusal::CallerNotTaskOwner.discriminant(),
            "refused for {fields} by the wrong discriminant"
        );
    }
}

/// Assignment, dispatch and the rest of execution are separate verbs, and the
/// Planner's structure grant must not leak onto any of them even with a body
/// that carries no forbidden field.
#[test]
fn planner_structure_grant_does_not_extend_to_execution_verbs() {
    for method in [
        "plan.task.assign",
        "plan.task.dispatch",
        "plan.task.close",
        "plan.task.pause",
        "plan.task.resume",
        "plan.task.respawn",
        "plan.task.rm",
    ] {
        let harness = planner_fixture();
        let task_id = dispatched_task(&harness, TaskStatus::InProgress);
        let params = planner_update(&harness, &task_id, json!({}));
        let error = verify_owner_epoch_for_task_mutation(method, &params, &harness.service)
            .unwrap_err();
        assert_eq!(
            refusal_type(&error),
            TaskAuthorityRefusal::CallerNotTaskOwner.discriminant(),
            "{method} refused by the wrong discriminant"
        );
    }
}

/// Plan approval is Orchestrator-exclusive through a different seam; pin it here
/// so "the Planner may edit structure" can never be read as "the Planner may
/// approve". Asserted on the authority verdict, not on a message.
#[test]
fn planner_may_not_approve_the_plan() {
    let harness = planner_fixture();
    let caller = crate::service::AuthenticatedPlanCaller {
        tab_id: Some(harness.planner_tab.clone()),
        session_id: Some(PLANNER_SESSION.into()),
        binding_nonce: Some(PLANNER_NONCE.into()),
    };
    assert!(
        harness
            .service
            .authorize_plan_orchestrator(&harness.plan_id, &caller)
            .is_err(),
        "approval and every other execution action stay Orchestrator-exclusive"
    );
    assert!(
        matches!(
            harness
                .service
                .classify_planning_caller(&harness.plan_id, &caller),
            Ok(crate::plan::PlanningAuthor::Planner { .. })
        ),
        "the same caller must still classify as this plan's Planner"
    );
}
