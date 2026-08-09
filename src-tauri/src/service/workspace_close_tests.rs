use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{PendingPrompt, PromptOption};
use crate::plan::{OrchestratorRef, Plan};
use crate::service::plan::NewTask;
use crate::service::test_fixture::ServiceFixture;

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

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn unattributed_close_refuses_a_protected_workspace() {
    let (fixture, workspace_id, _) = protected_workspace();

    let error = fixture
        .service
        .close_workspace_guarded(None, &workspace_id)
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
fn unknown_local_tab_header_cannot_bypass_workspace_close_protection() {
    let (fixture, workspace_id, _) = protected_workspace();

    let error = fixture
        .service
        .close_workspace_guarded(Some("forged-tab"), &workspace_id)
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
fn coordinator_local_tab_cannot_close_its_own_workspace() {
    let (fixture, workspace_id, coordinator) = protected_workspace();

    let error = fixture
        .service
        .close_workspace_guarded(Some(&coordinator), &workspace_id)
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
fn authenticated_mesh_peer_may_close_a_protected_workspace() {
    let (fixture, workspace_id, _) = protected_workspace();

    fixture
        .service
        .close_workspace_from_authenticated_mesh_peer("paired-peer", &workspace_id)
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
        .close_workspace_from_authenticated_mesh_peer(" ", &workspace_id)
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

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn close_team_via_service_persists_the_reduced_desk() {
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
    assert!(before_ids.contains(&orchestrator), "team desk on disk first");
    assert!(before_ids.contains(&worker));

    // The REAL service boundary, not the close_team_with_state_change helper.
    fixture
        .service
        .close_team(&team_id, TeamMutationCaller::Human)
        .unwrap();

    let after = session.lock().load_last().unwrap().expect("desk re-persisted");
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
