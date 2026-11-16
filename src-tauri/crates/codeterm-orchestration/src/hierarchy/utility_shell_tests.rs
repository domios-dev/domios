use crate::test_fixture::ServiceFixture;
use crate::traits::*;
use codeterm_service_core::agent::reports::ReportDetails;
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::multiplexer::types::{AgentRole, ShellPurpose};
use codeterm_service_core::plan::{OrchestratorRef, Plan};
use codeterm_service_core::service::plan::NewTask;
use codeterm_service_core::service::spawn::SpawnTabSpec;
#[test]
fn worker_owned_utility_shell_has_no_task_authority_and_reports_to_worker() {
    let (mut mux, orchestrator_id) = MultiplexerState::new();
    mux.set_orchestrator_role(&orchestrator_id).unwrap();
    let team_id = mux
        .resolve_or_mint_team_for_parent(&orchestrator_id)
        .unwrap()
        .team_id;
    let workspace_id = mux.find_workspace_for_tab(&orchestrator_id).unwrap();
    let worker_id = mux.create_tab(&workspace_id).unwrap();
    mux.set_tab_role(&worker_id, AgentRole::Worker, Some(&orchestrator_id))
        .unwrap();
    let fixture = ServiceFixture::new(mux);
    let mut plan = Plan::new(
        "utility shell owner".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef::from_tab(&orchestrator_id),
    );
    fixture.service.plan_store.save(&mut plan, 1).unwrap();
    let task_id = fixture
        .service
        .plan_store
        .add_task(
            &plan.id,
            NewTask {
                name: "worker shell report".into(),
                kind: "implement".into(),
                acceptance_criteria: vec!["the report reaches the Worker".into()],
                ..Default::default()
            },
            2,
        )
        .unwrap();
    fixture
        .service
        .plan_store
        .assign_task(&plan.id, &task_id, &worker_id, None, 3)
        .unwrap();
    let delivered =
        fixture.bind_recorded_agent_recipient(&worker_id, "worker-session", "worker-nonce");

    let shell = fixture
        .service
        .spawn_visible_shell_impl(SpawnTabSpec {
            workspace_id: Some(workspace_id),
            team_id: Some(team_id.clone()),
            creator_tab_id: Some(worker_id.clone()),
            purpose: ShellPurpose::Build,
            ..Default::default()
        })
        .unwrap();
    let tab = fixture
        .service
        .mux
        .read()
        .get_tab(&shell.tab_id)
        .unwrap()
        .clone();
    assert_eq!(tab.parent_tab_id.as_deref(), Some(worker_id.as_str()));
    assert_eq!(tab.spawned_by_tab_id.as_deref(), Some(worker_id.as_str()));
    assert_eq!(tab.team_id.as_deref(), Some(team_id.as_str()));
    assert_eq!(tab.role, None);

    for env in [
        fixture.service.tab_env(&shell.tab_id).unwrap(),
        fixture
            .service
            .frontend_pty_env(Some(&shell.tab_id))
            .unwrap(),
    ] {
        assert_eq!(
            env.get("CODETERM_PARENT_TAB_ID").map(String::as_str),
            Some(worker_id.as_str())
        );
        assert_eq!(env["CODETERM_TEAM_ID"], team_id);
        for key in [
            "CODETERM_ROLE",
            "CODETERM_PLAN_ID",
            "CODETERM_TASK_ID",
            "CODETERM_ATTEMPT_ID",
            "CODETERM_LANE_BASE",
            "CODETERM_WORKER_ID",
        ] {
            assert!(!env.contains_key(key), "{key} leaked into a utility shell");
        }
    }

    let before = serde_json::to_value(fixture.service.plan_store.get(&plan.id)).unwrap();
    for (index, (plan_hint, task_hint)) in [
        (None, None),
        (Some(plan.id.as_str()), None),
        (Some(plan.id.as_str()), Some(task_id.as_str())),
    ]
    .into_iter()
    .enumerate()
    {
        let payload = format!("utility-result-{index}");
        let report = fixture
            .service
            .submit_report(
                &shell.tab_id,
                &payload,
                ReportDetails {
                    plan_id: plan_hint.map(str::to_string),
                    task_id: task_hint.map(str::to_string),
                    attempt_id: Some("stale-parent-attempt".into()),
                    status: task_hint
                        .map(|_| codeterm_service_core::agent::reports::ReportStatus::InReview),
                    caller_tab_id: Some(shell.tab_id.clone()),
                    session_binding_nonce: Some("shell-without-agent-session".into()),
                    ..Default::default()
                },
                plan_hint,
                task_hint,
                None,
            )
            .unwrap();

        assert_eq!(report.orchestrator_tab_id, worker_id);
        assert!(report.persisted_task.is_none());
        assert!(report.persisted_plan_evidence.is_none());
        assert!(report.rejected_task_transition.is_none());
        assert_eq!(
            delivered
                .texts
                .lock()
                .iter()
                .filter(|text| text.contains(&payload))
                .count(),
            1
        );
        let stored = fixture.service.report_store.lock().get(&worker_id, false);
        let entry = stored
            .iter()
            .find(|entry| entry.message == payload)
            .unwrap();
        assert!(entry.plan_id.is_none());
        assert!(entry.task_id.is_none());
        assert!(entry.attempt_id.is_none());
        assert!(entry.status.is_none());
    }
    assert_eq!(
        serde_json::to_value(fixture.service.plan_store.get(&plan.id)).unwrap(),
        before
    );
}
