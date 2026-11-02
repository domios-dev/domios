use super::*;

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
