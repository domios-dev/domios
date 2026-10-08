use super::*;
use crate::multiplexer::types::{AgentRole, CloseRestoreTabState, ShellPurpose, TaskBinding};
use codeterm_types::team::Team;

fn inventory(team_only: bool) -> (MultiplexerState, ClosedStructure, String) {
    let mut mux = MultiplexerState::empty();
    let workspace = mux
        .create_workspace("Project".into(), Some("/project".into()))
        .id;
    let mut team = Team::new(&workspace, 10);
    team.name = Some("Editors".into());
    mux.upsert_team(team.clone());
    for (profile, session) in [
        ("orchestrator", "own-orchestrator"),
        ("planner", "own-planner"),
        ("worker", "own-worker"),
    ] {
        let id = mux.create_tab(&workspace).unwrap();
        let tab = mux.get_tab_mut(&id).unwrap();
        tab.team_id = Some(team.id.clone());
        tab.agent_type = Some("fixture-provider".into());
        tab.agent_session_id = Some(session.into());
        tab.role = Some(if profile == "worker" {
            AgentRole::Worker
        } else {
            AgentRole::Manager
        });
        tab.role_profile = Some(profile.into());
        tab.title = profile.into();
        tab.description = Some(format!("description-{profile}"));
        tab.cwd = Some("/project".into());
        tab.restorable_launch.captured = true;
        tab.restorable_launch.model = Some("fixture-model".into());
    }
    let shell = mux.create_tab(&workspace).unwrap();
    let tab = mux.get_tab_mut(&shell).unwrap();
    tab.team_id = Some(team.id.clone());
    tab.title = "QA".into();
    tab.purpose = ShellPurpose::Qa;
    tab.icon = Some("flask".into());
    tab.description = Some("Review shell".into());
    tab.shell_profile = Some(crate::pty::shell_profiles::ShellProfile {
        id: "fixture-shell".into(),
        name: "Fixture".into(),
        executable: "fixture".into(),
        args: vec!["--login".into()],
        wsl_distro: None,
    });
    tab.close_restore = Some(CloseRestoreTabState {
        command: Some("delete-review-fixture".into()),
        ..Default::default()
    });
    let chat = mux.create_tab(&workspace).unwrap();
    let tab = mux.get_tab_mut(&chat).unwrap();
    tab.team_id = Some(team.id.clone());
    tab.backing = crate::multiplexer::types::TabBacking::ChatBackend {
        plugin_id: "chat-fixture".into(),
        session_id: "own-chat-session".into(),
    };
    let loose = mux.create_tab(&workspace).unwrap();
    let ids = mux.get_workspace(&workspace).unwrap().tabs.clone();
    mux.get_tab_mut(&ids[0])
        .unwrap()
        .pending_paired_planner_tab_id = Some(ids[1].clone());
    let mut record = mux
        .capture_structure(
            &workspace,
            team_only.then_some(team.id.as_str()),
            "inventory".into(),
            100,
        )
        .unwrap();
    record.resources = serde_json::json!({"plans": [{"owner": "own-orchestrator", "planner": "own-planner"}],
        "crons": [{"id": "cron-1", "enabled": false}], "blueprints": [{"id": "bp-1", "paused": true}]});
    (mux, record, loose)
}

#[test]
fn workspace_inventory_round_trip_preserves_structure_and_stops_shells() {
    let (mut mux, record, _) = inventory(false);
    mux.save_closed_structure(&record).unwrap();
    mux.close_workspace(&record.workspace.id).unwrap();
    mux.finish_structure_close(&record.id).unwrap();
    assert!(mux.recent_closed_view().is_empty());
    let saved = mux.closed_structure(&record.id).unwrap().unwrap();
    assert_eq!(saved.state, StructureState::Closed);
    assert_eq!(saved.resources, record.resources);
    let views = mux.snapshot().closed_structures;
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].member_count, record.tabs.len());
    assert_eq!(views[0].cron_count, 1);
    assert_eq!(views[0].blueprint_count, 1);
    mux.restore_structure(&saved).unwrap();
    assert_eq!(
        mux.get_workspace(&record.workspace.id).unwrap().tabs,
        record.workspace.tabs
    );
    assert_eq!(mux.list_teams(), record.teams);
    for archived in &record.tabs {
        let tab = mux.get_tab(&archived.tab.id).unwrap();
        assert_eq!(tab.agent_session_id, archived.tab.agent_session_id);
        assert_eq!(tab.backing, archived.tab.backing);
        assert_eq!(tab.title, archived.tab.title);
        assert_eq!(tab.description, archived.tab.description);
        assert_eq!(tab.role, archived.tab.role);
        assert_eq!(tab.role_profile, archived.tab.role_profile);
        assert_eq!(
            tab.pending_paired_planner_tab_id,
            archived.paired_planner_tab_id
        );
        assert_eq!(tab.team_id, archived.tab.team_id);
        assert_eq!(tab.restorable_launch, archived.launch);
        assert_eq!(tab.shell_profile, archived.tab.shell_profile);
        assert_eq!(tab.icon, archived.tab.icon);
        assert_eq!(tab.purpose, archived.tab.purpose);
        assert_eq!(
            tab.close_restore
                .as_ref()
                .and_then(|state| state.command.as_ref()),
            archived
                .tab
                .close_restore
                .as_ref()
                .and_then(|state| state.command.as_ref())
        );
        assert!(tab.pty_id.is_none());
        assert!(!tab.utility_command_pending);
        assert_eq!(
            tab.close_restore.as_ref().unwrap().stopped,
            archived.tab.is_utility_shell()
        );
    }
}

