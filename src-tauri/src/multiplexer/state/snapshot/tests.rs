use super::*;
use crate::session::store::{SessionSnapshot, SplitNodeSnapshot, TabSnapshot, WorkspaceSnapshot};

fn chatbackend_leaf(pane_id: &str, plugin_id: &str, session_id: &str) -> SplitNodeSnapshot {
    SplitNodeSnapshot::Leaf {
        pane_id: pane_id.into(),
        cwd: None,
        title: Some("LM Studio".into()),
        description: None,
        title_level: Some("User".into()),
        agent_type: None,
        agent_type_locked: false,
        claude_session_id: None,
        role: None,
        role_profile: None,
        parent_pane_id: None,
        spawned_by_pane_id: None,
        report_target_pane_id: None,
        report_target_mesh_peer: None,
        can_spawn: false,
        depth: 0,
        chat_view: Some(true),
        auto_approve: false,
        agent_session_model: None,
        chatbackend_plugin_id: Some(plugin_id.into()),
        chatbackend_session_id: Some(session_id.into()),
    }
}

fn snapshot_with(leaf: SplitNodeSnapshot) -> SessionSnapshot {
    SessionSnapshot {
        version: "test".into(),
        timestamp: 0,
        active_workspace_id: Some("ws".into()),
        workspaces: vec![WorkspaceSnapshot {
            id: "ws".into(),
            name: "ws".into(),
            active_tab_id: Some("tab".into()),
            cwd: None,
            protected: false,
            sort_order: 0,
            tabs: vec![TabSnapshot {
                id: "tab".into(),
                title: "tab".into(),
                description: None,
                split_tree: leaf,
                sort_order: 0,
            }],
        }],
    }
}

#[test]
fn chatbackend_backing_restores_from_snapshot() {
    let snap = snapshot_with(chatbackend_leaf("p1", "lmstudio", "sess-7"));
    let state = MultiplexerState::from_session_snapshot(&snap);
    let pane = state.panes.get("p1").expect("pane restored");
    assert_eq!(
        pane.backing,
        PaneBacking::ChatBackend {
            plugin_id: "lmstudio".into(),
            session_id: "sess-7".into(),
        },
        "restored as chatBackend, not Pty"
    );
    // never mounts a terminal — the "comes back as a terminal" guard.
    assert!(pane.is_chat_backend(), "renders chat, not terminal");
    assert!(pane.restored_from_snapshot, "marked for lazy re-open");
}

#[test]
fn chatbackend_backing_round_trips_through_snapshot() {
    // snapshot → state → snapshot must preserve the chatBackend backing.
    let snap = snapshot_with(chatbackend_leaf("p1", "lmstudio", "sess-7"));
    let state = MultiplexerState::from_session_snapshot(&snap);
    let round = state.to_session_snapshot();

    let leaf = &round.workspaces[0].tabs[0].split_tree;
    match leaf {
        SplitNodeSnapshot::Leaf {
            chatbackend_plugin_id,
            chatbackend_session_id,
            ..
        } => {
            assert_eq!(chatbackend_plugin_id.as_deref(), Some("lmstudio"));
            assert_eq!(chatbackend_session_id.as_deref(), Some("sess-7"));
        }
        _ => panic!("expected a leaf"),
    }
}

#[test]
fn pty_pane_round_trips_without_chatbackend_fields() {
    // backing on round-trip.
    let leaf = SplitNodeSnapshot::Leaf {
        pane_id: "p2".into(),
        cwd: None,
        title: Some("claude".into()),
        description: None,
        title_level: Some("User".into()),
        agent_type: Some("claude".into()),
        agent_type_locked: true,
        claude_session_id: Some("c-1".into()),
        role: None,
        role_profile: None,
        parent_pane_id: None,
        spawned_by_pane_id: None,
        report_target_pane_id: None,
        report_target_mesh_peer: None,
        can_spawn: false,
        depth: 0,
        chat_view: Some(false),
        auto_approve: false,
        agent_session_model: None,
        chatbackend_plugin_id: None,
        chatbackend_session_id: None,
    };
    let state = MultiplexerState::from_session_snapshot(&snapshot_with(leaf));
    let pane = state.panes.get("p2").expect("pane restored");
    assert_eq!(pane.backing, PaneBacking::Pty);
}

