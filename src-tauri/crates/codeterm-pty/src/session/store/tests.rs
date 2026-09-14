use super::*;
use tempfile::TempDir;

fn fresh_store() -> (SessionStore, TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let store = SessionStore::for_test_dir(dir.path().to_path_buf());
    (store, dir)
}

fn snap(workspaces: usize) -> SessionSnapshot {
    SessionSnapshot {
        version: "test".into(),
        timestamp: 0,
        active_workspace_id: None,
        teams: vec![],
        workspaces: (0..workspaces)
            .map(|i| WorkspaceSnapshot {
                task_keys: Default::default(),
                id: format!("ws_{}", i),
                name: format!("ws{}", i),
                tabs: vec![],
                active_tab_id: None,
                cwd: None,
                provisioned_cwd: None,
                protected: false,
                sort_order: i as i32,
            })
            .collect(),
    }
}

#[test]
fn save_is_atomic_no_leftover_tmp() {
    let (store, _dir) = fresh_store();
    store.save(&snap(2)).unwrap();
    assert!(store.session_dir.join("last.json").exists());
    assert!(!store.session_dir.join("last.json.tmp").exists());
}

#[test]
fn load_corrupt_last_falls_back_to_backup() {
    let (store, _dir) = fresh_store();
    store.save(&snap(3)).unwrap();
    store.save(&snap(4)).unwrap();
    store.save(&snap(5)).unwrap();

    fs::write(store.session_dir.join("last.json"), b"{not valid json").unwrap();

    let recovered = store.load_last().unwrap().expect("must recover");
    assert!(
        !recovered.workspaces.is_empty(),
        "fallback to backups must return non-empty snapshot"
    );
}

#[test]
fn load_missing_last_falls_back_to_backup() {
    let (store, _dir) = fresh_store();
    store.save(&snap(2)).unwrap();
    store.save(&snap(3)).unwrap();
    fs::remove_file(store.session_dir.join("last.json")).unwrap();

    let recovered = store
        .load_last()
        .unwrap()
        .expect("must recover from backup");
    assert!(!recovered.workspaces.is_empty());
}

#[test]
fn load_no_last_no_backups_returns_none() {
    let (store, _dir) = fresh_store();
    let res = store.load_last().unwrap();
    assert!(res.is_none());
}

#[test]
fn load_corrupt_last_and_corrupt_backups_returns_none() {
    let (store, _dir) = fresh_store();
    store.save(&snap(1)).unwrap();
    store.save(&snap(1)).unwrap();
    fs::write(store.session_dir.join("last.json"), b"oops").unwrap();
    let backup_dir = store.session_dir.join("backups");
    for entry in fs::read_dir(&backup_dir).unwrap().flatten() {
        fs::write(entry.path(), b"corrupt").unwrap();
    }
    let res = store.load_last().unwrap();
    assert!(res.is_none(), "all corrupt → None, never panics");
}

#[test]
fn round_trip_save_load() {
    let (store, _dir) = fresh_store();
    let s = snap(3);
    store.save(&s).unwrap();
    let loaded = store.load_last().unwrap().unwrap();
    assert_eq!(loaded.workspaces.len(), 3);
}

