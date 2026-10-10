use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::TaskBinding;
use crate::plan::{OrchestratorRef, Plan, PlanMode, TaskLaneMetadata};
use crate::service::plan::task_assignment::AssignmentRuntimeBinding;
use crate::service::test_fixture::ServiceFixture;
use codeterm_types::agent::role_kind::builtins;
use codeterm_types::team::TeamLifecycle;

struct RouteFixture {
    service: ServiceFixture,
    source: String,
    worker: String,
    destination: String,
    plan_id: String,
}

impl RouteFixture {
    fn new() -> Self {
        let (mut mux, source) = MultiplexerState::new();
        let workspace_id = mux.find_workspace_for_tab(&source).unwrap();
        let destination = mux.create_tab(&workspace_id).unwrap();
        let worker = mux.create_tab(&workspace_id).unwrap();
        arm(&mut mux, &source, "source");
        arm(&mut mux, &destination, "destination");
        arm(&mut mux, &worker, "worker");
        mux.get_tab_mut(&source).unwrap().pty_id = Some("pty-source".into());
        mux.get_tab_mut(&destination).unwrap().pty_id = Some("pty-destination".into());
        mux.get_tab_mut(&worker).unwrap().pty_id = Some("pty-worker".into());
        mux.set_orchestrator_role(&source).unwrap();
        mux.set_orchestrator_role(&destination).unwrap();
        let source_team = mux
            .resolve_or_mint_team_for_parent(&source)
            .unwrap()
            .team_id;
        let destination_team = mux
            .resolve_or_mint_team_for_parent(&destination)
            .unwrap()
            .team_id;
        mux.set_team_lifecycle(&source_team, TeamLifecycle::Active)
            .unwrap();
        mux.set_team_lifecycle(&destination_team, TeamLifecycle::Active)
            .unwrap();
        mux.set_tab_role_kind(
            &worker,
            AgentRole::Worker,
            Some(builtins::WORKER),
            Some(&source),
        )
        .unwrap();
        let service = ServiceFixture::new(mux);
        let mut plan = Plan::new(
            "transfer route".into(),
            service.temp.path().display().to_string(),
            OrchestratorRef::from_tab(&destination),
        );
        plan.team_id = Some(destination_team);
        plan.mode = PlanMode::Approved;
        service.service.plan_store.save(&mut plan, 1).unwrap();
        let worktree = service
            .temp
            .path()
            .join("worker-lane")
            .display()
            .to_string();
        let task_id = service
            .service
            .plan_store
            .add_task(
                &plan.id,
                crate::service::plan::NewTask {
                    name: "route-bound task".into(),
                    kind: "implement".into(),
                    lane: TaskLaneMetadata {
                        tab_id: Some(worker.clone()),
                        workspace_id: Some(workspace_id),
                        worktree_path: Some(worktree.clone()),
                        branch: Some("lane/route-transfer".into()),
                        base: Some("base".into()),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                2,
            )
            .unwrap();
        service
            .service
            .plan_store
            .assign_task_with_runtime_binding(
                &plan.id,
                &task_id,
                &worker,
                None,
                Some("Worker"),
                Some(&AssignmentRuntimeBinding {
                    session_id: "session-worker".into(),
                    session_binding_nonce: "nonce-worker".into(),
                    provider: Some("codex".into()),
                    model: Some("gpt-6-luna".into()),
                    session_model: None,
                    reasoning_effort: Some("xhigh".into()),
                }),
                None,
                3,
            )
            .unwrap();
        let attempt_id = service.service.plan_store.get(&plan.id).unwrap().tasks[0]
            .execution_attempts[0]
            .id
            .clone();
        service
            .service
            .plan_store
            .acknowledge_task_attempt(&plan.id, &task_id, &worker, &attempt_id, 4)
            .unwrap();
        {
            let mut mux = service.service.mux.write();
            let tab = mux.get_tab_mut(&worker).unwrap();
            tab.cwd = Some(worktree);
            tab.task_binding = Some(TaskBinding {
                plan_id: Some(plan.id.clone()),
                task_id: Some(task_id),
                attempt_id: Some(attempt_id),
                spawn_handle: None,
                bound_at: 2,
            });
        }
        Self {
            service,
            source,
            worker,
            destination,
            plan_id: plan.id,
        }
    }
}

fn arm(mux: &mut MultiplexerState, tab_id: &str, suffix: &str) {
    let tab = mux.get_tab_mut(tab_id).unwrap();
    tab.agent_type = Some("codex".into());
    tab.chat_view = true;
    tab.agent_session_id = Some(format!("session-{suffix}"));
    tab.session_binding_nonce = Some(format!("nonce-{suffix}"));
    tab.reported_session_binding_nonce = Some(format!("nonce-{suffix}"));
}

fn headers(tab_id: &str, suffix: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("X-CodeTerm-Tab-Id", tab_id.parse().unwrap());
    headers.insert(
        "X-CodeTerm-Session-Id",
        format!("session-{suffix}").parse().unwrap(),
    );
    headers.insert(
        "X-CodeTerm-Binding-Nonce",
        format!("nonce-{suffix}").parse().unwrap(),
    );
    headers
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn worker_transfer_role_endpoint_accepts_only_source_manager_headers_and_returns_typed_commit_outcome(
) {
    let fixture = RouteFixture::new();
    let before_plan = fixture
        .service
        .service
        .plan_store
        .get(&fixture.plan_id)
        .unwrap();
    let missing = HeaderMap::new();
    assert!(try_task_bound_worker_transfer(
        &fixture.service.service,
        &missing,
        &fixture.worker,
        AgentRole::Worker,
        None,
        None,
        Some(&fixture.destination),
        None,
    )
    .is_err());
    assert_eq!(
        fixture
            .service
            .service
            .plan_store
            .get(&fixture.plan_id)
            .unwrap(),
        before_plan
    );

    let wrong_session = headers(&fixture.source, "wrong");
    assert!(try_task_bound_worker_transfer(
        &fixture.service.service,
        &wrong_session,
        &fixture.worker,
        AgentRole::Worker,
        None,
        None,
        Some(&fixture.destination),
        None,
    )
    .is_err());
    let conflict = headers(&fixture.source, "source");
    assert!(try_task_bound_worker_transfer(
        &fixture.service.service,
        &conflict,
        &fixture.worker,
        AgentRole::Worker,
        Some("conflicting-profile"),
        None,
        Some(&fixture.destination),
        None,
    )
    .is_err());
    assert!(try_task_bound_worker_transfer(
        &fixture.service.service,
        &conflict,
        &fixture.worker,
        AgentRole::Worker,
        None,
        Some("another-plan"),
        Some(&fixture.destination),
        None,
    )
    .is_err());
    assert!(try_task_bound_worker_transfer(
        &fixture.service.service,
        &conflict,
        &fixture.worker,
        AgentRole::Worker,
        None,
        None,
        Some(&fixture.destination),
        Some(false),
    )
    .is_err());

    let valid = headers(&fixture.source, "source");
    let outcome = try_task_bound_worker_transfer(
        &fixture.service.service,
        &valid,
        &fixture.worker,
        AgentRole::Worker,
        None,
        None,
        Some(&fixture.destination),
        None,
    )
    .unwrap()
    .unwrap();

    assert_eq!(outcome["status"], "committed_observation_unconfirmed");
    assert_eq!(outcome["worker_tab_id"], fixture.worker);
    assert_eq!(outcome["destination_manager_tab_id"], fixture.destination);
    assert!(!outcome.to_string().contains("nonce-worker"));
    assert_eq!(
        fixture
            .service
            .service
            .plan_store
            .get(&fixture.plan_id)
            .unwrap(),
        before_plan
    );
}