#[test]
fn pane_description_round_trips_through_snapshot() {
    let leaf = SplitNodeSnapshot::Leaf {
        pane_id: "p-desc".into(),
        cwd: None,
        title: Some("Worker".into()),
        description: Some("running focused tests".into()),
        title_level: Some("User".into()),
        agent_type: Some("claude".into()),
        agent_type_locked: true,
        claude_session_id: None,
        role: Some("Worker".into()),
        role_profile: Some("planner".into()),
        parent_pane_id: None,
        spawned_by_pane_id: None,
        report_target_pane_id: None,
        report_target_mesh_peer: None,
        can_spawn: false,
        depth: 2,
        chat_view: Some(false),
        auto_approve: false,
        agent_session_model: None,
        chatbackend_plugin_id: None,
        chatbackend_session_id: None,
    };
    let state = MultiplexerState::from_session_snapshot(&snapshot_with(leaf));
    let pane = state.panes.get("p-desc").expect("pane restored");
    assert_eq!(pane.description.as_deref(), Some("running focused tests"));
    assert_eq!(pane.role_profile.as_deref(), Some("planner"));
    assert_eq!(
        state.workspaces[0].tabs[0].description.as_deref(),
        Some("running focused tests")
    );

    let round = state.to_session_snapshot();
    assert_eq!(
        round.workspaces[0].tabs[0].description.as_deref(),
        Some("running focused tests")
    );
    match &round.workspaces[0].tabs[0].split_tree {
        SplitNodeSnapshot::Leaf { description, .. } => {
            assert_eq!(description.as_deref(), Some("running focused tests"));
        }
        _ => panic!("expected a leaf"),
    }
}

#[test]
fn spawned_by_and_report_target_restore_intact_not_none() {
    let leaf = SplitNodeSnapshot::Leaf {
        pane_id: "worker-1".into(),
        cwd: None,
        title: Some("Worker".into()),
        description: None,
        title_level: Some("User".into()),
        agent_type: Some("claude".into()),
        agent_type_locked: true,
        claude_session_id: None,
        role: Some("Worker".into()),
        role_profile: None,
        parent_pane_id: Some("orch-1".into()),
        spawned_by_pane_id: Some("spawner-1".into()),
        report_target_pane_id: Some("target-1".into()),
        report_target_mesh_peer: None,
        can_spawn: false,
        depth: 2,
        chat_view: Some(false),
        auto_approve: false,
        agent_session_model: None,
        chatbackend_plugin_id: None,
        chatbackend_session_id: None,
    };
    let state = MultiplexerState::from_session_snapshot(&snapshot_with(leaf));
    let pane = state.panes.get("worker-1").expect("pane restored");
    assert_eq!(
        pane.spawned_by_pane_id.as_deref(),
        Some("spawner-1"),
        "spawned_by must survive restore, not reset to None"
    );
    assert_eq!(
        pane.report_target_pane_id.as_deref(),
        Some("target-1"),
        "report_target must survive restore, not reset to None"
    );
    assert_eq!(pane.parent_pane_id.as_deref(), Some("orch-1"));
}

#[test]
fn legacy_tab_title_seeds_default_provider_pane_title() {
    let leaf = SplitNodeSnapshot::Leaf {
        pane_id: "legacy-worker".into(),
        cwd: None,
        title: Some("claude".into()),
        description: None,
        title_level: Some("Default".into()),
        agent_type: Some("claude".into()),
        agent_type_locked: true,
        claude_session_id: None,
        role: Some("Worker".into()),
        role_profile: None,
        parent_pane_id: None,
        spawned_by_pane_id: None,
        report_target_pane_id: None,
        report_target_mesh_peer: None,
        can_spawn: false,
        depth: 2,
        chat_view: Some(false),
        auto_approve: false,
        agent_session_model: None,
        chatbackend_plugin_id: None,
        chatbackend_session_id: None,
    };
    let mut snap = snapshot_with(leaf);
    snap.workspaces[0].tabs[0].title = "Euler".into();

    let state = MultiplexerState::from_session_snapshot(&snap);
    let pane = state.panes.get("legacy-worker").expect("pane restored");
    assert_eq!(pane.title, "Euler");
    assert_eq!(pane.title_level, TitleLevel::Default);
}