#[test]
fn team_inventory_excludes_loose_tabs_and_restores_original_order() {
    let (mut mux, record, loose) = inventory(true);
    assert!(!record.tabs.iter().any(|item| item.tab.id == loose));
    for item in &record.tabs {
        mux.close_tab_for_team_close(&item.tab.id).unwrap();
    }
    mux.remove_team(&record.teams[0].id).unwrap();
    assert!(mux.get_tab(&loose).is_some());
    mux.restore_structure(&record).unwrap();
    assert_eq!(
        mux.get_workspace(&record.workspace.id).unwrap().tabs,
        record.workspace.tabs
    );
    assert_eq!(mux.all_tabs().count(), record.tabs.len() + 1);
}

#[test]
fn structure_restore_is_idempotent_after_durable_store_and_desk_restart() {
    let dir = tempfile::tempdir().unwrap();
    let (mut mux, record, _) = inventory(false);
    mux.attach_closed_store(ClosedTabsStore::open(dir.path()).unwrap());
    mux.save_closed_structure(&record).unwrap();
    mux.close_workspace(&record.workspace.id).unwrap();
    mux.finish_structure_close(&record.id).unwrap();
    drop(mux);
    let mut reopened = MultiplexerState::empty();
    reopened.attach_closed_store(ClosedTabsStore::open(dir.path()).unwrap());
    let mut saved = reopened.closed_structure(&record.id).unwrap().unwrap();
    saved.state = StructureState::Restoring;
    reopened.save_closed_structure(&saved).unwrap();
    reopened.restore_structure(&saved).unwrap();
    let desk = reopened.to_session_snapshot();
    let desk_root = dir.path().join("desk");
    let storage =
        codeterm_storage::Storage::open(&desk_root, codeterm_storage::StorageConfig::default())
            .unwrap();
    let adapter = crate::session::store::StorageDeskAdapter::from_storage(storage.clone());
    adapter.import_snapshot(&desk, vec![]).unwrap();
    drop(adapter);
    drop(reopened);
    storage
        .shutdown()
        .wait(std::time::Duration::from_secs(5))
        .unwrap();
    drop(storage);
    let storage =
        codeterm_storage::Storage::open(&desk_root, codeterm_storage::StorageConfig::default())
            .unwrap();
    let adapter = crate::session::store::StorageDeskAdapter::from_storage(storage.clone());
    let desk = adapter.snapshot().unwrap();
    drop(adapter);
    storage
        .shutdown()
        .wait(std::time::Duration::from_secs(5))
        .unwrap();
    drop(storage);
    let mut restarted = MultiplexerState::from_session_snapshot(&desk);
    restarted.attach_closed_store(ClosedTabsStore::open(dir.path()).unwrap());
    let saved = restarted.closed_structure(&record.id).unwrap().unwrap();
    assert_eq!(saved.state, StructureState::Restoring);
    restarted.restore_structure(&saved).unwrap();
    assert_eq!(restarted.list_workspaces().len(), 1);
    assert_eq!(restarted.list_teams().len(), 1);
    assert_eq!(restarted.all_tabs().count(), record.tabs.len());
    for archived in &record.tabs {
        let tab = restarted.get_tab(&archived.tab.id).unwrap();
        assert_eq!(
            tab.pending_paired_planner_tab_id,
            archived.paired_planner_tab_id
        );
        assert_eq!(tab.agent_session_id, archived.tab.agent_session_id);
        assert_eq!(tab.role, archived.tab.role);
        assert_eq!(tab.role_profile, archived.tab.role_profile);
        assert_eq!(tab.team_id, archived.tab.team_id);
    }
    let shell = record
        .tabs
        .iter()
        .find(|item| item.tab.purpose == ShellPurpose::Qa)
        .unwrap();
    assert_eq!(
        restarted.get_tab(&shell.tab.id).unwrap().shell_profile,
        shell.tab.shell_profile
    );
    assert_eq!(
        restarted.get_workspace(&record.workspace.id).unwrap().tabs,
        record.workspace.tabs
    );
    assert!(restarted
        .all_tabs()
        .filter(|tab| tab.is_utility_shell())
        .all(|tab| tab.close_restore.as_ref().unwrap().stopped));
}

