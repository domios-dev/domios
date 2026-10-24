use super::*;
use crate::multiplexer::state::MultiplexerState;

#[test]
fn first_run_completion_publishes_saved_choice_before_launch() {
    let ui = crate::service::test_fixture::RecordedUiEvents::default();
    let mut config = crate::config::schema::AppConfig::default();
    config.general_agent.first_run_complete = true;
    config.general_agent.agent_provider = "fixture-provider".into();
    let mut saved = None;
    persist_first_run_completion(&config, &ui, |snapshot| {
        assert!(ui.0.lock().is_empty());
        saved = Some(snapshot.clone());
        Ok(())
    })
    .unwrap();
    let events = ui.0.lock();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, "config-changed");
    assert_eq!(events[0].1, serde_json::to_value(saved.unwrap()).unwrap());
    assert_eq!(events[0].1["general_agent"]["first_run_complete"], true);
}

#[test]
fn first_run_persistence_failure_never_announces_completion() {
    let ui = crate::service::test_fixture::RecordedUiEvents::default();
    let config = crate::config::schema::AppConfig::default();
    let error = persist_first_run_completion(&config, &ui, |_| Err("opaque save refusal".into()))
        .unwrap_err();
    assert_eq!(error, "opaque save refusal");
    assert!(ui.0.lock().is_empty());
}

#[test]
fn needs_picker_when_enabled_and_incomplete() {
    assert!(needs_first_run_picker(true, false));
    assert!(!needs_first_run_picker(true, true));
    assert!(!needs_first_run_picker(false, false));
}

#[test]
fn ensure_first_run_tab_creates_marker_tab() {
    let mut mux = MultiplexerState::empty();
    ensure_first_run_tab(&mut mux);
    assert!(has_first_run_tab(&mux));
    assert!(mux.resolve_tab_id(GENERAL_ALIAS).is_none());
}

#[test]
fn ensure_first_run_tab_is_idempotent() {
    let mut mux = MultiplexerState::empty();
    ensure_first_run_tab(&mut mux);
    let count = mux.list_workspaces()[0].tab_ids.len();
    ensure_first_run_tab(&mut mux);
    assert_eq!(mux.list_workspaces()[0].tab_ids.len(), count);
}

fn first_run_picker_tab_id(mux: &MultiplexerState) -> String {
    mux.list_workspaces()
        .iter()
        .flat_map(|ws| ws.tab_ids.iter().cloned())
        .find(|pid| {
            mux.get_tab(pid).and_then(|p| p.description.as_deref()) == Some(FIRST_RUN_TAB_MARKER)
        })
        .expect("first-run picker Tab exists")
}

#[test]
fn first_run_sole_picker_workspace_and_tab_are_protected_against_empty_close() {
    // P1: on a brand-new instance the picker is the ONLY content — closing it
    // must never leave an empty CodeTerm.
    let mut mux = MultiplexerState::empty();
    ensure_first_run_tab(&mut mux);

    let ws = mux.list_workspaces()[0].clone();
    assert!(ws.protected, "sole first-run workspace must be protected");
    let tab_id = first_run_picker_tab_id(&mux);
    assert!(
        mux.get_tab(&tab_id).unwrap().protected,
        "sole first-run picker Tab must be protected"
    );
    assert!(
        mux.close_workspace(&ws.id).is_err(),
        "closing the sole first-run workspace must be refused"
    );
    assert!(
        mux.close_tab(&tab_id).is_err(),
        "closing the sole first-run picker Tab must be refused"
    );
    // Invariant holds: still a workspace with a valid tab.
    assert_eq!(mux.list_workspaces().len(), 1);
    assert!(!mux.list_workspaces()[0].tab_ids.is_empty());
}

#[test]
fn first_run_picker_not_protected_when_other_workspaces_exist() {
    // Closing the picker here still leaves the pre-existing workspace, so no
    // protection is needed — and over-protecting a user workspace is wrong.
    let (mut mux, _) = MultiplexerState::new();
    ensure_first_run_tab(&mut mux);
    let tab_id = first_run_picker_tab_id(&mux);
    assert!(
        !mux.get_tab(&tab_id).unwrap().protected,
        "picker sharing a non-protected workspace must stay closable"
    );
}

#[test]
fn close_first_run_tabs_retires_the_protected_picker() {
    // Completion legitimately closes the (protected) picker before General is
    // materialized — the guard must be lifted, not block the transition.
    let mut mux = MultiplexerState::empty();
    ensure_first_run_tab(&mut mux);
    assert!(has_first_run_tab(&mux));
    close_first_run_tabs(&mut mux).expect("retire first-run tabs");
    assert!(!has_first_run_tab(&mux), "picker Tab removed on completion");
}

#[test]
fn first_run_picker_tab_has_skip_snapshot_set() {
    // The picker must not survive a daemon restart as a ghost tab.
    let mut mux = MultiplexerState::empty();
    ensure_first_run_tab(&mut mux);
    let tab_id = mux.list_workspaces()[0].tab_ids[0].clone();
    let tab = mux.get_tab(&tab_id).expect("picker Tab exists");
    assert!(
        tab.skip_snapshot,
        "first-run picker Tab must have skip_snapshot=true to be excluded from the session snapshot"
    );
}

