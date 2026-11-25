use super::*;

#[test]
fn oversized_leaf_does_not_block_desk_completion() {
    use crate::session::store::SplitNodeSnapshot;
    use codeterm_storage::{DeskLayoutNode, DeskLeafFields, MAX_DESK_LEAF_BYTES};

    let root = tempfile::tempdir().unwrap();
    let authority = crate::startup::open_instance_storage_at(root.path());
    let storage = authority.require_ready().expect("startup seeds the desk");
    let adapter = || crate::session::store::StorageDeskAdapter::from_storage(storage.clone());
    assert!(adapter().state().unwrap().snapshot.workspaces.is_empty());
    let worker = pending_worker();
    worker
        .configure(WorkerMessage::InstallStorage(adapter()))
        .unwrap();

    let mut snapshot = fixture_snapshot("main");
    let raw_fields = |leaf: &SplitNodeSnapshot| {
        let mut values = serde_json::to_value(leaf).unwrap();
        let fields = values.as_object_mut().unwrap();
        fields.remove("type");
        fields.remove("tab_id");
        DeskLeafFields { values }
    };
    let workspace = &mut snapshot.workspaces[0];
    for id in ["retained", "closing"] {
        workspace
            .tabs
            .push(fixture_snapshot(id).workspaces.remove(0).tabs.remove(0));
    }
    let main = &mut workspace.tabs[0];
    let SplitNodeSnapshot::Leaf {
        role,
        role_profile,
        agent_identity,
        ..
    } = &mut main.split_tree
    else {
        panic!("expected a Main leaf");
    };
    *role = Some("Manager".into());
    *role_profile = Some(codeterm_types::agent::role_kind::builtins::GENERAL_AGENT.into());
    agent_identity.as_mut().unwrap().agent_runtime_config =
        Some(serde_json::json!({"systemPrompt": "", "model": "fixture"}));
    let mut raw = raw_fields(&main.split_tree);
    let prompt_bytes = 61 * 1024;
    let baseline = serde_json::to_vec(&raw.values).unwrap().len();
    let escaped_bytes = MAX_DESK_LEAF_BYTES - baseline - prompt_bytes;
    let prompt = "x".repeat(prompt_bytes - escaped_bytes) + &"\n".repeat(escaped_bytes);
    raw.values["agent_identity"]["agent_runtime_config"]["systemPrompt"] =
        serde_json::json!(prompt);
    assert_eq!(
        serde_json::to_vec(&raw.values).unwrap().len(),
        MAX_DESK_LEAF_BYTES
    );
    assert_eq!(raw.validate(), Ok(()));
    let SplitNodeSnapshot::Leaf { agent_identity, .. } = &mut main.split_tree else {
        unreachable!();
    };
    agent_identity.as_mut().unwrap().agent_runtime_config =
        Some(raw.values["agent_identity"]["agent_runtime_config"].clone());
    raw.values["last_active_at_ms"] = serde_json::json!(main.last_active_at_ms);
    raw.values["last_opened_at_ms"] = serde_json::json!(main.last_opened_at_ms);
    assert_eq!(
        raw.validate(),
        Err(codeterm_storage::StorageError::Invalid {
            field: "desk leaf fields",
            reason: format!("exceeds {MAX_DESK_LEAF_BYTES} bytes"),
        })
    );

    let first_save = worker.submit_snapshot(snapshot.clone(), SaveIntent::PreserveDesk, true);
    assert!(
        matches!(first_save, Ok(Some(SaveOutcome::Saved))),
        "first desk completion: {first_save:?}"
    );
    let initial = adapter().state().unwrap();
    let mut newly_oversized = fixture_snapshot("new").workspaces.remove(0).tabs.remove(0);
    let workspace = &mut snapshot.workspaces[0];
    for tab in [&mut workspace.tabs[1], &mut newly_oversized] {
        let SplitNodeSnapshot::Leaf { close_restore, .. } = &mut tab.split_tree else {
            panic!("expected a leaf");
        };
        *close_restore = Some(crate::multiplexer::types::CloseRestoreTabState {
            command: Some(String::new()),
            ..Default::default()
        });
        let raw = raw_fields(&tab.split_tree);
        let command_bytes = MAX_DESK_LEAF_BYTES - serde_json::to_vec(&raw.values).unwrap().len();
        let SplitNodeSnapshot::Leaf { close_restore, .. } = &mut tab.split_tree else {
            unreachable!();
        };
        close_restore.as_mut().unwrap().command = Some("x".repeat(command_bytes));
    }
    workspace.tabs.push(newly_oversized);
    workspace.active_tab_id = Some("new".into());
    let preserving_save = worker.submit_snapshot(snapshot.clone(), SaveIntent::PreserveDesk, true);
    assert!(
        matches!(preserving_save, Ok(Some(SaveOutcome::Saved))),
        "desk completion with oversized tabs: {preserving_save:?}"
    );
    let preserved = adapter().state().unwrap();
    assert_eq!(
        preserved.snapshot.workspaces[0].tabs,
        initial.snapshot.workspaces[0].tabs
    );

    let workspace = &mut snapshot.workspaces[0];
    workspace.tabs.retain(|tab| tab.id != "closing");
    workspace.name = "Committed with omissions".into();
    let closing_save = worker.submit_snapshot(snapshot, SaveIntent::AuthorizedReduction, true);
    assert!(
        matches!(closing_save, Ok(Some(SaveOutcome::Saved))),
        "desk completion after closing a tab: {closing_save:?}"
    );
    stop_test_worker(&worker);

    let persisted = adapter().state().unwrap();
    let desk = &persisted.snapshot.workspaces[0];
    assert!(persisted.revision > initial.revision);
    assert_eq!(desk.record.name, "Committed with omissions");
    assert_eq!(desk.active_tab_id, None);
    assert_eq!(
        desk.tabs
            .iter()
            .map(|tab| tab.id.as_str())
            .collect::<Vec<_>>(),
        ["main", "retained"]
    );
    assert_eq!(desk.tabs[1], initial.snapshot.workspaces[0].tabs[1]);
    let DeskLayoutNode::Leaf { fields, .. } = &desk.tabs[0].layout else {
        panic!("expected persisted Main leaf");
    };
    assert_eq!(
        fields.values["agent_identity"]["agent_runtime_config"],
        serde_json::json!({"model": "fixture"})
    );
    assert_eq!(fields.values["agent_session_id"], "session-main");
    assert!(persisted.snapshot.validate().is_ok());
    storage.shutdown().wait(PERSISTENCE_DEADLINE).unwrap();
}