#[test]
fn authoritative_session_binding_persists_orchestration_identity() {
    use crate::multiplexer::state::MultiplexerState;
    use crate::multiplexer::types::AgentRole;

    let (store, _dir) = fresh_store();
    let (mut state, parent) = MultiplexerState::new();
    state.set_orchestrator_role(&parent).unwrap();
    let parent_tab = state.get_tab_mut(&parent).unwrap();
    parent_tab.agent_type = Some("claude".into());
    parent_tab.agent_session_id = Some("session-orchestrator".into());
    parent_tab.session_binding_nonce = Some("binding-orchestrator".into());
    parent_tab.reported_session_binding_nonce = Some("binding-orchestrator".into());
    let workspace = state.find_workspace_for_tab(&parent).unwrap();
    let worker = state.create_tab(&workspace).unwrap();
    state
        .set_tab_role(&worker, AgentRole::Worker, Some(&parent))
        .unwrap();
    let tab = state.get_tab_mut(&worker).unwrap();
    tab.role_profile = Some("executor".into());
    tab.agent_type = Some("claude".into());
    tab.agent_session_id = Some("session-owned".into());
    tab.session_binding_nonce = Some("binding-owned".into());
    tab.reported_session_binding_nonce = Some("binding-owned".into());
    tab.reasoning_effort_identity = crate::multiplexer::types::ReasoningEffortIdentity {
        planned: Some("high".into()),
        requested: Some("high".into()),
        resolved: Some("high".into()),
        actual: None,
        actual_source: Some("provider-default-unverified".into()),
    };
    tab.archive_context_epoch("claude", "prior-session");
    let planner = state.create_tab(&workspace).unwrap();
    state
        .set_tab_role_kind(&planner, AgentRole::Manager, Some("planner"), Some(&parent))
        .unwrap();
    let planner_tab = state.get_tab_mut(&planner).unwrap();
    planner_tab.agent_type = Some("claude".into());
    planner_tab.agent_session_id = Some("session-planner".into());
    planner_tab.session_binding_nonce = Some("binding-planner".into());
    planner_tab.reported_session_binding_nonce = Some("binding-planner".into());

    let snapshot = state.to_session_snapshot();
    store.save(&snapshot).unwrap();
    assert!(!store
        .list_saved()
        .unwrap()
        .iter()
        .any(|name| name == "agent-session-identities"));
    let identity = store
        .load_agent_session_identity("claude", "session-owned")
        .expect("authoritative identity");
    assert_eq!(identity.role, "Worker");
    assert_eq!(identity.parent_tab_id.as_deref(), Some(parent.as_str()));
    assert!(!identity.can_spawn);
    assert_eq!(identity.role_profile.as_deref(), Some("executor"));
    let orchestrator = store
        .load_agent_session_identity("claude", "session-orchestrator")
        .expect("orchestrator identity");
    assert_eq!(orchestrator.role, "Manager");
    assert_eq!(orchestrator.role_profile.as_deref(), Some("orchestrator"));
    assert!(orchestrator.can_spawn);
    let planner_identity = store
        .load_agent_session_identity("claude", "session-planner")
        .expect("planner identity");
    assert_eq!(planner_identity.role, "Manager");
    assert_eq!(planner_identity.role_profile.as_deref(), Some("planner"));
    assert_eq!(
        planner_identity.parent_tab_id.as_deref(),
        Some(parent.as_str())
    );
    assert!(store
        .load_agent_session_identity("claude", "different-session")
        .is_none());
    let catalog_path = store.session_dir.join("agent-session-identities.json");
    let backup_path = store.session_dir.join("agent-session-identities.json.bak");
    std::fs::copy(&catalog_path, &backup_path).unwrap();
    std::fs::write(&catalog_path, "{").unwrap();
    assert_eq!(
        store
            .load_agent_session_identity("claude", "session-owned")
            .map(|identity| identity.role),
        Some("Worker".into())
    );
    let restored = MultiplexerState::from_session_snapshot(&snapshot);
    assert_eq!(
        restored
            .get_tab(&worker)
            .unwrap()
            .reasoning_effort_identity
            .actual_source
            .as_deref(),
        Some("provider-default-unverified")
    );
    assert_eq!(
        restored
            .get_tab(&worker)
            .unwrap()
            .context_epoch_archives
            .first()
            .map(|archive| archive.session_id.as_str()),
        Some("prior-session")
    );

    state.clear_tab_role(&worker).unwrap();
    store.save(&state.to_session_snapshot()).unwrap();
    assert!(store
        .load_agent_session_identity("claude", "session-owned")
        .is_none());
}