#[test]
fn remote_report_target_round_trips_through_snapshot() {
    // restarted worker still relays its reports back across the mesh.
    let (mut state, _root) = MultiplexerState::new();
    let wsid = state.list_workspaces()[0].id.clone();
    let (_tab, pane_id) = state.create_tab(&wsid).unwrap();
    if let Some(p) = state.get_pane_mut(&pane_id) {
        p.report_target_pane_id = Some("main".into());
        p.report_target_mesh_peer = Some("Mac-Air".into());
    }
    let snap = state.to_session_snapshot();
    let restored = MultiplexerState::from_session_snapshot(&snap);
    let pane = restored.panes.get(&pane_id).expect("pane restored");
    assert_eq!(pane.report_target_pane_id.as_deref(), Some("main"));
    assert_eq!(
        pane.report_target_mesh_peer.as_deref(),
        Some("Mac-Air"),
        "remote report peer must survive restore"
    );
}

#[test]
fn session_snapshot_no_longer_carries_closed_history() {
    // session snapshot must stay lean and not round-trip it.
    let (mut state, _root) = MultiplexerState::new();
    let wsid = state.list_workspaces()[0].id.clone();
    let (tab_a, _pane_a) = state.create_tab(&wsid).unwrap();
    state.close_tab(&wsid, &tab_a).unwrap();
    assert_eq!(
        state.list_recently_closed(&wsid).len(),
        1,
        "recorded in sqlite"
    );

    let snap = state.to_session_snapshot();
    let json = serde_json::to_value(&snap).unwrap();
    assert!(
        json["workspaces"][0].get("recently_closed").is_none(),
        "session snapshot must not carry the closed-history ring"
    );
}

/// A chatBackend pane stays chatBackend after a save + restore cycle.
/// `pane_chat_backing` returning `Some` is what gates the send path into the
/// plugin route instead of the PTY route — this invariant must hold after
/// /new triggers a snapshot re-save (simulated as a full round-trip here).
#[test]
fn chatbackend_pane_stays_chat_backend_after_snapshot_resave() {
    // Start with a live chatBackend pane (simulates post-spawn state).
    let (mut state, _root) = MultiplexerState::new();
    let wsid = state.list_workspaces()[0].id.clone();
    let (_, pane_id) = state.create_tab(&wsid).unwrap();
    {
        let pane = state.get_pane_mut(&pane_id).unwrap();
        pane.backing = PaneBacking::ChatBackend {
            plugin_id: "lmstudio".into(),
            session_id: "sess-abc".into(),
        };
        pane.chat_view = true;
    }

    // Save → restore (what happens after /new triggers the 30s save + restart).
    let snap = state.to_session_snapshot();
    let restored = MultiplexerState::from_session_snapshot(&snap);
    let pane = restored.panes.get(&pane_id).expect("pane survived restore");

    // Backing must still be ChatBackend — not Pty — after the round-trip.
    assert!(
        pane.is_chat_backend(),
        "chatBackend pane must stay chatBackend after save+restore; \
         pane_chat_backing() returns None when backing=Pty, causing the \
         send path to mis-route to PTY and throw 'Pane has no PTY'"
    );
    assert_eq!(
        pane.backing,
        PaneBacking::ChatBackend {
            plugin_id: "lmstudio".into(),
            session_id: "sess-abc".into(),
        }
    );
    // Routing invariant: only a ChatBackend backing routes to the plugin.
    // A Pty backing would fall through to ensure_pane_pty_for_send and fail.
    let routes_to_plugin = matches!(pane.backing, PaneBacking::ChatBackend { .. });
    assert!(routes_to_plugin, "send must route to plugin, not PTY path");
}

