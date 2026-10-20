use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::plan::OrchestratorRef;

use crate::test_fixture::ServiceFixture;
use crate::TeamExt;

#[tokio::test]
async fn human_force_close_of_team_workspace_tears_down_team_via_close_team() {
    use codeterm_service_core::cron::store::{CronCreateParams, CronStore};
    use codeterm_service_core::multiplexer::types::AgentRole;
    use codeterm_service_core::service::plan::NewTask;
    use parking_lot::Mutex;
    use std::sync::Arc;

    let (mut mux, orchestrator) = MultiplexerState::new();
    let team_id = mux
        .resolve_or_mint_team_for_parent(&orchestrator)
        .unwrap()
        .team_id;
    let workspace_id = mux.find_workspace_for_tab(&orchestrator).unwrap();
    mux.get_tab_mut(&orchestrator).unwrap().agent_type = Some("fixture".into());
    let worker = mux.create_tab(&workspace_id).unwrap();
    mux.set_tab_role(&worker, AgentRole::Worker, Some(&orchestrator))
        .unwrap();
    mux.get_tab_mut(&worker).unwrap().agent_type = Some("fixture".into());
    let second_orchestrator = mux.create_tab(&workspace_id).unwrap();
    let second_team_id = mux
        .resolve_or_mint_team_for_parent(&second_orchestrator)
        .unwrap()
        .team_id;
    mux.get_tab_mut(&second_orchestrator).unwrap().agent_type = Some("fixture".into());

    let fixture = ServiceFixture::new(mux);
    let plan_store = &fixture.service.plan_store;
    let plan = plan_store
        .create_plan_for_team(
            "Workspace close cleanup".into(),
            fixture.temp.path().display().to_string(),
            OrchestratorRef::from_tab(&orchestrator),
            team_id.clone(),
            vec![],
            1,
        )
        .unwrap();
    let task_id = plan_store
        .add_task(
            &plan.id,
            NewTask {
                name: "Assigned lane".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    plan_store
        .assign_task(&plan.id, &task_id, &worker, None, 3)
        .unwrap();
    let mut plan_with_open_attempt = plan_store.get(&plan.id).unwrap();
    let assigned_task = plan_with_open_attempt
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .unwrap();
    assigned_task.status = codeterm_service_core::plan::TaskStatus::InProgress;
    assigned_task
        .execution_attempts
        .push(codeterm_service_core::plan::TaskExecutionAttempt {
            id: "attempt_workspace_close".into(),
            executor_tab_id: worker.clone(),
            ..Default::default()
        });
    plan_store.save(&mut plan_with_open_attempt, 4).unwrap();

    let cron = Arc::new(Mutex::new(CronStore::new_with_path(
        fixture.temp.path().join("crons.json"),
    )));
    let cron_wake = Arc::new(std::sync::Condvar::new());
    cron.lock()
        .create(CronCreateParams {
            name: "Team cadence".into(),
            workspace_id: workspace_id.clone(),
            team_id: Some(team_id.clone()),
            detach_from_team: false,
            schedule: "0 0 * * *".into(),
            task: "Check progress".into(),
            description: None,
            agent: Some("fixture".into()),
            model: None,
            reasoning_effort: None,
            cwd: None,
            role: None,
            skill: None,
            creator_tab_id: Some(orchestrator.clone()),
            report_target_tab_id: None,
            enabled: Some(true),
            max_history: None,
            oneshot: None,
            jitter_max_ms: None,
            tz: None,
            timeout_seconds: None,
        })
        .unwrap();
    assert!(fixture.service.set_ext(cron.clone()).is_ok());
    assert!(fixture.service.set_ext(cron_wake).is_ok());

    fixture
        .service
        .close_workspace_as_human(&workspace_id, true)
        .await
        .unwrap();

    let mux = fixture.service.mux.read();
    assert!(mux.get_workspace(&workspace_id).is_none());
    assert!(mux.get_team(&team_id).is_none());
    assert!(mux.get_team(&second_team_id).is_none());
    assert!(mux.get_tab(&orchestrator).is_none());
    assert!(mux.get_tab(&worker).is_none());
    assert!(mux.get_tab(&second_orchestrator).is_none());
    drop(mux);
    assert!(cron.lock().list_for_team(&team_id).is_empty());
    let closed_plan = plan_store.get(&plan.id).unwrap();
    let closed_task = closed_plan
        .tasks
        .iter()
        .find(|task| task.id == task_id)
        .unwrap();
    assert_eq!(
        closed_task.status,
        codeterm_service_core::plan::TaskStatus::Pending
    );
    assert!(closed_task.execution_attempts.iter().all(|attempt| {
        attempt.closed_at.is_some() && attempt.closure_reason.as_deref() == Some("team_closed")
    }));
    assert_ne!(
        closed_task.lane.cleanup_status.as_deref(),
        Some("cleanup_debt")
    );
    assert!(closed_task.lane.tab_id.is_none());
}

#[tokio::test]
async fn close_team_via_service_persists_the_reduced_desk() {
    use codeterm_service_core::service::TeamMutationCaller;
    use codeterm_service_core::session::store::StorageDeskAdapter;

    let (mut mux, orchestrator) = MultiplexerState::new();
    let team_id = mux
        .resolve_or_mint_team_for_parent(&orchestrator)
        .unwrap()
        .team_id;
    let workspace_id = mux.find_workspace_for_tab(&orchestrator).unwrap();
    let worker = mux.create_tab(&workspace_id).unwrap();
    mux.set_tab_role(
        &worker,
        codeterm_service_core::multiplexer::types::AgentRole::Worker,
        Some(&orchestrator),
    )
    .unwrap();
    let bystander = mux.create_tab(&workspace_id).unwrap();

    let fixture = ServiceFixture::new(mux);
    let storage = codeterm_service_core::storage::Storage::open(
        fixture.temp.path().join("workspace-close-storage"),
        codeterm_service_core::storage::StorageConfig::default(),
    )
    .unwrap();
    let adapter = StorageDeskAdapter::from_storage(storage.clone());
    adapter
        .import_snapshot(
            &fixture.service.mux.read().to_session_snapshot(),
            Vec::new(),
        )
        .expect("seed the Storage desk before Team close");
    codeterm_service_core::service::session_persist::install_storage_desk_adapter(
        &fixture.service,
        StorageDeskAdapter::from_storage(storage.clone()),
    )
    .unwrap();
    let persisted_ids =
        |snapshot: &codeterm_service_core::session::store::SessionSnapshot| -> Vec<String> {
            snapshot
                .workspaces
                .iter()
                .flat_map(|ws| ws.tabs.iter().map(|tab| tab.id.clone()))
                .collect()
        };
    let before = adapter.snapshot().expect("desk persisted");
    let before_ids = persisted_ids(&before);
    assert!(
        before_ids.contains(&orchestrator),
        "team desk persisted first"
    );
    assert!(before_ids.contains(&worker));

    // The REAL service boundary, not the close_team_with_state_change helper.
    fixture
        .service
        .close_team(&team_id, TeamMutationCaller::Human, false)
        .await
        .unwrap();

    codeterm_service_core::service::session_persist::flush_session_persistence(&fixture.service)
        .unwrap();
    let after = adapter.snapshot().expect("desk re-persisted");
    let after_ids = persisted_ids(&after);
    assert!(
        !after_ids.contains(&orchestrator) && !after_ids.contains(&worker),
        "closed Team members must not survive in the persisted desk to resurrect on restart"
    );
    assert!(
        after_ids.contains(&bystander),
        "non-member tab stays persisted"
    );
    assert!(
        after.teams.iter().all(|team| team.id != team_id),
        "closed Team is gone from the snapshot"
    );
    codeterm_service_core::service::session_persist::stop_worker(&fixture.service).unwrap();
    storage
        .shutdown()
        .wait(std::time::Duration::from_secs(30))
        .unwrap();
}

#[tokio::test]
async fn workspace_close_after_team_close_finds_existing_workspace() {
    use codeterm_service_core::service::TeamMutationCaller;

    let (mut mux, orchestrator) = MultiplexerState::new();
    let team_id = mux
        .resolve_or_mint_team_for_parent(&orchestrator)
        .unwrap()
        .team_id;
    let existing_workspace = mux.create_workspace("Other".into(), None);
    let fixture = ServiceFixture::new(mux);

    fixture
        .service
        .close_team(&team_id, TeamMutationCaller::Human, false)
        .await
        .unwrap();
    fixture
        .service
        .close_workspace_as_human(&existing_workspace.id, true)
        .await
        .unwrap();
}
