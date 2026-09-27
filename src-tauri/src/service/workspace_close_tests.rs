use super::WorkspaceCloseCaller;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{PendingPrompt, PromptOption};
use crate::plan::{OrchestratorRef, Plan};
use crate::service::plan::NewTask;
use crate::service::test_fixture::ServiceFixture;

#[test]
fn workspace_close_log_caller_distinguishes_confirmed_headerless_humans() {
    assert_eq!(
        WorkspaceCloseCaller::HumanConfirmedHeaderless.kind(),
        "human-attributed (headerless, confirmed)",
    );
    assert_eq!(WorkspaceCloseCaller::Unattributed.kind(), "Unattributed");
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn workspace_close_skips_peer_anchored_team_ids() {
    let (mut mux, root_tab_id) = MultiplexerState::new();
    let workspace_id = mux.find_workspace_for_tab(&root_tab_id).unwrap();
    let remote_tab_id = mux.create_tab(&workspace_id).unwrap();
    let remote_team = codeterm_types::team::Team::new(workspace_id.clone(), 0);
    let remote_team_id = remote_team.id.clone();
    let qualified_remote_team_id = format!("mesh:peer-123/{remote_team_id}");
    mux.upsert_team(remote_team);
    let remote_tab = mux.get_tab_mut(&remote_tab_id).unwrap();
    remote_tab.team_id = Some(qualified_remote_team_id.clone());
    remote_tab.ownership = Some(codeterm_types::pty::RemoteOwnership {
        controller_title: "Remote coordinator".into(),
        controller_tab_id: None,
        peer_id: "peer-123".into(),
        peer_name: "Remote peer".into(),
        team: Some(codeterm_types::pty::RemoteOwnershipTeam {
            id: remote_team_id.clone(),
            title: None,
        }),
    });
    let fixture = ServiceFixture::new(mux);

    let team_ids = fixture.service.workspace_team_ids(&workspace_id);
    assert!(!team_ids.contains(&remote_team_id));
    assert!(!team_ids.contains(&qualified_remote_team_id));
}

fn prompt(question: &str) -> PendingPrompt {
    PendingPrompt {
        question: question.into(),
        options: vec![PromptOption {
            label: "accept".into(),
            value: "1".into(),
        }],
        sections: None,
        multi_select: None,
        submit_available: None,
        selected_values: None,
    }
}

fn protected_workspace() -> (ServiceFixture, String, String) {
    let (mut mux, coordinator) = MultiplexerState::new();
    let workspace_id = mux.find_workspace_for_tab(&coordinator).unwrap();
    mux.set_orchestrator_role(&coordinator).unwrap();
    (ServiceFixture::new(mux), workspace_id, coordinator)
}

fn workspace_with_agent_tab() -> (ServiceFixture, String, String, String) {
    let (mut mux, caller_tab) = MultiplexerState::new();
    let workspace_id = mux.create_workspace("Agent workspace".into(), None).id;
    let tab_id = mux.create_tab(&workspace_id).unwrap();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("fixture".into());
    tab.title = "Release guard agent".into();
    (ServiceFixture::new(mux), workspace_id, tab_id, caller_tab)
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn workspace_close_guard_unconfirmed_agent_tabs_remain() {
    let (fixture, workspace_id, tab_id, caller_tab) = workspace_with_agent_tab();

    assert!(fixture
        .service
        .close_workspace_guarded(Some(&caller_tab), &workspace_id, false)
        .is_err());
    let mux = fixture.service.mux.read();
    assert!(mux.get_workspace(&workspace_id).is_some());
    assert!(mux.get_tab(&tab_id).is_some());
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn optional_body_contract_workspace_agent_guard_is_reported_as_forbidden() {
    let (fixture, workspace_id, _, caller_tab) = workspace_with_agent_tab();
    let error = fixture
        .service
        .close_workspace_guarded(Some(&caller_tab), &workspace_id, false)
        .unwrap_err();
    let (status, message) = crate::api::tab_routes::map_close_error(error);

    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
    assert!(message.contains("Release guard agent"));
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn workspace_close_guard_confirmed_agent_tabs_close() {
    let (fixture, workspace_id, tab_id, caller_tab) = workspace_with_agent_tab();

    fixture
        .service
        .close_workspace_guarded(Some(&caller_tab), &workspace_id, true)
        .unwrap();
    let mux = fixture.service.mux.read();
    assert!(mux.get_workspace(&workspace_id).is_none());
    assert!(mux.get_tab(&tab_id).is_none());
}

#[tokio::test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
async fn workspace_close_guard_human_can_close_empty_workspace() {
    let (mut mux, _) = MultiplexerState::new();
    let workspace_id = mux.create_workspace("Empty".into(), None).id;
    let fixture = ServiceFixture::new(mux);

    fixture
        .service
        .close_workspace_as_human(&workspace_id, false)
        .await
        .unwrap();
    assert!(fixture
        .service
        .mux
        .read()
        .get_workspace(&workspace_id)
        .is_none());
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn unattributed_close_cannot_close_an_unprotected_workspace() {
    let (mut mux, _) = MultiplexerState::new();
    let workspace_id = mux.create_workspace("Unattributed".into(), None).id;
    let fixture = ServiceFixture::new(mux);

    let error = fixture
        .service
        .close_workspace_guarded(None, &workspace_id, false)
        .unwrap_err();

    assert!(error.starts_with(crate::service::AGENT_TAB_CLOSE_REFUSAL_PREFIX));
    assert!(fixture
        .service
        .mux
        .read()
        .get_workspace(&workspace_id)
        .is_some());
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn workspace_close_guard_mesh_requires_force() {
    let (fixture, workspace_id, tab_id, _) = workspace_with_agent_tab();

    assert!(fixture
        .service
        .close_workspace_from_authenticated_mesh_peer("paired-peer", &workspace_id, false)
        .is_err());
    assert!(fixture.service.mux.read().get_tab(&tab_id).is_some());
    fixture
        .service
        .close_workspace_from_authenticated_mesh_peer("paired-peer", &workspace_id, true)
        .unwrap();
    assert!(fixture.service.mux.read().get_tab(&tab_id).is_none());
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn unattributed_close_refuses_a_protected_workspace() {
    let (fixture, workspace_id, _) = protected_workspace();

    let error = fixture
        .service
        .close_workspace_guarded(None, &workspace_id, false)
        .unwrap_err();

    assert!(error.starts_with(crate::service::AGENT_TAB_CLOSE_REFUSAL_PREFIX));
    assert!(fixture
        .service
        .mux
        .read()
        .get_workspace(&workspace_id)
        .is_some());
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn unknown_local_tab_header_cannot_bypass_workspace_close_protection() {
    let (fixture, workspace_id, _) = protected_workspace();

    let error = fixture
        .service
        .close_workspace_guarded(Some("forged-tab"), &workspace_id, false)
        .unwrap_err();

    assert!(error.starts_with(crate::service::tab::SELF_CLOSE_REFUSAL_PREFIX));
    assert!(fixture
        .service
        .mux
        .read()
        .get_workspace(&workspace_id)
        .is_some());
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn agent_coordinator_self_close_of_workspace_is_still_refused() {
    let (fixture, workspace_id, coordinator) = protected_workspace();

    let error = fixture
        .service
        .close_workspace_guarded(Some(&coordinator), &workspace_id, true)
        .unwrap_err();

    assert!(error.starts_with(crate::service::tab::SELF_CLOSE_REFUSAL_PREFIX));
    assert!(fixture
        .service
        .mux
        .read()
        .get_workspace(&workspace_id)
        .is_some());
}

#[tokio::test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
async fn human_force_close_of_team_workspace_tears_down_team_via_close_team() {
    use crate::cron::store::{CronCreateParams, CronStore};
    use crate::multiplexer::types::AgentRole;
    use crate::service::plan::NewTask;
    use parking_lot::Mutex;
    use std::sync::Arc;
    use tauri::Manager;

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
    assigned_task.status = crate::plan::TaskStatus::InProgress;
    assigned_task
        .execution_attempts
        .push(crate::plan::TaskExecutionAttempt {
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
            schedule: "0 0 * * * *".into(),
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
    fixture._app.manage(cron.clone());
    fixture._app.manage(cron_wake);

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
    assert_eq!(closed_task.status, crate::plan::TaskStatus::Pending);
    assert!(closed_task.execution_attempts.iter().all(|attempt| {
        attempt.closed_at.is_some() && attempt.closure_reason.as_deref() == Some("team_closed")
    }));
    assert_eq!(closed_task.lane.cleanup_status.as_deref(), Some("done"));
    assert!(closed_task.lane.tab_id.is_none());
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn authenticated_mesh_peer_may_close_a_protected_workspace() {
    let (fixture, workspace_id, _) = protected_workspace();

    fixture
        .service
        .close_workspace_from_authenticated_mesh_peer("paired-peer", &workspace_id, true)
        .unwrap();

    assert!(fixture
        .service
        .mux
        .read()
        .get_workspace(&workspace_id)
        .is_none());
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn empty_mesh_peer_provenance_is_rejected() {
    let (fixture, workspace_id, _) = protected_workspace();

    let error = fixture
        .service
        .close_workspace_from_authenticated_mesh_peer(" ", &workspace_id, false)
        .unwrap_err();

    assert!(error.contains("peer id"));
    assert!(fixture
        .service
        .mux
        .read()
        .get_workspace(&workspace_id)
        .is_some());
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn workspace_close_tears_down_prompt_state_for_every_tab() {
    let (mut mux, first_tab) = MultiplexerState::new();
    let workspace_id = mux.find_workspace_for_tab(&first_tab).unwrap();
    let second_tab = mux.create_tab(&workspace_id).unwrap();
    let fixture = ServiceFixture::new(mux);

    fixture
        .service
        .prompt_detector
        .lock()
        .hook_detected_explicit(&first_tab, prompt("first"));
    fixture
        .service
        .prompt_detector
        .lock()
        .hook_detected_explicit(&second_tab, prompt("second"));

    fixture.service.close_workspace(&workspace_id).unwrap();

    let detector = fixture.service.prompt_detector.lock();
    assert!(detector.active_prompt(&first_tab).is_none());
    assert!(detector.active_prompt(&second_tab).is_none());
    drop(detector);
    let mux = fixture.service.mux.read();
    assert!(mux.get_tab(&first_tab).is_none());
    assert!(mux.get_tab(&second_tab).is_none());
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn workspace_close_keeps_one_workspace_restore_group() {
    let (mut mux, first_tab) = MultiplexerState::new();
    let workspace_id = mux.find_workspace_for_tab(&first_tab).unwrap();
    let second_tab = mux.create_tab(&workspace_id).unwrap();
    let fixture = ServiceFixture::new(mux);

    fixture.service.close_workspace(&workspace_id).unwrap();

    let view = fixture.service.mux.read().snapshot().recently_closed;
    assert_eq!(view.len(), 2);
    assert!(view.iter().all(|entry| entry.group_kind == "workspace"));
    assert_eq!(view[0].group_id, view[1].group_id);
    assert!(view.iter().any(|entry| entry.tab.tab_id == first_tab));
    assert!(view.iter().any(|entry| entry.tab.tab_id == second_tab));
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn workspace_close_retires_task_bindings_for_member_tabs() {
    let (mut mux, owner) = MultiplexerState::new();
    let workspace_id = mux.find_workspace_for_tab(&owner).unwrap();
    let worker = mux.create_tab(&workspace_id).unwrap();
    let fixture = ServiceFixture::new(mux);
    let mut plan = Plan::new(
        "workspace close cleanup".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef::from_tab(&owner),
    );
    fixture.service.plan_store.save(&mut plan, 1).unwrap();
    let task_id = fixture
        .service
        .plan_store
        .add_task(
            &plan.id,
            NewTask {
                name: "bound worker".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    fixture
        .service
        .plan_store
        .assign_task(&plan.id, &task_id, &worker, None, 3)
        .unwrap();

    fixture.service.close_workspace(&workspace_id).unwrap();

    let task = fixture
        .service
        .plan_store
        .get(&plan.id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == task_id)
        .unwrap();
    assert_eq!(task.assigned_tab_id, None);
    let attempt = task
        .execution_attempts
        .iter()
        .find(|attempt| attempt.executor_tab_id == worker)
        .expect("worker attempt");
    assert!(attempt.closed_at.is_some());
    assert!(attempt.retired_at.is_some());
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn workspace_close_reservation_blocks_membership_before_lifecycle_cleanup() {
    let (mut mux, tab_id) = MultiplexerState::new();
    let workspace_id = mux.find_workspace_for_tab(&tab_id).unwrap();
    mux.stamp_lifecycle(
        &tab_id,
        crate::session::lifecycle::LifecycleEpoch(7),
        "operation-before-close",
    );
    let fixture = ServiceFixture::new(mux);
    let prepared = fixture
        .service
        .mux
        .write()
        .reserve_workspace_close(&workspace_id)
        .unwrap();

    let create_error = fixture.service.create_tab(&workspace_id).unwrap_err();
    let close_error = fixture.service.close_tab(&tab_id).unwrap_err();

    assert!(create_error.contains("Workspace is closing"));
    assert!(close_error.contains("Workspace is closing"));
    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&tab_id).unwrap();
    assert_eq!(tab.lifecycle_epoch, Some(7));
    assert_eq!(
        tab.lifecycle_operation_id.as_deref(),
        Some("operation-before-close")
    );
    drop(mux);
    fixture.service.mux.write().abort_workspace_close(&prepared);
    assert!(fixture.service.create_tab(&workspace_id).is_ok());
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn workspace_close_finishes_after_a_pre_teardown_error() {
    let (mut mux, first_tab) = MultiplexerState::new();
    let workspace_id = mux.find_workspace_for_tab(&first_tab).unwrap();
    let second_tab = mux.create_tab(&workspace_id).unwrap();
    let fixture = ServiceFixture::new(mux);
    let mut prepared_tabs = Vec::new();
    let mut finished_tabs = Vec::new();

    let error = fixture
        .service
        .close_workspace_with_teardown(
            &workspace_id,
            |_, tab_id| {
                prepared_tabs.push(tab_id.to_string());
                if tab_id == first_tab.as_str() {
                    Err("injected cleanup failure".to_string())
                } else {
                    Ok(())
                }
            },
            |_, tab_id| {
                finished_tabs.push(tab_id.to_string());
                Ok(())
            },
        )
        .unwrap_err();

    assert!(error.contains("workspace closed with cleanup errors"));
    assert_eq!(prepared_tabs.len(), 2);
    assert_eq!(finished_tabs.len(), 2);
    let mux = fixture.service.mux.read();
    assert!(mux.get_workspace(&workspace_id).is_none());
    assert!(mux.get_tab(&first_tab).is_none());
    assert!(mux.get_tab(&second_tab).is_none());
    let view = mux.snapshot().recently_closed;
    assert_eq!(view.len(), 2);
    assert_eq!(view[0].group_id, view[1].group_id);
}

#[tokio::test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
async fn close_team_via_service_persists_the_reduced_desk() {
    use crate::service::TeamMutationCaller;
    use crate::session::store::SessionStore;
    use tauri::Manager;

    let (mut mux, orchestrator) = MultiplexerState::new();
    let team_id = mux
        .resolve_or_mint_team_for_parent(&orchestrator)
        .unwrap()
        .team_id;
    let workspace_id = mux.find_workspace_for_tab(&orchestrator).unwrap();
    let worker = mux.create_tab(&workspace_id).unwrap();
    mux.set_tab_role(
        &worker,
        crate::multiplexer::types::AgentRole::Worker,
        Some(&orchestrator),
    )
    .unwrap();
    let bystander = mux.create_tab(&workspace_id).unwrap();

    let fixture = ServiceFixture::new(mux);
    let session: crate::session::SessionState = std::sync::Arc::new(parking_lot::Mutex::new(
        SessionStore::for_test_dir(fixture.temp.path().join("sessions")),
    ));
    fixture._app.manage(session.clone());

    crate::service::session_persist::write_session_snapshot(
        &fixture.service.app_handle,
        &fixture.service.mux,
    )
    .unwrap();
    let persisted_ids = |snapshot: &crate::session::store::SessionSnapshot| -> Vec<String> {
        snapshot
            .workspaces
            .iter()
            .flat_map(|ws| ws.tabs.iter().map(|tab| tab.id.clone()))
            .collect()
    };
    let before = session.lock().load_last().unwrap().expect("desk persisted");
    let before_ids = persisted_ids(&before);
    assert!(
        before_ids.contains(&orchestrator),
        "team desk on disk first"
    );
    assert!(before_ids.contains(&worker));

    // The REAL service boundary, not the close_team_with_state_change helper.
    fixture
        .service
        .close_team(&team_id, TeamMutationCaller::Human)
        .await
        .unwrap();

    let after = session
        .lock()
        .load_last()
        .unwrap()
        .expect("desk re-persisted");
    let after_ids = persisted_ids(&after);
    assert!(
        !after_ids.contains(&orchestrator) && !after_ids.contains(&worker),
        "closed Team members must not survive in last.json to resurrect on restart"
    );
    assert!(
        after_ids.contains(&bystander),
        "non-member tab stays persisted"
    );
    assert!(
        after.teams.iter().all(|team| team.id != team_id),
        "closed Team is gone from the snapshot"
    );
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn chatbackend_launch_failure_rolls_back_and_persists_the_reduced_desk() {
    use crate::service::spawn::{SpawnTabSpec, TabCommand};
    use crate::session::store::SessionStore;
    use tauri::Manager;

    let (mux, seed_tab) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let sessions_dir = fixture.temp.path().join("sessions");
    let session: crate::session::SessionState = std::sync::Arc::new(parking_lot::Mutex::new(
        SessionStore::for_test_dir(sessions_dir.clone()),
    ));
    fixture._app.manage(session.clone());

    crate::service::session_persist::write_session_snapshot(
        &fixture.service.app_handle,
        &fixture.service.mux,
    )
    .unwrap();
    // Only the rollback path's own save can bring last.json back; a rollback
    // that skips persistence leaves the file missing and fails the test.
    std::fs::remove_file(sessions_dir.join("last.json")).unwrap();
    let tabs_before = fixture.service.mux.read().snapshot().tabs.len();

    // Genuine failure injection through the REAL spawn path: the tab is
    // created first, then the unknown plugin fails the chatbackend launch.
    let spec = SpawnTabSpec {
        command: TabCommand::ChatBackend {
            plugin_id: "no-such-plugin".into(),
            config: serde_json::Value::Null,
            role: None,
            role_profile: None,
            can_spawn: None,
            watcher: None,
        },
        ..Default::default()
    };
    fixture
        .service
        .spawn_chatbackend_impl_with_state_change(spec, true)
        .expect_err("unknown plugin must fail the launch");

    assert_eq!(
        fixture.service.mux.read().snapshot().tabs.len(),
        tabs_before,
        "provisional tab rolled back from the live desk"
    );
    let persisted = std::fs::read_to_string(sessions_dir.join("last.json"))
        .expect("the rollback path must persist the authorized reduction");
    let snapshot: crate::session::store::SessionSnapshot =
        serde_json::from_str(&persisted).unwrap();
    let persisted_ids: Vec<String> = snapshot
        .workspaces
        .iter()
        .flat_map(|ws| ws.tabs.iter().map(|tab| tab.id.clone()))
        .collect();
    assert_eq!(
        persisted_ids,
        vec![seed_tab],
        "persisted desk holds exactly the pre-spawn tabs — no provisional tab, nothing lost"
    );
}