#[test]
fn snapshot_with_status_mirrors_tracked_pty_status_only() {
    use crate::agent::status::{PaneStatus, PaneStatusTracker};

    let (mut state, _root) = MultiplexerState::new();
    let wsid = state.list_workspaces()[0].id.clone();
    let (_tab, waiting_pane) = state.create_tab(&wsid).unwrap();
    let (_tab2, working_pane) = state.create_tab(&wsid).unwrap();
    let (_tab3, untracked_pane) = state.create_tab(&wsid).unwrap();
    state.get_pane_mut(&waiting_pane).unwrap().pty_id = Some("pty-waiting".into());
    state.get_pane_mut(&working_pane).unwrap().pty_id = Some("pty-working".into());
    state.get_pane_mut(&untracked_pane).unwrap().pty_id = Some("pty-untracked".into());

    let mut tracker = PaneStatusTracker::new();
    tracker.update_output("pty-waiting", [1u8; 32]);
    tracker.set_waiting("pty-waiting");
    tracker.update_output("pty-working", [2u8; 32]);

    let snap = state.snapshot_with_status(&tracker);
    assert_eq!(
        snap.pane_statuses.get(&waiting_pane),
        Some(&PaneStatus::Waiting),
        "tracked pending_prompt pty's status must be mirrored into the snapshot"
    );
    assert_eq!(
        snap.pane_statuses.get(&working_pane),
        Some(&PaneStatus::Working),
        "tracked working pty's status must be mirrored into the snapshot too"
    );
    assert!(
        !snap.pane_statuses.contains_key(&untracked_pane),
        "a pty the tracker has never seen must be absent, not defaulted to Dead"
    );
}

fn test_pending_prompt() -> crate::multiplexer::types::PendingPrompt {
    crate::multiplexer::types::PendingPrompt {
        question: "Allow?".into(),
        options: vec![crate::multiplexer::types::PromptOption {
            label: "Yes".into(),
            value: "1".into(),
        }],
        sections: None,
        multi_select: None,
        submit_available: None,
        selected_values: None,
    }
}

#[test]
fn snapshot_with_status_derives_waiting_from_pending_prompt_over_working() {
    use crate::agent::status::{PaneStatus, PaneStatusTracker};

    let (mut state, _root) = MultiplexerState::new();
    let wsid = state.list_workspaces()[0].id.clone();
    let (_tab, pane_id) = state.create_tab(&wsid).unwrap();
    {
        let pane = state.get_pane_mut(&pane_id).unwrap();
        pane.pty_id = Some("pty-prompted".into());
        pane.pending_prompt = Some(test_pending_prompt());
    }

    let mut tracker = PaneStatusTracker::new();
    tracker.update_output("pty-prompted", [3u8; 32]);
    assert_eq!(tracker.get_status("pty-prompted"), PaneStatus::Working);

    let snap = state.snapshot_with_status(&tracker);
    assert_eq!(
        snap.pane_statuses.get(&pane_id),
        Some(&PaneStatus::Waiting),
        "pending_prompt must win over a raw Working tracker status in the snapshot"
    );
}

#[test]
fn snapshot_with_status_derives_waiting_from_pending_prompt_when_untracked() {
    use crate::agent::status::{PaneStatus, PaneStatusTracker};

    let (mut state, _root) = MultiplexerState::new();
    let wsid = state.list_workspaces()[0].id.clone();
    let (_tab, pane_id) = state.create_tab(&wsid).unwrap();
    state.get_pane_mut(&pane_id).unwrap().pending_prompt = Some(test_pending_prompt());

    let tracker = PaneStatusTracker::new();
    let snap = state.snapshot_with_status(&tracker);
    assert_eq!(
        snap.pane_statuses.get(&pane_id),
        Some(&PaneStatus::Waiting),
        "untracked/no-pty pane with a pending_prompt must still hydrate as Waiting"
    );
}

#[test]
fn snapshot_with_status_keeps_dead_over_stale_pending_prompt() {
    // pending_prompt that never got cleared on exit — Dead is terminal.
    use crate::agent::status::{PaneStatus, PaneStatusTracker};

    let (mut state, _root) = MultiplexerState::new();
    let wsid = state.list_workspaces()[0].id.clone();
    let (_tab, pane_id) = state.create_tab(&wsid).unwrap();
    {
        let pane = state.get_pane_mut(&pane_id).unwrap();
        pane.pty_id = Some("pty-dead".into());
        pane.pending_prompt = Some(test_pending_prompt());
    }

    let mut tracker = PaneStatusTracker::new();
    tracker.update_output("pty-dead", [4u8; 32]);
    tracker.set_dead("pty-dead");

    let snap = state.snapshot_with_status(&tracker);
    assert_eq!(
        snap.pane_statuses.get(&pane_id),
        Some(&PaneStatus::Dead),
        "a confirmed-dead pty must stay Dead even with a stale pending_prompt"
    );
}

