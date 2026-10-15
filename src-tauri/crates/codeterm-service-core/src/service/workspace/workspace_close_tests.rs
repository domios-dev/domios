use super::test_fixture::workspace_with_agent_tab;
use super::WorkspaceCloseCaller;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{PendingPrompt, PromptOption};
use crate::plan::{OrchestratorRef, Plan};
use crate::service::plan::NewTask;
use crate::service::tab::{normalize_tab_description, validate_tab_title, MAX_TAB_TITLE_CHARS};
use crate::service::test_fixture::ServiceFixture;
use crate::service::workspace::{validate_workspace_name, MAX_WORKSPACE_NAME_CHARS};

#[test]
fn names_and_titles_validated() {
    assert!(validate_workspace_name("Project Alpha").is_ok());
    for name in ["", "   ", "line\nbreak", "raw\u{1b}escape", "bell\u{7}"] {
        assert!(validate_workspace_name(name).is_err());
    }
    assert!(validate_workspace_name(&"w".repeat(MAX_WORKSPACE_NAME_CHARS)).is_ok());
    assert!(validate_workspace_name(&"w".repeat(MAX_WORKSPACE_NAME_CHARS + 1)).is_err());

    assert!(validate_tab_title("Worker42").is_ok());
    for title in ["", "two words", "raw\u{1b}escape", "bell\u{7}"] {
        assert!(validate_tab_title(title).is_err());
    }
    assert!(validate_tab_title(&"w".repeat(MAX_TAB_TITLE_CHARS)).is_ok());
    assert!(validate_tab_title(&"w".repeat(MAX_TAB_TITLE_CHARS + 1)).is_err());

    assert!(normalize_tab_description("single line").is_ok());
    assert!(normalize_tab_description("line one\nline two").is_err());
    assert!(normalize_tab_description("raw\u{1b}escape").is_err());
}

#[test]
fn move_tab_missing_workspace_names_target() {
    let (mux, tab_id) = MultiplexerState::new();
    let from_workspace_id = mux.find_workspace_for_tab(&tab_id).unwrap();
    let fixture = ServiceFixture::new(mux);
    let error = fixture
        .service
        .move_tab(&tab_id, &from_workspace_id, "missing-workspace")
        .unwrap_err();
    assert_eq!(error, "Workspace not found: missing-workspace");
}

#[test]
fn close_tab_keeps_focus_in_workspace() {
    let (mut mux, _) = MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let middle_tab = mux.create_tab(&workspace_id).unwrap();
    let adjacent_tab = mux.create_tab(&workspace_id).unwrap();
    mux.set_active_tab(&workspace_id, &middle_tab).unwrap();
    mux.set_focused_tab(&middle_tab).unwrap();

    mux.close_tab(&middle_tab).unwrap();

    let focused_tab = mux.get_focused_tab_id().unwrap();
    assert_eq!(focused_tab, adjacent_tab);
    assert_eq!(
        mux.find_workspace_for_tab(focused_tab).as_deref(),
        Some(workspace_id.as_str())
    );
}

#[test]
fn workspace_not_found_refusal_is_logged_with_known_ids() {
    let message = super::workspace_not_found_refusal_log(
        "requested-id",
        &["known-a".into(), "known-b".into()],
    );

    assert_eq!(
        message,
        "requested=requested-id known_ids=[\"known-a\", \"known-b\"]"
    );
}

#[test]
fn workspace_close_log_caller_distinguishes_confirmed_headerless_humans() {
    assert_eq!(
        WorkspaceCloseCaller::HumanConfirmedHeaderless.kind(),
        "human-attributed (headerless, confirmed)",
    );
    assert_eq!(WorkspaceCloseCaller::Unattributed.kind(), "Unattributed");
}

#[test]
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
        request_id: None,
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

