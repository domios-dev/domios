use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::AgentRole;
use crate::plan::{
    AgentRef, OrchestratorRef, Plan, PlanMode, ReviewStage, ReviewVerdict, TaskStatus,
};
use crate::service::plan::interruption::TaskMutationAuthority;
use crate::service::plan::task_assignment::AssignmentRuntimeBinding;
use crate::service::plan::task_attempts::BoundWorkerReport;
use crate::service::plan::{NewTask, TaskLaneUpdate};
use crate::service::test_fixture::ServiceFixture;
use codeterm_types::agent::ReportStatus;
use serde_json::{json, Value};

const OWNER_SESSION: &str = "synthetic-owner-session";
const OWNER_NONCE: &str = "synthetic-owner-nonce";
const OTHER_SESSION: &str = "synthetic-other-session";
const OTHER_NONCE: &str = "synthetic-other-nonce";

struct CleanedRuntimeFixture {
    fixture: ServiceFixture,
    plan_id: String,
    task_id: String,
    owner_tab_id: String,
    owner_team_id: String,
    other_tab_id: String,
    other_team_id: String,
    attempt_ids: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
struct RuntimeInventory {
    tabs: Vec<(String, String)>,
    pty_tabs: Vec<(String, String)>,
    linked_pty_resources: Vec<(String, bool)>,
}

fn set_identity(mux: &mut MultiplexerState, tab_id: &str, session: &str, nonce: &str) {
    let tab = mux.get_tab_mut(tab_id).unwrap();
    tab.agent_session_id = Some(session.into());
    tab.session_binding_nonce = Some(nonce.into());
    tab.reported_session_binding_nonce = Some(nonce.into());
}

fn set_status_label(
    fixture: &ServiceFixture,
    plan_id: &str,
    task_id: &str,
    label: &str,
    now_ms: u64,
) {
    fixture
        .service
        .plan_store
        .update_task(
            plan_id,
            task_id,
            None,
            None,
            None,
            None,
            None,
            Some(Some(label.into())),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            &[],
            None,
            None,
            now_ms,
        )
        .unwrap();
}

fn add_reported_generation(
    fixture: &ServiceFixture,
    plan_id: &str,
    task_id: &str,
    generation: u64,
) -> String {
    let assigned_at = generation * 10;
    let store = &fixture.service.plan_store;
    let lane_path = fixture
        .temp
        .path()
        .join(format!("removed-review-lane-{generation}"));
    assert!(!lane_path.exists());
    store
        .update_task_lane(
            plan_id,
            task_id,
            TaskLaneUpdate {
                worktree_path: Some(Some(lane_path.display().to_string())),
                branch: Some(Some(format!("lane/removed-review-{generation}"))),
                ..Default::default()
            },
            assigned_at - 1,
        )
        .unwrap();
    let session_id = format!("synthetic-worker-session-{generation}");
    let session_binding_nonce = format!("synthetic-worker-nonce-{generation}");
    let binding = AssignmentRuntimeBinding {
        session_id: session_id.clone(),
        session_binding_nonce: session_binding_nonce.clone(),
        provider: Some("codex".into()),
        model: Some("codex-synthetic".into()),
        session_model: Some("codex-synthetic".into()),
        reasoning_effort: Some("high".into()),
    };
    let executor_tab_id = format!("synthetic-review-worker-{generation}");
    store
        .assign_task_with_runtime_binding(
            plan_id,
            task_id,
            &executor_tab_id,
            None,
            Some("Worker"),
            Some(&binding),
            None,
            assigned_at,
        )
        .unwrap();
    let assigned = store.show_task(task_id).unwrap().task;
    let attempt_id = assigned.execution_attempts.last().unwrap().id.clone();
    store
        .acknowledge_task_attempt(
            plan_id,
            task_id,
            &executor_tab_id,
            &attempt_id,
            assigned_at + 1,
        )
        .unwrap();
    let authority = TaskMutationAuthority {
        tab_id: executor_tab_id.clone(),
        session_id,
        session_binding_nonce,
        owner_epoch: assigned.interruption.owner_epoch,
    };
    assert!(store
        .ingest_bound_worker_report(
            plan_id,
            task_id,
            &AgentRef::from_tab(&executor_tab_id),
            BoundWorkerReport {
                message: format!("synthetic review report {generation}"),
                status: Some(ReportStatus::InReview),
                sha: Some(format!("abcdef{generation:06}")),
                authority: Some(authority),
                ..Default::default()
            },
            assigned_at + 2,
        )
        .unwrap());
    attempt_id
}

fn retire_and_clean(
    fixture: &ServiceFixture,
    plan_id: &str,
    task_id: &str,
    outcome: TaskStatus,
    now_ms: u64,
) {
    let store = &fixture.service.plan_store;
    store
        .retire_stale_task_owner(plan_id, task_id, Some(outcome), |_| false, now_ms)
        .unwrap();
    let plan = store.get(plan_id).unwrap();
    assert!(matches!(
        crate::service::plan::lane_cleanup::try_cleanup_task_lane(
            store,
            &plan,
            task_id,
            |_| false,
            now_ms + 1,
        ),
        crate::service::plan::lane_cleanup::LaneCleanupOutcome::Cleaned
    ));
}

fn fixture() -> CleanedRuntimeFixture {
    let (mut mux, _) = MultiplexerState::new();
    let workspace = mux.list_workspaces()[0].id.clone();
    let owner_tab_id = mux.create_tab(&workspace).unwrap();
    let other_tab_id = mux.create_tab(&workspace).unwrap();
    mux.set_orchestrator_role(&owner_tab_id).unwrap();
    let owner_team_id = mux
        .resolve_or_mint_team_for_parent(&owner_tab_id)
        .unwrap()
        .team_id;
    mux.set_orchestrator_role(&other_tab_id).unwrap();
    let other_team_id = mux
        .resolve_or_mint_team_for_parent(&other_tab_id)
        .unwrap()
        .team_id;
    set_identity(&mut mux, &owner_tab_id, OWNER_SESSION, OWNER_NONCE);
    set_identity(&mut mux, &other_tab_id, OTHER_SESSION, OTHER_NONCE);

    let fixture = ServiceFixture::new(mux);
    let mut plan = Plan::new(
        "Authenticated cleaned review bounce".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef {
            tab_id: Some(owner_tab_id.clone()),
            session_id: Some(OWNER_SESSION.into()),
            title: None,
        },
    );
    plan.mode = PlanMode::Execution;
    plan.team_id = Some(owner_team_id.clone());
    fixture.service.plan_store.save(&mut plan, 1).unwrap();
    let task_id = fixture
        .service
        .plan_store
        .add_task(
            &plan.id,
            NewTask {
                name: "two cleaned review generations".into(),
                kind: "implement".into(),
                acceptance_criteria: vec!["the authenticated review bounce is exact".into()],
                test_plan: Some("dispatch through the service RPC boundary".into()),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let first = add_reported_generation(&fixture, &plan.id, &task_id, 1);
    retire_and_clean(&fixture, &plan.id, &task_id, TaskStatus::Pending, 15);
    let second = add_reported_generation(&fixture, &plan.id, &task_id, 2);
    retire_and_clean(&fixture, &plan.id, &task_id, TaskStatus::InReview, 25);
    set_status_label(&fixture, &plan.id, &task_id, "review", 27);

    CleanedRuntimeFixture {
        fixture,
        plan_id: plan.id,
        task_id,
        owner_tab_id,
        owner_team_id,
        other_tab_id,
        other_team_id,
        attempt_ids: vec![first, second],
    }
}

fn runtime_inventory(fixture: &ServiceFixture) -> RuntimeInventory {
    let (mut tabs, mut pty_tabs) = {
        let mux = fixture.service.mux.read();
        let mut tabs = mux
            .all_tabs()
            .map(|tab| (tab.id.clone(), format!("{tab:?}")))
            .collect::<Vec<_>>();
        let mut pty_tabs = mux.pty_backed_tabs();
        tabs.sort();
        pty_tabs.sort();
        (tabs, pty_tabs)
    };
    let linked_pty_resources = {
        let pty = fixture.service.pty.lock();
        pty_tabs
            .iter()
            .map(|(_, pty_id)| (pty_id.clone(), pty.get_session(pty_id).is_some()))
            .collect()
    };
    tabs.sort();
    pty_tabs.sort();
    RuntimeInventory {
        tabs,
        pty_tabs,
        linked_pty_resources,
    }
}

fn bounce_params(
    plan_id: &str,
    task_id: &str,
    tab_id: &str,
    session_id: &str,
    nonce: &str,
    note: &str,
) -> Value {
    json!({
        "plan_id": plan_id,
        "task_id": task_id,
        "note": note,
        "changed_executable_surfaces": true,
        "distinct_oracle": false,
        "caller_tab_id": tab_id,
        "session_id": session_id,
        "session_binding_nonce": nonce,
        "from": {
            "tab_id": "caller-supplied-attribution",
            "session_id": "caller-supplied-session",
            "title": "caller-supplied-title"
        }
    })
}

fn dispatch_bounce(fixture: &ServiceFixture, params: Value) -> Result<Value, super::JsonRpcError> {
    crate::socket::server::dispatch_service_rpc(
        "plan.review.bounce",
        Some(params),
        &std::sync::Arc::new(fixture.service.clone()),
    )
}

#[test]
fn plan_review_cleaned_runtime_two_generations_endpoint() {
    let runtime = fixture();
    let store = &runtime.fixture.service.plan_store;
    let before_plan = store.get(&runtime.plan_id).unwrap();
    let before_task = store.show_task(&runtime.task_id).unwrap().task;
    let before_runtime = runtime_inventory(&runtime.fixture);
    let owner = {
        let mux = runtime.fixture.service.mux.read();
        mux.get_tab(&runtime.owner_tab_id).unwrap().clone()
    };
    let other = {
        let mux = runtime.fixture.service.mux.read();
        mux.get_tab(&runtime.other_tab_id).unwrap().clone()
    };

    assert_eq!(before_task.status, TaskStatus::InReview);
    assert_eq!(before_task.status_label.as_deref(), Some("review"));
    assert_eq!(before_task.execution_attempts.len(), 2);
    assert_eq!(before_task.execution_attempts[0].id, runtime.attempt_ids[0]);
    assert_eq!(before_task.execution_attempts[1].id, runtime.attempt_ids[1]);
    for (attempt, generation) in before_task.execution_attempts.iter().zip([1u64, 2]) {
        assert!(attempt.acknowledged_at.is_some());
        assert!(attempt.reported_at.is_some());
        assert!(attempt.closed_at.is_some());
        assert!(attempt.retired_at.is_some());
        assert_eq!(attempt.provider.as_deref(), Some("codex"));
        assert_eq!(attempt.model.as_deref(), Some("codex-synthetic"));
        assert_eq!(
            attempt.session_id.as_deref(),
            Some(format!("synthetic-worker-session-{generation}").as_str())
        );
        assert_eq!(
            attempt.session_binding_nonce.as_deref(),
            Some(format!("synthetic-worker-nonce-{generation}").as_str())
        );
        assert_eq!(
            attempt.report.as_ref().unwrap().status.as_deref(),
            Some("in_review")
        );
    }
    assert_eq!(before_task.lane.cleanup_status.as_deref(), Some("done"));
    assert_eq!(before_task.assigned_tab_id, None);
    assert_eq!(before_task.peer, None);
    assert_eq!(before_task.lane.tab_id, None);
    assert_eq!(before_task.lane.worktree_path, None);
    assert_eq!(before_task.lane.branch, None);
    assert!(before_runtime.tabs.iter().all(|(tab_id, _)| !before_task
        .execution_attempts
        .iter()
        .any(|attempt| &attempt.executor_tab_id == tab_id)));
    assert!(before_runtime.pty_tabs.is_empty());
    assert!(before_runtime.linked_pty_resources.is_empty());
    assert_eq!(before_plan.owner.tab_id.as_deref(), Some(owner.id.as_str()));
    assert_eq!(
        before_plan.team_id.as_deref(),
        Some(runtime.owner_team_id.as_str())
    );
    assert_eq!(
        owner.team_id.as_deref(),
        Some(runtime.owner_team_id.as_str())
    );
    assert_eq!(owner.role, Some(AgentRole::Manager));
    assert_eq!(owner.role_profile.as_deref(), Some("orchestrator"));
    assert_eq!(owner.agent_session_id.as_deref(), Some(OWNER_SESSION));
    assert_eq!(owner.session_binding_nonce.as_deref(), Some(OWNER_NONCE));
    assert_eq!(
        owner.reported_session_binding_nonce.as_deref(),
        Some(OWNER_NONCE)
    );
    assert_ne!(runtime.owner_team_id, runtime.other_team_id);
    assert_eq!(
        other.team_id.as_deref(),
        Some(runtime.other_team_id.as_str())
    );
    assert_eq!(other.role, Some(AgentRole::Manager));
    assert_eq!(other.role_profile.as_deref(), Some("orchestrator"));
    assert_eq!(other.agent_session_id.as_deref(), Some(OTHER_SESSION));
    assert_eq!(other.session_binding_nonce.as_deref(), Some(OTHER_NONCE));
    assert_eq!(
        other.reported_session_binding_nonce.as_deref(),
        Some(OTHER_NONCE)
    );

    let wrong_owner_error = dispatch_bounce(
        &runtime.fixture,
        bounce_params(
            &runtime.plan_id,
            &runtime.task_id,
            &runtime.other_tab_id,
            OTHER_SESSION,
            OTHER_NONCE,
            "wrong owner must not bounce",
        ),
    )
    .expect_err("a structurally valid non-owner must be refused by the endpoint");
    assert_eq!(wrong_owner_error.code, -32003);
    assert_eq!(
        wrong_owner_error.data.as_ref().unwrap()["type"],
        "plan_orchestrator_authorization_required"
    );
    assert_eq!(store.get(&runtime.plan_id).unwrap(), before_plan);
    assert_eq!(runtime_inventory(&runtime.fixture), before_runtime);

    let result = dispatch_bounce(
        &runtime.fixture,
        bounce_params(
            &runtime.plan_id,
            &runtime.task_id,
            &runtime.owner_tab_id,
            OWNER_SESSION,
            OWNER_NONCE,
            "resume review after cleaned generations",
        ),
    )
    .expect("the current authenticated owner can bounce the terminal review");
    assert!(result.get("continuation").is_none_or(Value::is_null));
    let after_plan = store.get(&runtime.plan_id).unwrap();
    let after_task = store.show_task(&runtime.task_id).unwrap().task;
    assert_eq!(after_task.status, TaskStatus::Pending);
    assert_eq!(after_task.status_label.as_deref(), Some("review bounced"));
    assert_eq!(after_task.review_stage, ReviewStage::Review);
    assert_eq!(after_task.qa_state, None);
    assert_eq!(after_task.review_round, before_task.review_round + 1);
    assert_eq!(
        after_task.review_evidence.len(),
        before_task.review_evidence.len() + 1
    );
    assert_eq!(
        after_task.milestones.len(),
        before_task.milestones.len() + 1
    );
    assert_eq!(
        after_task.acceptance_criteria,
        before_task.acceptance_criteria
    );
    assert_eq!(after_task.test_plan, before_task.test_plan);
    assert_eq!(
        after_task.execution_attempts.len(),
        before_task.execution_attempts.len()
    );
    assert_eq!(
        &after_task.execution_attempts[..1],
        &before_task.execution_attempts[..1]
    );
    let terminal_before = before_task.execution_attempts.last().unwrap();
    let terminal_after = after_task.execution_attempts.last().unwrap();
    let mut expected_terminal = (*terminal_before).clone();
    expected_terminal.continuation.review_round = Some(after_task.review_round);
    expected_terminal.continuation.state = Some("executor_gone".into());
    expected_terminal.continuation.milestone_id =
        Some(after_task.milestones.last().unwrap().id.clone());
    expected_terminal.continuation.delivered_at = None;
    expected_terminal
        .closure_reason
        .get_or_insert_with(|| "review_bounce_executor_gone".into());
    assert_eq!(terminal_after, &expected_terminal);

    let expected_actor = AgentRef {
        tab_id: Some(owner.id.clone()),
        session_id: Some(OWNER_SESSION.into()),
        title: Some(owner.title.clone()),
    };
    let new_evidence = after_task.review_evidence.last().unwrap();
    let new_milestone = after_task.milestones.last().unwrap();
    assert_eq!(new_evidence.from, expected_actor);
    assert_eq!(new_evidence.verdict, ReviewVerdict::Bounce);
    assert_eq!(new_evidence.note, "resume review after cleaned generations");
    assert!(new_evidence.changed_executable_surfaces);
    assert!(!new_evidence.distinct_oracle);
    assert_eq!(new_milestone.id, result["milestone_id"].as_str().unwrap());
    assert_eq!(new_milestone.kind, "review");
    assert_eq!(new_milestone.from, expected_actor);
    assert_eq!(new_milestone.ts, new_evidence.at);
    assert_eq!(
        after_plan.review_policy.credit,
        before_plan.review_policy.credit + 1
    );
    assert_eq!(after_plan.updated_at, after_task.updated_at);
    assert_eq!(after_task.status_changed_at, new_evidence.at);

    let mut normalized_after = after_plan.clone();
    normalized_after.updated_at = before_plan.updated_at;
    normalized_after.review_policy.credit = before_plan.review_policy.credit;
    let normalized_task = normalized_after
        .tasks
        .iter_mut()
        .find(|task| task.id == runtime.task_id)
        .unwrap();
    normalized_task.status = before_task.status;
    normalized_task.status_label = before_task.status_label.clone();
    normalized_task.status_changed_at = before_task.status_changed_at;
    normalized_task.updated_at = before_task.updated_at;
    normalized_task.review_evidence = before_task.review_evidence.clone();
    normalized_task.review_round = before_task.review_round;
    normalized_task.review_stage = before_task.review_stage;
    normalized_task.qa_state = before_task.qa_state.clone();
    normalized_task.milestones = before_task.milestones.clone();
    normalized_task.execution_attempts = before_task.execution_attempts.clone();
    assert_eq!(normalized_after, before_plan);
    assert_eq!(runtime_inventory(&runtime.fixture), before_runtime);

    let before_replay_plan = store.get(&runtime.plan_id).unwrap();
    let before_replay_runtime = runtime_inventory(&runtime.fixture);
    let replay_error = dispatch_bounce(
        &runtime.fixture,
        bounce_params(
            &runtime.plan_id,
            &runtime.task_id,
            &runtime.owner_tab_id,
            OWNER_SESSION,
            OWNER_NONCE,
            "pending replay remains refused",
        ),
    )
    .expect_err("a pending cleaned bounce replay is not a new review transition");
    assert_eq!(replay_error.code, -32603);
    assert_eq!(
        replay_error.data.as_ref().unwrap()["type"],
        "review_not_in_review"
    );
    assert_eq!(store.get(&runtime.plan_id).unwrap(), before_replay_plan);
    assert_eq!(runtime_inventory(&runtime.fixture), before_replay_runtime);
}