fn leaf(id: &str) -> SplitNodeSnapshot {
    SplitNodeSnapshot::Leaf {
        tab_id: id.into(),
        cwd: None,
        title: None,
        description: None,
        title_level: None,
        agent_type: None,
        agent_type_locked: false,
        claude_session_id: None,
        agent_identity: None,
        role: None,
        role_profile: None,
        team_id: None,
        parent_tab_id: None,
        spawned_by_tab_id: None,
        report_target_tab_id: None,
        report_target_mesh_peer: None,
        ownership: None,
        can_spawn: false,
        depth: 0,
        chat_view: None,
        auto_approve: false,
        audience: legacy_snapshot_audience(),
        agent_session_model: None,
        agent_planned_model: None,
        agent_requested_model: None,
        agent_provider_resolved_model: None,
        model_fallback: None,
        model_identity_mismatch: None,
        chatbackend_plugin_id: None,
        chatbackend_session_id: None,
        watcher: None,
        pending_delivery: None,
    }
}

fn desk(specs: &[(&str, &[&str])]) -> SessionSnapshot {
    SessionSnapshot {
        version: "test".into(),
        timestamp: 0,
        active_workspace_id: None,
        teams: vec![],
        workspaces: specs
            .iter()
            .map(|(ws_id, tab_ids)| WorkspaceSnapshot {
                task_keys: Default::default(),
                id: (*ws_id).into(),
                name: (*ws_id).into(),
                active_tab_id: None,
                cwd: None,
                provisioned_cwd: None,
                protected: false,
                sort_order: 0,
                tabs: tab_ids
                    .iter()
                    .map(|tab_id| TabSnapshot {
                        id: (*tab_id).into(),
                        title: (*tab_id).into(),
                        description: None,
                        sort_order: 0,
                        split_tree: leaf(tab_id),
                    })
                    .collect(),
            })
            .collect(),
    }
}

fn quarantine_files(store: &SessionStore) -> Vec<std::path::PathBuf> {
    fs::read_dir(&store.session_dir)
        .unwrap()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("last.json.reduced-")
        })
        .map(|e| e.path())
        .collect()
}

#[test]
fn reducing_save_is_refused_quarantined_and_last_json_stays_intact() {
    let (store, _dir) = fresh_store();
    store
        .save(&desk(&[("ws_a", &["t1", "t2"]), ("ws_b", &["t3"])]))
        .unwrap();

    let reduced = desk(&[("ws_a", &["t1"])]);
    let outcome = store
        .save(&reduced)
        .expect("a refusal is protection, never an Err");
    let SaveOutcome::RefusedReduction(refusal) = outcome else {
        panic!("reducing save must refuse, got {outcome:?}");
    };
    assert_eq!(refusal.dropped_workspaces, 1);
    assert_eq!(refusal.dropped_tabs, 2);

    let persisted = store.load_last().unwrap().expect("last.json intact");
    assert_eq!(
        persisted.workspaces.len(),
        2,
        "full desk still authoritative"
    );
    assert_eq!(
        persisted
            .workspaces
            .iter()
            .map(|ws| ws.tabs.len())
            .sum::<usize>(),
        3
    );

    let quarantined = quarantine_files(&store);
    assert_eq!(quarantined.len(), 1, "rejected snapshot kept for forensics");
    let content = fs::read_to_string(&quarantined[0]).unwrap();
    let parsed: SessionSnapshot = serde_json::from_str(&content).unwrap();
    assert_eq!(
        parsed.workspaces.len(),
        1,
        "quarantine holds the rejected desk"
    );
}

#[test]
fn authorized_reduction_intent_allows_reducing_save() {
    let (store, _dir) = fresh_store();
    store
        .save(&desk(&[("ws_a", &["t1", "t2"]), ("ws_b", &["t3"])]))
        .unwrap();

    let outcome = store
        .save_with_intent(&desk(&[("ws_a", &["t1"])]), SaveIntent::AuthorizedReduction)
        .expect("an authorized reduction may shrink the desk");
    assert!(matches!(outcome, SaveOutcome::Saved));
    let persisted = store.load_last().unwrap().unwrap();
    assert_eq!(persisted.workspaces.len(), 1);
    assert!(
        quarantine_files(&store).is_empty(),
        "no quarantine on intent"
    );
}