#[test]
fn repeated_preserving_save_with_omitted_tab_skips_redundant_commit() {
    use crate::session::store::SplitNodeSnapshot;
    use codeterm_storage::MAX_DESK_LEAF_BYTES;

    let root = tempfile::tempdir().unwrap();
    let authority = crate::startup::open_instance_storage_at(root.path());
    let storage = authority.require_ready().expect("startup seeds the desk");
    let adapter = || crate::session::store::StorageDeskAdapter::from_storage(storage.clone());
    let worker = pending_worker();
    worker
        .configure(WorkerMessage::InstallStorage(adapter()))
        .unwrap();

    let mut snapshot = fixture_snapshot("normal");
    let mut oversized = fixture_snapshot("oversized")
        .workspaces
        .remove(0)
        .tabs
        .remove(0);
    let SplitNodeSnapshot::Leaf { close_restore, .. } = &mut oversized.split_tree else {
        panic!("expected a leaf");
    };
    *close_restore = Some(crate::multiplexer::types::CloseRestoreTabState {
        command: Some("x".repeat(MAX_DESK_LEAF_BYTES + 1024)),
        ..Default::default()
    });
    snapshot.workspaces[0].tabs.push(oversized);

    assert!(matches!(
        worker.submit_snapshot(snapshot.clone(), SaveIntent::PreserveDesk, true),
        Ok(Some(SaveOutcome::Saved))
    ));
    let committed_revision = adapter().state().unwrap().revision;

    for _ in 0..3 {
        assert!(matches!(
            worker.submit_snapshot(snapshot.clone(), SaveIntent::PreserveDesk, true),
            Ok(Some(SaveOutcome::Saved))
        ));
    }
    assert_eq!(adapter().state().unwrap().revision, committed_revision);

    snapshot.workspaces[0].name = "Updated with omission".into();
    assert!(matches!(
        worker.submit_snapshot(snapshot, SaveIntent::PreserveDesk, true),
        Ok(Some(SaveOutcome::Saved))
    ));
    let updated = adapter().state().unwrap();
    assert!(updated.revision > committed_revision);
    assert_eq!(
        updated.snapshot.workspaces[0].record.name,
        "Updated with omission"
    );

    stop_test_worker(&worker);
    storage.shutdown().wait(PERSISTENCE_DEADLINE).unwrap();
}