#[test]
fn structure_plan_never_guesses_a_missing_or_different_session() {
    let (mut mux, mut record, _) = inventory(false);
    record.tabs[0].tab.agent_session_id = None;
    assert!(record.restore_tabs().is_err());
    record.tabs[0].tab.agent_session_id = Some("own-orchestrator".into());
    mux.get_tab_mut(&record.tabs[0].tab.id)
        .unwrap()
        .agent_session_id = Some("someone-elses-latest".into());
    let before = mux.all_tabs().count();
    assert!(mux.restore_structure(&record).is_err());
    assert_eq!(mux.all_tabs().count(), before);
    assert_eq!(
        mux.get_tab(&record.tabs[0].tab.id)
            .unwrap()
            .agent_session_id
            .as_deref(),
        Some("someone-elses-latest")
    );
}

#[test]
fn structure_plan_detaches_finalized_worker_assignment_without_replaying_brief() {
    let (_, mut record, _) = inventory(true);
    let worker = &mut record.tabs[2];
    worker.tab.task_binding = Some(TaskBinding {
        plan_id: Some("plan".into()),
        task_id: Some("task".into()),
        attempt_id: Some("finalized".into()),
        ..Default::default()
    });
    worker.launch.identity_instructions = Some("old-assignment".into());
    worker.tab.cwd = Some("/canonical".into());
    worker.tab.close_restore = Some(CloseRestoreTabState {
        session_cwd: Some("/removed-lane".into()),
        ..Default::default()
    });
    let tabs = record.restore_tabs().unwrap();
    let worker = &tabs[2];
    assert_eq!(worker.agent_session_id.as_deref(), Some("own-worker"));
    assert!(worker.task_binding.is_none());
    assert!(worker.role.is_none());
    assert!(worker.worker_id.is_none());
    assert!(worker.restorable_launch.identity_instructions.is_none());
    assert_eq!(worker.cwd.as_deref(), Some("/canonical"));
    assert_eq!(
        worker
            .close_restore
            .as_ref()
            .unwrap()
            .session_cwd
            .as_deref(),
        Some("/removed-lane")
    );
}

#[test]
fn shell_rerun_claim_survives_restart_and_cannot_run_twice() {
    let mut state = CloseRestoreTabState {
        command: Some("one-shot".into()),
        stopped: true,
        ..Default::default()
    };
    assert_eq!(state.claim_rerun().unwrap().as_deref(), Some("one-shot"));
    let mut restarted: CloseRestoreTabState =
        serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap();
    assert!(restarted.claim_rerun().is_err());
}

#[test]
fn interrupted_restore_retry_does_not_restop_a_shell_already_started() {
    let (mut mux, record, _) = inventory(false);
    mux.close_workspace(&record.workspace.id).unwrap();
    mux.restore_structure(&record).unwrap();
    let shell = record
        .tabs
        .iter()
        .find(|item| item.tab.purpose == ShellPurpose::Qa)
        .unwrap();
    mux.get_tab_mut(&shell.tab.id)
        .unwrap()
        .close_restore
        .as_mut()
        .unwrap()
        .claim_rerun()
        .unwrap();
    mux.restore_structure(&record).unwrap();
    let state = mux
        .get_tab_mut(&shell.tab.id)
        .unwrap()
        .close_restore
        .as_mut()
        .unwrap();
    assert!(!state.stopped);
    assert!(state.claim_rerun().is_err());
}

#[test]
fn consumed_manager_pair_is_not_revived_by_desk_restart() {
    let (mut mux, record, _) = inventory(false);
    mux.close_workspace(&record.workspace.id).unwrap();
    mux.restore_structure(&record).unwrap();
    let orchestrator = &record.tabs[0].tab.id;
    assert!(mux
        .get_tab(orchestrator)
        .unwrap()
        .pending_paired_planner_tab_id
        .is_some());
    mux.get_tab_mut(orchestrator)
        .unwrap()
        .pending_paired_planner_tab_id = None;
    let restored = MultiplexerState::from_session_snapshot(&mux.to_session_snapshot());
    assert!(restored
        .get_tab(orchestrator)
        .unwrap()
        .pending_paired_planner_tab_id
        .is_none());
}

#[test]
fn team_inventory_keeps_members_moved_to_another_workspace() {
    let (mut mux, record, loose) = inventory(true);
    let other = mux
        .create_workspace("Other".into(), Some("/other".into()))
        .id;
    let moved = record.tabs[2].tab.id.clone();
    mux.move_tab(&moved, &record.workspace.id, &other).unwrap();
    let record = mux
        .capture_structure(
            &record.workspace.id,
            Some(&record.teams[0].id),
            "moved".into(),
            101,
        )
        .unwrap();
    assert_eq!(record.tabs.len(), 5);
    assert_eq!(record.workspaces.len(), 2);
    for item in &record.tabs {
        mux.close_tab_for_team_close(&item.tab.id).unwrap();
    }
    mux.restore_structure(&record).unwrap();
    assert!(mux.get_tab(&loose).is_some());
    assert_eq!(
        mux.find_workspace_for_tab(&moved).as_deref(),
        Some(other.as_str())
    );
}