#[tokio::test]
async fn workspace_close_handles_crons() {
    use crate::cron::store::{CronCreateParams, CronStore};
    use parking_lot::Mutex;
    use std::sync::Arc;
    use tauri::Manager;

    let (mut mux, _) = MultiplexerState::new();
    let workspace_id = mux.create_workspace("Cron workspace".into(), None).id;
    let retained_workspace_id = mux.create_workspace("Retained workspace".into(), None).id;
    let fixture = ServiceFixture::new(mux);
    let path = fixture.temp.path().join("crons.json");
    let cron = Arc::new(Mutex::new(CronStore::new_with_path(path.clone())));
    let add_cron = |workspace_id: String, name: &str| CronCreateParams {
        name: name.into(),
        workspace_id,
        team_id: None,
        detach_from_team: false,
        schedule: "* * * * *".into(),
        task: "run the job".into(),
        description: None,
        agent: Some("fixture".into()),
        model: None,
        reasoning_effort: None,
        cwd: None,
        role: None,
        skill: None,
        creator_tab_id: None,
        report_target_tab_id: None,
        enabled: Some(true),
        max_history: None,
        oneshot: None,
        jitter_max_ms: Some(0),
        tz: None,
        timeout_seconds: None,
    };
    let removed_job = cron
        .lock()
        .create(add_cron(workspace_id.clone(), "Removed cron"))
        .unwrap();
    let removed_log = cron
        .lock()
        .save_log(&removed_job.id, "run-1", "run output")
        .unwrap();
    cron.lock()
        .create(add_cron(retained_workspace_id.clone(), "Retained cron"))
        .unwrap();
    fixture._app.manage(cron.clone());
    fixture._app.manage(Arc::new(std::sync::Condvar::new()));

    fixture
        .service
        .close_workspace_as_human(&workspace_id, false)
        .await
        .unwrap();

    assert!(cron.lock().list_for_workspace(&workspace_id).is_empty());
    assert!(!removed_log.exists());
    assert_eq!(
        cron.lock().list_for_workspace(&retained_workspace_id).len(),
        1
    );
    let persisted = CronStore::new_with_path(path);
    assert!(persisted.list_for_workspace(&workspace_id).is_empty());
    assert_eq!(
        persisted.list_for_workspace(&retained_workspace_id).len(),
        1
    );
}

#[test]
fn unattributed_close_cannot_close_an_unprotected_workspace() {
    let (mut mux, _) = MultiplexerState::new();
    let workspace_id = mux.create_workspace("Unattributed".into(), None).id;
    mux.create_tab(&workspace_id).unwrap();
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
fn unattributed_caller_closes_an_empty_workspace() {
    let (mut mux, _) = MultiplexerState::new();
    let workspace_id = mux.create_workspace("Empty".into(), None).id;
    let fixture = ServiceFixture::new(mux);

    fixture
        .service
        .close_workspace_guarded(None, &workspace_id, false)
        .expect("an empty workspace has nothing a confirmation protects");

    assert!(fixture
        .service
        .mux
        .read()
        .get_workspace(&workspace_id)
        .is_none());
}

#[test]
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

#[test]
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
fn chatbackend_launch_failure_rolls_back_and_persists_the_reduced_desk() {
    use crate::service::spawn::{SpawnTabSpec, TabCommand};
    use crate::session::store::StorageDeskAdapter;

    let (mux, seed_tab) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let storage = crate::storage::Storage::open(
        fixture.temp.path().join("chatbackend-rollback-storage"),
        crate::storage::StorageConfig::default(),
    )
    .unwrap();
    let adapter = StorageDeskAdapter::from_storage(storage.clone());
    let mut stale_mux =
        MultiplexerState::from_session_snapshot(&fixture.service.mux.read().to_session_snapshot());
    let workspace_id = stale_mux.find_workspace_for_tab(&seed_tab).unwrap();
    let stale_tab = stale_mux.create_tab(&workspace_id).unwrap();
    adapter
        .import_snapshot(&stale_mux.to_session_snapshot(), Vec::new())
        .expect("seed a stale persisted tab for the rollback reduction");
    crate::service::session_persist::install_storage_desk_adapter(
        fixture._app.handle(),
        StorageDeskAdapter::from_storage(storage.clone()),
    )
    .unwrap();
    let before_revision = adapter.state().unwrap().revision;
    let tabs_before = fixture.service.mux.read().snapshot().tabs.len();

    // Genuine failure injection through the REAL spawn path: the tab is
    // created first, then the unknown plugin fails the chatbackend launch.
    let spec = SpawnTabSpec {
        shell: None,
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
    crate::service::session_persist::flush_session_persistence(fixture._app.handle()).unwrap();
    let after_state = adapter.state().unwrap();
    assert!(
        after_state.revision > before_revision,
        "rollback must commit its authorized desk reduction"
    );
    let snapshot = adapter.snapshot().unwrap();
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
    assert!(!persisted_ids.contains(&stale_tab));
    crate::service::session_persist::stop_worker(fixture._app.handle()).unwrap();
    storage
        .shutdown()
        .wait(std::time::Duration::from_secs(30))
        .unwrap();
}