#[test]
fn storage_desk_app_adapter_imports_and_flushes_exact_selection() {
    let root = tempfile::tempdir().unwrap();
    let adapter = test_adapter(root.path());
    let snapshot = crate::session::store::SessionSnapshot {
        version: "test".into(),
        timestamp: 17,
        active_workspace_id: Some("ws-a".into()),
        teams: vec![],
        workspaces: vec![crate::session::store::WorkspaceSnapshot {
            created_at_ms: 0,
            id: "ws-a".into(),
            name: "Alpha".into(),
            tabs: vec![],
            active_tab_id: None,
            cwd: None,
            provisioned_cwd: None,
            protected: true,
            sort_order: 8,
            task_keys: Default::default(),
        }],
    };

    adapter.import_snapshot(&snapshot, Vec::new()).unwrap();
    let initial = adapter.state().unwrap();
    let workspace_revision = initial.workspace_revisions["ws-a"];
    let committed_revision = adapter
        .rename_workspace("ws-a".into(), workspace_revision, "Renamed".into())
        .unwrap();
    assert!(committed_revision > initial.revision);
    assert!(adapter
        .rename_workspace("ws-a".into(), workspace_revision, "Stale".into())
        .is_err());
    let mut backup = snapshot.clone();
    backup.timestamp = 99;
    backup.active_workspace_id = Some("ws-b".into());
    backup.workspaces[0].name = "Stale backup".into();
    backup
        .workspaces
        .push(crate::session::store::WorkspaceSnapshot {
            created_at_ms: 0,
            id: "ws-b".into(),
            name: "Restored".into(),
            tabs: vec![],
            active_tab_id: None,
            cwd: None,
            provisioned_cwd: None,
            protected: false,
            sort_order: 19,
            task_keys: Default::default(),
        });
    adapter.additive_restore(&backup, Vec::new()).unwrap();
    adapter.additive_restore(&backup, Vec::new()).unwrap();
    adapter.flush().unwrap();
    let restored = adapter.snapshot().unwrap();

    assert_eq!(restored.active_workspace_id, Some("ws-a".into()));
    assert_eq!(restored.workspaces[0].active_tab_id, None);
    assert_eq!(restored.workspaces[0].name, "Renamed");
    assert_eq!(restored.workspaces[0].cwd, None);
    assert_eq!(restored.workspaces[0].provisioned_cwd, None);
    assert_eq!(restored.workspaces[0].sort_order, 8);
    assert!(restored.workspaces[0].protected);
    assert_eq!(restored.timestamp, 17);
    assert_eq!(restored.workspaces.len(), 2);
    assert_eq!(restored.workspaces[1].name, "Restored");
}

#[test]
fn storage_desk_restores_live_provider_sessions_for_each_provider() {
    let root = tempfile::tempdir().unwrap();
    let adapter = test_adapter(root.path());
    let (mut mux, parent_id) = crate::multiplexer::state::MultiplexerState::new();
    let workspace_id = mux.find_workspace_for_tab(&parent_id).unwrap();
    let providers = ["claude", "codex", "opencode", "grok"];
    let mut expected = Vec::new();

    for provider in providers {
        let tab_id = mux.create_tab(&workspace_id).unwrap();
        mux.set_tab_role(
            &tab_id,
            crate::multiplexer::types::AgentRole::Worker,
            Some(&parent_id),
        )
        .unwrap();
        let session_id = format!("{provider}-session");
        let nonce = format!("{provider}-nonce");
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some(provider.into());
        tab.agent_type_locked = true;
        tab.agent_session_id = Some(session_id.clone());
        tab.session_binding_nonce = Some(nonce.clone());
        tab.reported_session_binding_nonce = Some(nonce);
        expected.push((tab_id, provider, session_id));
    }

    let snapshot = mux.to_session_snapshot();
    let identities = desk_agent_session_identities(&snapshot);
    adapter.import_snapshot(&snapshot, identities).unwrap();
    adapter.flush().unwrap();
    assert_eq!(adapter.identity_catalog().unwrap().len(), providers.len());
    let restored_snapshot = adapter.snapshot().unwrap();
    let restored =
        crate::multiplexer::state::MultiplexerState::from_session_snapshot(&restored_snapshot);

    for (tab_id, provider, session_id) in expected {
        let tab = restored.get_tab(&tab_id).expect("restored agent tab");
        assert_eq!(tab.agent_type.as_deref(), Some(provider));
        assert_eq!(tab.agent_session_id.as_deref(), Some(session_id.as_str()));
        assert!(tab.agent_type_locked);
    }
}