#[test]
fn purge_snapshot_ghosts_drops_picker_tab_and_records_to_closed() {
    // purge_snapshot_ghosts must drop it and record it in Recently Closed.
    let mut mux = MultiplexerState::empty();
    ensure_first_run_tab(&mut mux);
    let ws_id = mux.list_workspaces()[0].id.clone();
    // Tab is present in live state (just not in the snapshot after the fix).
    assert!(!mux.get_workspace(&ws_id).unwrap().tabs.is_empty());

    let dropped = purge_snapshot_ghosts(&mut mux);
    assert_eq!(dropped, 1);

    // Tab removed from workspace.
    assert_eq!(mux.get_workspace(&ws_id).unwrap().tabs.len(), 0);
    // Tab removed from map.
    assert!(!mux.list_workspaces()[0].tab_ids.iter().any(|pid| {
        mux.get_tab(pid).and_then(|p| p.description.as_deref()) == Some(FIRST_RUN_TAB_MARKER)
    }));
    // Appears in Recently Closed.
    assert!(
        !mux.list_recently_closed(&ws_id).is_empty(),
        "purged ghost must appear in Recently Closed"
    );
}

#[test]
fn purge_snapshot_ghosts_preserves_real_tabs() {
    // A real tab (no ephemeral marker) must survive purge_snapshot_ghosts.
    let (mut mux, _) = MultiplexerState::new();
    let ws_id = mux.list_workspaces()[0].id.clone();
    let tab_id = mux.create_tab(&ws_id).unwrap();
    mux.get_tab_mut(&tab_id).unwrap().title = "Fixture".to_string();

    let dropped = purge_snapshot_ghosts(&mut mux);
    assert_eq!(dropped, 0);
    assert!(
        mux.get_tab(&tab_id).is_some(),
        "real Tab must survive purge"
    );
}

// ── Tests moved from multiplexer::state::snapshot::tests ─────────────

#[test]
fn ghost_repro_restore_from_old_snapshot_drops_picker_tab() {
    use crate::session::store::{
        SessionSnapshot, SplitNodeSnapshot, TabSnapshot, WorkspaceSnapshot,
    };

    let old_snapshot = SessionSnapshot {
        version: "test".into(),
        timestamp: 0,
        active_workspace_id: Some("ws".into()),
        teams: vec![],
        workspaces: vec![WorkspaceSnapshot {
            created_at_ms: 0,
            task_keys: Default::default(),
            id: "ws".into(),
            name: "Welcome".into(),
            active_tab_id: Some("tab-picker".into()),
            cwd: None,
            provisioned_cwd: None,
            protected: false,
            sort_order: 0,
            tabs: vec![TabSnapshot {
                last_active_at_ms: None,
                last_opened_at_ms: None,
                id: "tab-picker".into(),
                title: "Get started".into(),
                description: Some(FIRST_RUN_TAB_MARKER.into()),
                split_tree: SplitNodeSnapshot::Leaf {
                    created_at_ms: 0,
                    purpose: Default::default(),
                    icon: None,
                    shell_profile: None,
                    utility_created_at: 0,
                    close_restore: None,
                    tab_id: "tab-picker".into(),
                    cwd: None,
                    title: Some("Get started".into()),
                    description: Some(FIRST_RUN_TAB_MARKER.into()),
                    title_level: Some("User".into()),
                    agent_type: None,
                    agent_type_locked: false,
                    pending_delivery: None,
                    agent_session_id: None,
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
                    chat_view: Some(false),
                    auto_approve: false,
                    permission_auto_confirm: false,
                    audience: crate::multiplexer::types::TabAudience::User,
                    agent_session_model: None,
                    agent_planned_model: None,
                    agent_requested_model: None,
                    agent_provider_resolved_model: None,
                    model_fallback: None,
                    model_identity_mismatch: None,
                    chatbackend_plugin_id: None,
                    chatbackend_session_id: None,
                    surface: None,
                    watcher: None,
                },
                sort_order: 0,
            }],
        }],
    };

    let mut restored = MultiplexerState::from_session_snapshot(&old_snapshot);
    assert!(
        restored.get_tab("tab-picker").is_some(),
        "pre-purge: picker Tab must be present in restored state"
    );
    assert_eq!(
        restored.get_workspace("ws").map(|w| w.tabs.len()),
        Some(1),
        "pre-purge: workspace must have the ghost tab"
    );

    let dropped = purge_snapshot_ghosts(&mut restored);
    assert_eq!(dropped, 1, "purge must drop exactly one ghost tab");

    assert!(
        restored.get_tab("tab-picker").is_none(),
        "post-purge: picker Tab must be absent"
    );
    assert_eq!(
        restored.get_workspace("ws").map(|w| w.tabs.len()),
        Some(0),
        "post-purge: workspace must have no tabs"
    );
    let closed = restored.list_recently_closed("ws");
    assert!(
        !closed.is_empty(),
        "dropped ghost tab must appear in Recently Closed"
    );
}

#[test]
fn ghost_repro_first_run_picker_excluded_from_snapshot() {
    let mut state = MultiplexerState::empty();
    ensure_first_run_tab(&mut state);

    assert!(!state.list_workspaces().is_empty());
    assert!(!state.list_workspaces()[0].tab_ids.is_empty());

    let snap = state.to_session_snapshot();
    assert_eq!(
        snap.workspaces[0].tabs.len(),
        0,
        "first-run picker Tab must be excluded from the session snapshot"
    );
}