#[test]
fn spawned_by_round_trips_through_snapshot() {
    // state → snapshot → state preserves the hierarchy linkage fields.
    let leaf = SplitNodeSnapshot::Leaf {
        pane_id: "w".into(),
        cwd: None,
        title: Some("Worker".into()),
        description: None,
        title_level: Some("User".into()),
        agent_type: Some("claude".into()),
        agent_type_locked: true,
        claude_session_id: None,
        role: Some("Worker".into()),
        role_profile: None,
        parent_pane_id: Some("orch".into()),
        spawned_by_pane_id: Some("spawner".into()),
        report_target_pane_id: None,
        report_target_mesh_peer: None,
        can_spawn: false,
        depth: 2,
        chat_view: Some(false),
        auto_approve: false,
        agent_session_model: None,
        chatbackend_plugin_id: None,
        chatbackend_session_id: None,
    };
    let state = MultiplexerState::from_session_snapshot(&snapshot_with(leaf));
    let round = state.to_session_snapshot();
    match &round.workspaces[0].tabs[0].split_tree {
        SplitNodeSnapshot::Leaf {
            spawned_by_pane_id,
            report_target_pane_id,
            ..
        } => {
            assert_eq!(spawned_by_pane_id.as_deref(), Some("spawner"));
            assert_eq!(report_target_pane_id.as_deref(), None);
        }
        _ => panic!("expected a leaf"),
    }
}

// ── Ghost-tab repro harness ───────────────────────────────────────────────

