use super::super::JsonRpcError;
use crate::multiplexer::state::MultiplexerState;
use crate::plan::{OrchestratorRef, PlanMode, PlanningState, TaskStatus};
use crate::service::plan::NewTask;
use crate::service::test_fixture::ServiceFixture;
use crate::service::{SpawnAgentParams, TeamMutationCaller};
use serde_json::{json, Value};

struct UnplannedFixture {
    fixture: ServiceFixture,
    plan_id: String,
    team_id: String,
    owner_tab: String,
    project: tempfile::TempDir,
}

fn fixture() -> UnplannedFixture {
    let (mut mux, _) = MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let owner_tab = mux.create_tab(&workspace_id).unwrap();
    mux.set_orchestrator_role(&owner_tab).unwrap();
    let team_id = mux
        .resolve_or_mint_team_for_parent(&owner_tab)
        .unwrap()
        .team_id;
    {
        let owner = mux.get_tab_mut(&owner_tab).unwrap();
        owner.agent_session_id = Some("unplanned-owner-session".into());
        owner.session_binding_nonce = Some("unplanned-owner-nonce".into());
        owner.reported_session_binding_nonce = Some("unplanned-owner-nonce".into());
    }
    let fixture = ServiceFixture::new(mux);
    let project = tempfile::tempdir().unwrap();
    super::git(project.path(), &["init", "-q"]);
    super::git(
        project.path(),
        &["config", "user.email", "test@example.com"],
    );
    super::git(project.path(), &["config", "user.name", "Test"]);
    std::fs::write(project.path().join("README.md"), "fixture").unwrap();
    super::git(project.path(), &["add", "README.md"]);
    super::git(project.path(), &["commit", "-qm", "fixture"]);
    let mut plan = fixture
        .service
        .plan_store
        .create_plan_for_team(
            "unplanned dispatch".into(),
            project.path().display().to_string(),
            OrchestratorRef {
                tab_id: Some(owner_tab.clone()),
                session_id: Some("unplanned-owner-session".into()),
                title: None,
            },
            team_id.clone(),
            vec![],
            1,
        )
        .unwrap();
    plan.mode = PlanMode::Planning;
    plan.planning_state = PlanningState::Drafting;
    fixture.service.plan_store.save(&mut plan, 2).unwrap();
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("unplanned-dispatch-provider")
            .with_launch_binary("cmd.exe"),
    ));
    fixture
        .service
        .require_agent_provider_ready("unplanned-dispatch-provider")
        .unwrap();
    UnplannedFixture {
        fixture,
        plan_id: plan.id,
        team_id,
        owner_tab,
        project,
    }
}

fn params(harness: &UnplannedFixture, reason: &str) -> Option<Value> {
    Some(json!({
        "plan_id": harness.plan_id,
        "reason": reason,
        "brief": "Run the urgent probe.",
        "caller_tab_id": harness.owner_tab,
        "parent_tab_id": harness.owner_tab,
        "session_id": "unplanned-owner-session",
        "session_binding_nonce": "unplanned-owner-nonce",
        "agent": "unplanned-dispatch-provider",
        "cwd": harness.project.path().display().to_string(),
    }))
}