#[test]
fn desk_delta_rename_touches_three_rows_in_a_large_inventory() {
    let root = tempfile::tempdir().unwrap();
    let adapter = test_adapter(root.path());
    let mut snapshot = desk_delta_snapshot(512);
    adapter.import_snapshot(&snapshot, Vec::new()).unwrap();
    let initial = adapter.state().unwrap();
    snapshot.workspaces[255].name = "Renamed".into();

    let committed = adapter
        .delta_snapshot(
            &snapshot,
            Vec::new(),
            initial.revision,
            crate::session::store::SaveIntent::PreserveDesk,
        )
        .unwrap()
        .wait(std::time::Duration::from_secs(30))
        .unwrap();

    assert_eq!(committed.receipt.rows_written, 3);
    let saved = adapter.state().unwrap();
    assert_eq!(saved.snapshot.workspaces.len(), 512);
    assert_eq!(saved.snapshot.workspaces[255].record.name, "Renamed");
}

#[test]
fn desk_delta_adjacent_reorder_normalizes_the_full_large_inventory() {
    let root = tempfile::tempdir().unwrap();
    let adapter = test_adapter(root.path());
    let mut snapshot = desk_delta_snapshot(512);
    adapter.import_snapshot(&snapshot, Vec::new()).unwrap();
    let initial = adapter.state().unwrap();
    snapshot.workspaces.swap(255, 256);
    snapshot.workspaces[255].sort_order = 255;
    snapshot.workspaces[256].sort_order = 256;

    let committed = adapter
        .delta_snapshot(
            &snapshot,
            Vec::new(),
            initial.revision,
            crate::session::store::SaveIntent::PreserveDesk,
        )
        .unwrap()
        .wait(std::time::Duration::from_secs(30))
        .unwrap();

    assert!(committed.receipt.rows_written >= 1024);
    let saved = adapter.state().unwrap();
    assert_eq!(saved.snapshot.workspaces[255].record.id.as_str(), "ws-256");
    assert_eq!(saved.snapshot.workspaces[256].record.id.as_str(), "ws-255");
}

#[test]
fn desk_delta_preserves_omissions_and_rejects_stale_reduction() {
    let root = tempfile::tempdir().unwrap();
    let adapter = test_adapter(root.path());
    let mut snapshot = desk_delta_snapshot(2);
    adapter.import_snapshot(&snapshot, Vec::new()).unwrap();
    let initial = adapter.state().unwrap();
    snapshot.workspaces.pop();

    adapter
        .delta_snapshot(
            &snapshot,
            Vec::new(),
            initial.revision,
            crate::session::store::SaveIntent::PreserveDesk,
        )
        .unwrap()
        .wait(std::time::Duration::from_secs(30))
        .unwrap();
    assert_eq!(adapter.state().unwrap().snapshot.workspaces.len(), 2);

    let mut newer = desk_delta_snapshot(2);
    newer.workspaces[0].name = "Current".into();
    adapter
        .delta_snapshot(
            &newer,
            Vec::new(),
            initial.revision,
            crate::session::store::SaveIntent::PreserveDesk,
        )
        .unwrap()
        .wait(std::time::Duration::from_secs(30))
        .unwrap();
    let stale = adapter
        .delta_snapshot(
            &snapshot,
            Vec::new(),
            initial.revision,
            crate::session::store::SaveIntent::PreserveDesk,
        )
        .unwrap()
        .wait(std::time::Duration::from_secs(30));

    assert!(stale.is_err());
    let saved = adapter.state().unwrap();
    assert_eq!(saved.snapshot.workspaces.len(), 2);
    assert_eq!(saved.snapshot.workspaces[0].record.name, "Current");
}

#[test]
fn desk_delta_authorized_reduction_applies_a_workspace_close() {
    let root = tempfile::tempdir().unwrap();
    let adapter = test_adapter(root.path());
    let mut snapshot = desk_delta_snapshot(2);
    adapter.import_snapshot(&snapshot, Vec::new()).unwrap();
    let initial = adapter.state().unwrap();
    snapshot.workspaces.pop();

    adapter
        .delta_snapshot(
            &snapshot,
            Vec::new(),
            initial.revision,
            crate::session::store::SaveIntent::AuthorizedReduction,
        )
        .unwrap()
        .wait(std::time::Duration::from_secs(30))
        .unwrap();

    let saved = adapter.state().unwrap();
    assert_eq!(saved.snapshot.workspaces.len(), 1);
    assert_eq!(saved.snapshot.workspaces[0].record.id.as_str(), "ws-0");
}