/// PRIMARY REPRO — simulates a session snapshot saved BEFORE skip_snapshot
/// was enforced. The picker pane IS in the snapshot, gets restored as a
/// live pane, and must be dropped by purge_snapshot_ghosts rather than
/// rendered as a ghost tab after restart.
///
/// Failure mode (pre-fix): the tab survives restore, appears in the sidebar
/// with title "Get started", and the pane exists in /api/panes but has no
/// PTY — a meaningless ghost that confuses the user.
#[test]
fn ghost_repro_restore_from_old_snapshot_drops_picker_pane() {
    use crate::service::general_agent::{purge_snapshot_ghosts, FIRST_RUN_PANE_MARKER};

    // the picker pane IS present with its marker description.
    let old_snapshot = SessionSnapshot {
        version: "test".into(),
        timestamp: 0,
        active_workspace_id: Some("ws".into()),
        workspaces: vec![WorkspaceSnapshot {
            id: "ws".into(),
            name: "Welcome".into(),
            active_tab_id: Some("tab-picker".into()),
            cwd: None,
            protected: false,
            sort_order: 0,
            tabs: vec![TabSnapshot {
                id: "tab-picker".into(),
                title: "Get started".into(),
                description: Some(FIRST_RUN_PANE_MARKER.into()),
                split_tree: SplitNodeSnapshot::Leaf {
                    pane_id: "pane-picker".into(),
                    cwd: None,
                    title: Some("Get started".into()),
                    description: Some(FIRST_RUN_PANE_MARKER.into()),
                    title_level: Some("User".into()),
                    agent_type: None,
                    agent_type_locked: false,
                    claude_session_id: None,
                    role: None,
                    role_profile: None,
                    parent_pane_id: None,
                    spawned_by_pane_id: None,
                    report_target_pane_id: None,
                    report_target_mesh_peer: None,
                    can_spawn: false,
                    depth: 0,
                    chat_view: Some(false),
                    auto_approve: false,
                    agent_session_model: None,
                    chatbackend_plugin_id: None,
                    chatbackend_session_id: None,
                },
                sort_order: 0,
            }],
        }],
    };

    // pane IS present in the restored state (it leaked in before the fix).
    let mut restored = MultiplexerState::from_session_snapshot(&old_snapshot);
    assert!(
        restored.panes.contains_key("pane-picker"),
        "pre-purge: picker pane must be present in restored state (proving the ghost-tab bug exists)"
    );
    assert_eq!(
        restored.get_workspace("ws").map(|w| w.tabs.len()),
        Some(1),
        "pre-purge: workspace must have the ghost tab"
    );

    // records it in Recently Closed.
    let dropped = purge_snapshot_ghosts(&mut restored);
    assert_eq!(dropped, 1, "purge must drop exactly one ghost tab");

    // /api/panes; Recently Closed records the drop.
    assert!(
        !restored.panes.contains_key("pane-picker"),
        "post-purge: picker pane must be absent from panes map"
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

/// Repro: a pane marked skip_snapshot (first-run picker or any ephemeral
/// onboarding pane) must be absent from the serialised session snapshot.
/// Without the fix this test fails — the tab IS included, then restores as
/// a ghost after a daemon restart.
#[test]
fn ghost_repro_skip_snapshot_pane_excluded_from_snapshot() {
    let mut state = MultiplexerState::empty();
    let wsid = state.create_workspace("Test".to_string(), None).id;
    let (_tab, pane_id) = state.create_tab(&wsid).unwrap();
    state.get_pane_mut(&pane_id).unwrap().skip_snapshot = true;

    let snap = state.to_session_snapshot();
    assert_eq!(
        snap.workspaces[0].tabs.len(),
        0,
        "skip_snapshot pane must be excluded from the session snapshot — \
         if it weren't, a daemon restart would restore it as a ghost tab"
    );
}

/// Happy path: a regular (non-ephemeral) pane must survive the round-trip
/// and not be accidentally dropped by the ghost-tab fix.
#[test]
fn ghost_repro_normal_pane_survives_snapshot_round_trip() {
    let mut state = MultiplexerState::empty();
    let wsid = state.create_workspace("Test".to_string(), None).id;
    let (_tab, pane_id) = state.create_tab(&wsid).unwrap();
    state.get_pane_mut(&pane_id).unwrap().title = "My session".to_string();
    // skip_snapshot defaults to false

    let snap = state.to_session_snapshot();
    assert_eq!(
        snap.workspaces[0].tabs.len(),
        1,
        "regular pane must survive the session snapshot round-trip"
    );

    let restored = MultiplexerState::from_session_snapshot(&snap);
    assert!(
        restored.panes.contains_key(&pane_id),
        "restored state must include the regular pane"
    );
}

/// Picker pane (first-run) is excluded from snapshot so it cannot restore
/// as a ghost tab — verified end-to-end through ensure_first_run_tab.
#[test]
fn ghost_repro_first_run_picker_excluded_from_snapshot() {
    use crate::service::general_agent::ensure_first_run_tab;

    let mut state = MultiplexerState::empty();
    ensure_first_run_tab(&mut state);

    // Sanity: picker pane was created.
    assert!(!state.list_workspaces().is_empty());
    assert!(!state.list_workspaces()[0].pane_ids.is_empty());

    let snap = state.to_session_snapshot();
    assert_eq!(
        snap.workspaces[0].tabs.len(),
        0,
        "first-run picker pane must be excluded from the session snapshot \
         to prevent ghost tabs after a daemon restart"
    );
}

/// Mixed workspace: only the ephemeral pane is dropped; the real pane survives.
#[test]
fn ghost_repro_mixed_workspace_drops_only_ephemeral() {
    let mut state = MultiplexerState::empty();
    let wsid = state.create_workspace("Test".to_string(), None).id;

    let (_tab_real, pane_real) = state.create_tab(&wsid).unwrap();
    state.get_pane_mut(&pane_real).unwrap().title = "Real session".to_string();

    let (_tab_picker, pane_picker) = state.create_tab(&wsid).unwrap();
    state.get_pane_mut(&pane_picker).unwrap().skip_snapshot = true;

    let snap = state.to_session_snapshot();
    assert_eq!(
        snap.workspaces[0].tabs.len(),
        1,
        "only the real pane must survive; the ephemeral picker must be excluded"
    );
    let surviving_leaf = &snap.workspaces[0].tabs[0].split_tree;
    match surviving_leaf {
        crate::session::store::SplitNodeSnapshot::Leaf { pane_id, .. } => {
            assert_eq!(pane_id, &pane_real, "surviving tab must be the real pane");
        }
        _ => panic!("expected a leaf"),
    }
}
