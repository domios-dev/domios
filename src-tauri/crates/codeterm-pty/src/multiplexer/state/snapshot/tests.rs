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
        team_id: None,
        parent_pane_id: None,
        spawned_by_pane_id: None,
        report_target_pane_id: None,
        report_target_mesh_peer: None,
        can_spawn: false,
        depth: 0,
        chat_view: Some(true),
        auto_approve: false,
        agent_session_model: None,
        agent_planned_model: None,
        agent_requested_model: None,
        agent_provider_resolved_model: None,
        model_fallback: None,
        model_identity_mismatch: None,
        chatbackend_plugin_id: Some(plugin_id.into()),
        chatbackend_session_id: Some(session_id.into()),
        watcher: None,
        pending_delivery: None,
    }
}

fn snapshot_with(leaf: SplitNodeSnapshot) -> SessionSnapshot {
    SessionSnapshot {
        version: "test".into(),
        timestamp: 0,
        active_workspace_id: Some("ws".into()),
        teams: vec![],
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
fn team_snapshot_roundtrip() {
    use codeterm_types::team::{Team, TeamLifecycle};

    let mut state = MultiplexerState::empty();
    let wsid = state.create_workspace("Team WS".to_string(), None).id;

    let team = Team {
        id: "team_abc123".into(),
        name: Some("Alpha".into()),
        lifecycle: TeamLifecycle::Active,
        created_at: 1_700_000_000_000,
        workspace_id: wsid.clone(),
    };
    state.upsert_team(team.clone());

    let (_t1, m1) = state.create_tab(&wsid).unwrap();
    let (_t2, m2) = state.create_tab(&wsid).unwrap();
    state.get_pane_mut(&m1).unwrap().team_id = Some(team.id.clone());
    state.get_pane_mut(&m2).unwrap().team_id = Some(team.id.clone());

    let snap = state.to_session_snapshot();
    let restored = MultiplexerState::from_session_snapshot(&snap);

    assert_eq!(
        restored.get_team("team_abc123"),
        Some(&team),
        "teams registry must survive the snapshot round-trip"
    );
    assert_eq!(restored.team_count(), 1, "exactly one team survives");
    assert_eq!(
        restored.panes.get(&m1).unwrap().team_id.as_deref(),
        Some("team_abc123"),
        "member pane team_id must survive the round-trip"
    );
    assert_eq!(
        restored.panes.get(&m2).unwrap().team_id.as_deref(),
        Some("team_abc123"),
        "second member pane team_id must survive too"
    );
}

#[test]
fn pending_record_snapshot_round_trip() {
    use crate::multiplexer::types::{
        DeliveryAttemptProvenance, PendingDeliveryChannel, PendingDeliveryRecord,
    };

    let (mut state, pane_id) = MultiplexerState::new();
    let mut pending = PendingDeliveryRecord::new(
        "e8b7be43c6c1de318beb5c4a99e5406d2d1861f55ad8c03b2b4591f8ac44d784".into(),
        PendingDeliveryChannel::InteractiveInput,
    );
    pending
        .begin_attempt(DeliveryAttemptProvenance {
            attempt_id: "delivery-17".into(),
            started_at_ms: 1_700_000_000_000,
        })
        .unwrap();
    state.get_pane_mut(&pane_id).unwrap().pending_delivery = Some(pending);

    let snapshot = state.to_session_snapshot();
    let before = serde_json::to_vec(&snapshot.workspaces[0].tabs[0].split_tree).unwrap();
    let restored = MultiplexerState::from_session_snapshot(&snapshot);
    let after_snapshot = restored.to_session_snapshot();
    let after = serde_json::to_vec(&after_snapshot.workspaces[0].tabs[0].split_tree).unwrap();

    assert_eq!(before, after);
}

#[test]
fn pending_record_absent_round_trips_absent() {
    let (state, pane_id) = MultiplexerState::new();
    assert_eq!(state.get_pane(&pane_id).unwrap().pending_delivery, None);

    let snapshot = state.to_session_snapshot();
    let restored = MultiplexerState::from_session_snapshot(&snapshot);
    assert_eq!(restored.get_pane(&pane_id).unwrap().pending_delivery, None);
}

#[test]
fn legacy_snapshot_without_field_deserializes() {
    let snapshot: SessionSnapshot = serde_json::from_value(serde_json::json!({
        "version": "legacy",
        "timestamp": 0,
        "workspaces": [{
            "id": "ws",
            "name": "ws",
            "tabs": [{
                "id": "tab",
                "title": "tab",
                "split_tree": { "type": "Leaf", "pane_id": "pane", "cwd": null }
            }],
            "active_tab_id": "tab"
        }],
        "active_workspace_id": "ws"
    }))
    .unwrap();

    let restored = MultiplexerState::from_session_snapshot(&snapshot);
    assert_eq!(restored.get_pane("pane").unwrap().pending_delivery, None);
}

#[test]
fn manager_close_survives_snapshot_restore_without_resurrection() {
    use crate::multiplexer::types::AgentRole;

    let (mut state, root_id) = MultiplexerState::new();
    let workspace_id = state.list_workspaces()[0].id.clone();
    state
        .set_pane_role(&root_id, AgentRole::Orchestrator, None)
        .unwrap();
    let team_id = state
        .resolve_or_mint_team_for_parent(&root_id)
        .unwrap()
        .team_id;
    let (_tab, manager_id) = state.create_tab(&workspace_id).unwrap();
    state
        .set_pane_role(&manager_id, AgentRole::Manager, Some(&root_id))
        .unwrap();
    let (_tab, worker_id) = state.create_tab(&workspace_id).unwrap();
    state
        .set_pane_role(&worker_id, AgentRole::Worker, Some(&root_id))
        .unwrap();
    state.repoint_parent(&worker_id, &manager_id);

    state.close_pane(&manager_id).unwrap();
    let restored = MultiplexerState::from_session_snapshot(&state.to_session_snapshot());

    assert!(restored.get_pane(&manager_id).is_none());
    let worker = restored.get_pane(&worker_id).expect("worker restores");
    assert!(worker.parent_pane_id.is_none());
    assert_eq!(worker.team_id.as_deref(), Some(team_id.as_str()));
    assert!(restored.get_team(&team_id).is_some());
}

#[test]
fn snapshot_includes_teams() {
    use codeterm_types::team::Team;

    let mut state = MultiplexerState::empty();
    let workspace_id = state.create_workspace("Team WS".to_string(), None).id;
    let team = Team::new(&workspace_id, 1);
    let team_id = team.id.clone();
    state.upsert_team(team);
    let (_tab_id, pane_id) = state.create_tab(&workspace_id).unwrap();
    state.get_pane_mut(&pane_id).unwrap().team_id = Some(team_id.clone());

    let snapshot = serde_json::to_value(state.snapshot()).unwrap();
    assert_eq!(snapshot["teams"][0]["id"], team_id);
    assert_eq!(snapshot["panes"][pane_id]["team_id"], team_id);
}

#[test]
fn legacy_snapshot_no_team_fields_restores() {
    // A pre-team last.json lacks both the top-level `teams` array and each
    // leaf's `team_id`. Emulate one by stripping those keys from a current
    // snapshot, then prove it still restores cleanly and teamless.
    let (mut state, _root) = MultiplexerState::new();
    let wsid = state.list_workspaces()[0].id.clone();
    let (_t, pane_id) = state.create_tab(&wsid).unwrap();

    let snap = state.to_session_snapshot();
    let mut json = serde_json::to_value(&snap).unwrap();
    json.as_object_mut().unwrap().remove("teams");
    for ws in json["workspaces"].as_array_mut().unwrap() {
        for tab in ws["tabs"].as_array_mut().unwrap() {
            tab["split_tree"].as_object_mut().unwrap().remove("team_id");
        }
    }
    assert!(
        json.get("teams").is_none(),
        "fixture must genuinely lack the new team fields"
    );

    let legacy: SessionSnapshot =
        serde_json::from_value(json).expect("legacy snapshot must deserialize with defaults");
    let restored = MultiplexerState::from_session_snapshot(&legacy);

    assert_eq!(restored.team_count(), 0, "legacy snapshot mints no teams");
    assert!(
        restored.panes.contains_key(&pane_id),
        "legacy pane must restore"
    );
    assert_eq!(
        restored.panes.get(&pane_id).unwrap().team_id,
        None,
        "legacy pane restores teamless"
    );
}

#[test]
fn watcher_spec_round_trips_through_snapshot() {
    use crate::multiplexer::types::{WatcherPolicy, WatcherSpec};

    let (mut state, _root) = MultiplexerState::new();
    let wsid = state.list_workspaces()[0].id.clone();
    let (_, pane_id) = state.create_tab(&wsid).unwrap();
    let spec = WatcherSpec {
        charter: "Observe this orchestration.".into(),
        policy: WatcherPolicy::clamped(Some(90), Some(120), Some(2)),
        observations: serde_json::json!({"orchestration": true, "chatTail": 2}),
        state: serde_json::json!({"round": 1}),
        tick: 4,
        last_tick_ms: Some(1_700_000_000_000),
        last_status: Some("attention".into()),
        tick_inflight_since_ms: Some(1_700_000_000_100),
    };
    state.get_pane_mut(&pane_id).unwrap().watcher = Some(spec.clone());

    let snap = state.to_session_snapshot();
    let restored = MultiplexerState::from_session_snapshot(&snap);
    let pane = restored.panes.get(&pane_id).expect("pane restored");
    assert_eq!(
        pane.watcher,
        Some(spec.clone()),
        "WatcherSpec must survive session snapshot round-trip"
    );

    let round = restored.to_session_snapshot();
    let again = MultiplexerState::from_session_snapshot(&round);
    assert_eq!(
        again.panes.get(&pane_id).unwrap().watcher,
        Some(spec),
        "second round-trip must preserve WatcherSpec identically"
    );
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
        team_id: None,
        parent_pane_id: None,
        spawned_by_pane_id: None,
        report_target_pane_id: None,
        report_target_mesh_peer: None,
        can_spawn: false,
        depth: 0,
        chat_view: Some(false),
        auto_approve: false,
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
        team_id: None,
        parent_pane_id: None,
        spawned_by_pane_id: None,
        report_target_pane_id: None,
        report_target_mesh_peer: None,
        can_spawn: false,
        depth: 2,
        chat_view: Some(false),
        auto_approve: false,
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
        team_id: None,
        parent_pane_id: Some("orch-1".into()),
        spawned_by_pane_id: Some("spawner-1".into()),
        report_target_pane_id: Some("target-1".into()),
        report_target_mesh_peer: None,
        can_spawn: false,
        depth: 2,
        chat_view: Some(false),
        auto_approve: false,
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
        team_id: None,
        parent_pane_id: None,
        spawned_by_pane_id: None,
        report_target_pane_id: None,
        report_target_mesh_peer: None,
        can_spawn: false,
        depth: 2,
        chat_view: Some(false),
        auto_approve: false,
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
    use codeterm_types::pty::{PaneStatus, PaneStatusTracker};

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
    use codeterm_types::pty::{PaneStatus, PaneStatusTracker};

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
    use codeterm_types::pty::{PaneStatus, PaneStatusTracker};

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
    use codeterm_types::pty::{PaneStatus, PaneStatusTracker};

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
        team_id: None,
        parent_pane_id: Some("orch".into()),
        spawned_by_pane_id: Some("spawner".into()),
        report_target_pane_id: None,
        report_target_mesh_peer: None,
        can_spawn: false,
        depth: 2,
        chat_view: Some(false),
        auto_approve: false,
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

#[test]
fn requested_and_actual_model_identity_round_trips_through_snapshot() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = state.list_workspaces()[0].id.clone();
    let (_, pane_id) = state.create_tab(&wsid).unwrap();
    let mismatch = crate::multiplexer::types::ModelIdentityMismatch {
        planned_model: Some("codex/gpt-5.6-luna".into()),
        requested_model: "gpt-5.6-luna".into(),
        provider_resolved_model: Some("gpt-5.6-luna".into()),
        actual_model: "gpt-5.6-terra".into(),
        reason: "runtime_session_model_mismatch".into(),
    };
    let fallback = crate::multiplexer::types::ModelFallbackNotice {
        planned_model: Some("codex/gpt-5.6-luna".into()),
        requested_model: "gpt-5.6-luna".into(),
        provider_resolved_model: "gpt-5.6-luna".into(),
        actual_model: Some("gpt-5.6-terra".into()),
        reason: "runtime_session_model_mismatch".into(),
    };
    {
        let pane = state.get_pane_mut(&pane_id).unwrap();
        pane.agent_type = Some("codex".into());
        pane.agent_session_model = Some("gpt-5.6-terra".into());
        pane.agent_planned_model = Some("codex/gpt-5.6-luna".into());
        pane.agent_requested_model = Some("gpt-5.6-luna".into());
        pane.agent_provider_resolved_model = Some("gpt-5.6-luna".into());
        pane.model_fallback = Some(fallback.clone());
        pane.model_identity_mismatch = Some(mismatch.clone());
    }

    let snap = state.to_session_snapshot();
    let restored = MultiplexerState::from_session_snapshot(&snap);
    let pane = restored.panes.get(&pane_id).expect("pane restored");

    assert_eq!(pane.agent_session_model.as_deref(), Some("gpt-5.6-terra"));
    assert_eq!(
        pane.agent_planned_model.as_deref(),
        Some("codex/gpt-5.6-luna")
    );
    assert_eq!(pane.agent_requested_model.as_deref(), Some("gpt-5.6-luna"));
    assert_eq!(
        pane.agent_provider_resolved_model.as_deref(),
        Some("gpt-5.6-luna")
    );
    assert_eq!(pane.model_fallback, Some(fallback));
    assert_eq!(pane.model_identity_mismatch, Some(mismatch));
}

// ── Ghost-tab repro harness ───────────────────────────────────────────────

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

#[test]
fn snapshots_exclude_orphaned_and_duplicate_live_tabs() {
    let mut state = MultiplexerState::empty();
    let workspace_id = state.create_workspace("Clean".to_string(), None).id;
    let (tab_id, pane_id) = state.create_tab(&workspace_id).unwrap();
    let workspace = state
        .workspaces
        .iter_mut()
        .find(|workspace| workspace.id == workspace_id)
        .unwrap();
    workspace.tabs.push(crate::multiplexer::types::Tab {
        id: "duplicate-tab".to_string(),
        description: None,
        pane_id: pane_id.clone(),
        sort_order: 1,
    });
    workspace.tabs.push(crate::multiplexer::types::Tab {
        id: "orphaned-tab".to_string(),
        description: None,
        pane_id: "closed-pane".to_string(),
        sort_order: 2,
    });
    workspace.active_tab_id = Some("orphaned-tab".to_string());

    let live = state.snapshot();
    assert_eq!(live.workspaces[0].tabs.len(), 1);
    assert_eq!(live.workspaces[0].tabs[0].id, tab_id);
    assert_eq!(
        live.workspaces[0].active_tab_id.as_deref(),
        Some(tab_id.as_str())
    );

    let persisted = state.to_session_snapshot();
    assert_eq!(persisted.workspaces[0].tabs.len(), 1);
    assert_eq!(persisted.workspaces[0].tabs[0].id, tab_id);
}