#[test]
fn tab_drop_without_workspace_drop_is_refused() {
    let (store, _dir) = fresh_store();
    store.save(&desk(&[("ws_a", &["t1", "t2"])])).unwrap();
    let outcome = store.save(&desk(&[("ws_a", &["t1"])])).unwrap();
    let SaveOutcome::RefusedReduction(refusal) = outcome else {
        panic!("tab drop must refuse, got {outcome:?}");
    };
    assert_eq!(refusal.dropped_workspaces, 0);
    assert_eq!(refusal.dropped_tabs, 1);
}

#[test]
fn tab_moved_across_workspaces_is_not_a_reduction() {
    let (store, _dir) = fresh_store();
    store
        .save(&desk(&[("ws_a", &["t1"]), ("ws_b", &["t2"])]))
        .unwrap();
    let outcome = store
        .save(&desk(&[("ws_a", &[]), ("ws_b", &["t2", "t1"])]))
        .expect("a move keeps every id alive");
    assert!(matches!(outcome, SaveOutcome::Saved));
}

#[test]
fn additive_save_is_allowed_without_intent() {
    let (store, _dir) = fresh_store();
    store.save(&desk(&[("ws_a", &["t1"])])).unwrap();
    let outcome = store
        .save(&desk(&[("ws_a", &["t1", "t2"]), ("ws_b", &["t3"])]))
        .expect("growth needs no intent");
    assert!(matches!(outcome, SaveOutcome::Saved));
}

#[test]
fn list_backups_reports_counts_newest_first_and_load_backup_round_trips() {
    let (store, _dir) = fresh_store();
    store.save(&desk(&[("ws_a", &["t1", "t2"])])).unwrap();
    // Second save rotates the first desk into backups/.
    store
        .save(&desk(&[("ws_a", &["t1", "t2"]), ("ws_b", &["t3"])]))
        .unwrap();

    let backups = store.list_backups().unwrap();
    assert_eq!(backups.len(), 1);
    assert_eq!(backups[0].workspaces, 1);
    assert_eq!(backups[0].tabs, 2);

    let snap = store.load_backup(&backups[0].name).unwrap();
    assert_eq!(snap.workspaces.len(), 1);
    assert!(
        store.load_backup("../escape").is_err(),
        "name is digits-only"
    );
    assert!(
        store.load_backup("999999999999").is_err(),
        "absent backup errs"
    );
}

#[test]
fn restore_normalizes_pane_era_leaves_and_isolates_malformed_tabs() {
    let (store, _dir) = fresh_store();
    let fixture = serde_json::json!({
        "version": "pane-era",
        "timestamp": 7,
        "active_workspace_id": "workspace",
        "teams": [],
        "workspaces": [{
            "id": "workspace",
            "name": "Recovered",
            "tabs": [
                {
                    "id": "tab-valid",
                    "title": "Valid",
                    "split_tree": {
                        "type": "Leaf",
                        "pane_id": "tab-valid",
                        "cwd": "D:/project"
                    },
                    "sort_order": 0
                },
                {
                    "id": "tab-malformed",
                    "title": "Malformed",
                    "split_tree": {
                        "type": "Leaf",
                        "cwd": "D:/project"
                    },
                    "sort_order": 1
                }
            ],
            "active_tab_id": "tab-valid",
            "cwd": "D:/project",
            "protected": false,
            "sort_order": 0
        }]
    })
    .to_string();
    let path = store.session_dir.join("last.json");
    fs::write(&path, &fixture).unwrap();

    let snapshot = store.load_last().unwrap().unwrap();
    assert_eq!(snapshot.workspaces.len(), 1);
    assert_eq!(snapshot.workspaces[0].tabs.len(), 1);
    assert_eq!(snapshot.workspaces[0].tabs[0].id, "tab-valid");
    match &snapshot.workspaces[0].tabs[0].split_tree {
        SplitNodeSnapshot::Leaf { tab_id, .. } => assert_eq!(tab_id, "tab-valid"),
        SplitNodeSnapshot::Split { .. } => panic!("expected recovered leaf"),
    }
    assert_eq!(fs::read_to_string(path).unwrap(), fixture);
}