fn dispatch(harness: &UnplannedFixture, params: &Option<Value>) -> Result<Value, JsonRpcError> {
    super::super::unplanned::dispatch_unplanned_with_spawn(
        params,
        harness.fixture._app.handle(),
        &std::sync::Arc::new(harness.fixture.service.clone()),
        10,
        |service, spawn_params, ready_tx| {
            let tab_id = spawn_params.existing_tab_id.unwrap();
            {
                let mut mux = service.mux.write();
                let tab = mux.get_tab_mut(&tab_id).unwrap();
                tab.role = Some(crate::multiplexer::types::AgentRole::Worker);
                tab.role_profile = Some("worker".into());
                tab.parent_tab_id = Some(harness.owner_tab.clone());
                tab.team_id = Some(harness.team_id.clone());
                tab.agent_session_id = Some("unplanned-worker-session".into());
                tab.session_binding_nonce = Some("unplanned-worker-nonce".into());
                tab.reported_session_binding_nonce = Some("unplanned-worker-nonce".into());
            }
            if let Some(ready_tx) = ready_tx {
                let _ = ready_tx.send(Ok(()));
            }
            Ok(super::spawn_result(&tab_id))
        },
    )
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn unplanned_worker_spawn_creates_and_dispatches_an_unplanned_task() {
    let harness = fixture();
    let response = dispatch(&harness, &params(&harness, "urgent probe")).unwrap();
    let task_id = response["task_id"].as_str().unwrap();
    let task = harness
        .fixture
        .service
        .plan_store
        .show_task(task_id)
        .unwrap()
        .task;
    assert_eq!(task.kind, "unplanned");
    assert!(task.tags.iter().any(|tag| tag == "unplanned"));
    assert_eq!(task.name, "unplanned: urgent probe");
    assert_eq!(task.body, "Run the urgent probe.");
    assert_eq!(task.status, TaskStatus::Pending);
    assert_eq!(task.execution_attempts.len(), 1);
    assert!(task.lane.worktree_path.is_some());
    assert_ne!(
        task.lane.worktree_path.as_deref(),
        Some(harness.project.path().to_str().unwrap())
    );
    let tab_id = response["tab_id"].as_str().unwrap();
    let attempt_id = response["attempt_id"].as_str().unwrap();
    harness
        .fixture
        .service
        .plan_store
        .acknowledge_task_attempt(&harness.plan_id, task_id, tab_id, attempt_id, 11)
        .unwrap();
    assert_eq!(
        harness
            .fixture
            .service
            .plan_store
            .show_task(task_id)
            .unwrap()
            .task
            .status,
        TaskStatus::InProgress
    );
    let sibling_id = harness
        .fixture
        .service
        .plan_store
        .add_task(
            &harness.plan_id,
            NewTask {
                name: "unapproved sibling".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            12,
        )
        .unwrap();
    let mut sibling_params = params(&harness, "").unwrap();
    sibling_params["task_id"] = json!(sibling_id);
    sibling_params["execution_intent"] = json!("mutating");
    let sibling_error = super::super::dispatch_with_spawn(
        &Some(sibling_params),
        harness.fixture._app.handle(),
        &std::sync::Arc::new(harness.fixture.service.clone()),
        13,
        |_, _, _| panic!("an unapproved sibling must not inherit the exemption"),
        || {},
    )
    .unwrap_err();
    assert_eq!(
        harness
            .fixture
            .service
            .plan_store
            .get(&harness.plan_id)
            .unwrap()
            .planning_state,
        PlanningState::Drafting
    );
    assert_eq!(sibling_error.code, -32003);
    assert_eq!(
        sibling_error.data.as_ref().unwrap()["type"],
        "plan_orchestrator_authorization_required"
    );
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn unplanned_worker_requires_reason_and_orchestrator_authority() {
    let harness = fixture();
    let empty_reason = dispatch(&harness, &params(&harness, "   ")).unwrap_err();
    assert!(empty_reason.message.contains("non-empty --reason"));
    let mut canonical = params(&harness, "urgent probe").unwrap();
    canonical["placement"] = json!("canonical_writer");
    let canonical_error = dispatch(&harness, &Some(canonical)).unwrap_err();
    assert!(canonical_error.message.contains("isolated lane"));
    let mut gates = params(&harness, "urgent probe").unwrap();
    gates["authorized_gates"] = json!(["gate-token"]);
    let gate_error = dispatch(&harness, &Some(gates)).unwrap_err();
    assert!(gate_error.message.contains("authorized_gates"));

    let mut mux = harness.fixture.service.mux.write();
    let workspace = mux.list_workspaces()[0].id.clone();
    let worker = mux.create_tab(&workspace).unwrap();
    mux.set_tab_role(
        &worker,
        crate::multiplexer::types::AgentRole::Worker,
        Some(&harness.owner_tab),
    )
    .unwrap();
    let worker_tab = mux.get_tab_mut(&worker).unwrap();
    worker_tab.agent_session_id = Some("worker-session".into());
    worker_tab.session_binding_nonce = Some("worker-nonce".into());
    worker_tab.reported_session_binding_nonce = Some("worker-nonce".into());
    drop(mux);
    let mut worker_params = params(&harness, "urgent probe").unwrap();
    worker_params["caller_tab_id"] = json!(worker);
    worker_params["session_id"] = json!("worker-session");
    worker_params["session_binding_nonce"] = json!("worker-nonce");
    let worker_error = dispatch(&harness, &Some(worker_params)).unwrap_err();
    assert_eq!(
        worker_error.data.unwrap()["type"],
        "plan_orchestrator_authorization_required"
    );

    let mut planner_params = params(&harness, "urgent probe").unwrap();
    let planner_tab = {
        let mut mux = harness.fixture.service.mux.write();
        let workspace = mux.list_workspaces()[0].id.clone();
        let planner = mux.create_tab(&workspace).unwrap();
        let tab = mux.get_tab_mut(&planner).unwrap();
        tab.role = Some(crate::multiplexer::types::AgentRole::Manager);
        tab.role_profile = Some("planner".into());
        tab.parent_tab_id = Some(harness.owner_tab.clone());
        tab.team_id = Some(harness.team_id.clone());
        tab.agent_session_id = Some("planner-session".into());
        tab.session_binding_nonce = Some("planner-nonce".into());
        tab.reported_session_binding_nonce = Some("planner-nonce".into());
        planner
    };
    planner_params["caller_tab_id"] = json!(planner_tab);
    planner_params["session_id"] = json!("planner-session");
    planner_params["session_binding_nonce"] = json!("planner-nonce");
    let planner_error = dispatch(&harness, &Some(planner_params)).unwrap_err();
    assert_eq!(
        planner_error.data.unwrap()["type"],
        "plan_orchestrator_authorization_required"
    );
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn team_close_cleans_up_unplanned_worker() {
    let harness = fixture();
    let response = dispatch(&harness, &params(&harness, "urgent probe")).unwrap();
    let task_id = response["task_id"].as_str().unwrap().to_string();
    let tab_id = response["tab_id"].as_str().unwrap().to_string();
    {
        let mux = harness.fixture.service.mux.read();
        let worker = mux.get_tab(&tab_id).unwrap();
        assert_eq!(worker.team_id.as_deref(), Some(harness.team_id.as_str()));
        assert_eq!(worker.parent_tab_id.as_deref(), Some(harness.owner_tab.as_str()));
        assert_eq!(worker.role, Some(crate::multiplexer::types::AgentRole::Worker));
    }
    tauri::async_runtime::block_on(harness.fixture.service.close_team(
        &harness.team_id,
        TeamMutationCaller::AgentTab(harness.owner_tab.clone()),
    ))
    .unwrap();
    assert!(harness
        .fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .is_none());
    // Team close may retire the Plan with its tasks; a surviving task must be closed out.
    if let Some(shown) = harness.fixture.service.plan_store.show_task(&task_id) {
        assert_eq!(shown.task.status, TaskStatus::Cancelled);
        assert!(shown.task.execution_attempts[0].closed_at.is_some());
    }
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn unplanned_worker_dispatch_failure_removes_created_task() {
    let harness = fixture();
    let before = harness
        .fixture
        .service
        .plan_store
        .get(&harness.plan_id)
        .unwrap()
        .tasks
        .len();
    let error = super::super::unplanned::dispatch_unplanned_with_spawn(
        &params(&harness, "urgent probe"),
        harness.fixture._app.handle(),
        &std::sync::Arc::new(harness.fixture.service.clone()),
        10,
        |_, _: SpawnAgentParams, _| Err("simulated provider launch failure".into()),
    )
    .unwrap_err();
    assert_eq!(error.data.unwrap()["stage"], "agent_spawn");
    assert_eq!(
        harness
            .fixture
            .service
            .plan_store
            .get(&harness.plan_id)
            .unwrap()
            .tasks
            .len(),
        before
    );
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn unplanned_worker_refuses_unbound_team() {
    let harness = fixture();
    let error = super::super::unplanned::dispatch_unplanned_with_spawn(
        &Some(json!({ "reason": "urgent probe", "team_id": "unbound-team" })),
        harness.fixture._app.handle(),
        &std::sync::Arc::new(harness.fixture.service.clone()),
        10,
        |_, _, _| panic!("unbound Team must be refused before launch"),
    )
    .unwrap_err();
    assert!(error.message.contains("set the Team goal first"));
}