#[test]
fn restore_keeps_a_full_fidelity_1_7_7_leaf() {
    // A real 1.7.7 leaf carries the whole agent identity, and a single field this build cannot deserialize quarantines the tab silently — parse "succeeds", session empty.
    let (store, _dir) = fresh_store();
    let fixture = r#"{
  "version": "1.7.7",
  "timestamp": 7,
  "active_workspace_id": "ws",
  "teams": [],
  "workspaces": [{
"id": "ws",
"name": "General",
"active_tab_id": "p1",
"cwd": "/tmp/project",
"protected": false,
"sort_order": 0,
"tabs": [{
  "id": "p1",
  "title": "Keats",
  "sort_order": 0,
  "split_tree": {
    "type": "Leaf",
    "pane_id": "p1",
    "cwd": "/tmp/project",
    "title": "Keats",
    "title_level": "Default",
    "agent_type": "claude",
    "agent_type_locked": true,
    "claude_session_id": "b0937780-2ad6-4ceb-a8dc-571a3c491d6b",
    "agent_identity": {
      "session_binding_nonce": "0244b07f-7746-4d22-9e73-9bc396c3af48",
      "reported_session_binding_nonce": "0244b07f-7746-4d22-9e73-9bc396c3af48",
      "agent_state": { "state": "idle", "last_turn_summary": "Baked for 9s" },
      "boot_stage": "agent_ready"
    },
    "role": "Manager",
    "role_profile": "orchestrator",
    "team_id": "team_ac70d9af",
    "parent_pane_id": "p0",
    "spawned_by_pane_id": "p0",
    "report_target_pane_id": "p0",
    "report_target_mesh_peer": "Mac-Air",
    "can_spawn": true,
    "depth": 1,
    "chat_view": true,
    "auto_approve": true,
    "agent_session_model": "opus",
    "agent_planned_model": "opus",
    "agent_requested_model": "opus",
    "agent_provider_resolved_model": "opus"
  }
}]
  }]
}"#
    .to_string();
    fs::write(store.session_dir.join("last.json"), &fixture).unwrap();

    let snapshot = store.load_last().unwrap().unwrap();
    assert_eq!(
        snapshot.workspaces.len(),
        1,
        "a 1.7.7 workspace must survive the upgrade",
    );
    let tabs = &snapshot.workspaces[0].tabs;
    assert_eq!(tabs.len(), 1, "the 1.7.7 tab must not be quarantined");
    match &tabs[0].split_tree {
        SplitNodeSnapshot::Leaf {
            tab_id,
            parent_tab_id,
            spawned_by_tab_id,
            report_target_tab_id,
            agent_type,
            claude_session_id,
            agent_identity,
            role,
            role_profile,
            team_id,
            can_spawn,
            depth,
            ..
        } => {
            assert_eq!(tab_id, "p1");
            assert_eq!(parent_tab_id.as_deref(), Some("p0"));
            assert_eq!(spawned_by_tab_id.as_deref(), Some("p0"));
            assert_eq!(report_target_tab_id.as_deref(), Some("p0"));
            assert_eq!(
                claude_session_id.as_deref(),
                Some("b0937780-2ad6-4ceb-a8dc-571a3c491d6b"),
                "resume identity must survive or the agent restarts blank",
            );
            // Pre-journal authority restores like everything else —
            // restore is unconditional, so every field comes back verbatim.
            assert_eq!(agent_type.as_deref(), Some("claude"));
            assert_eq!(role.as_deref(), Some("Manager"));
            assert_eq!(role_profile.as_deref(), Some("orchestrator"));
            assert_eq!(team_id.as_deref(), Some("team_ac70d9af"));
            assert!(*can_spawn);
            assert_eq!(*depth, 1);
            let identity = agent_identity.as_ref().expect("agent identity survives");
            assert_eq!(
                identity.session_binding_nonce.as_deref(),
                Some("0244b07f-7746-4d22-9e73-9bc396c3af48"),
            );
        }
        SplitNodeSnapshot::Split { .. } => panic!("expected a leaf"),
    }
}
