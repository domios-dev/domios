use super::*;
use crate::session::store::{SessionSnapshot, SplitNodeSnapshot, TabSnapshot, WorkspaceSnapshot};

fn chatbackend_leaf(tab_id: &str, plugin_id: &str, session_id: &str) -> SplitNodeSnapshot {
    SplitNodeSnapshot::Leaf {
        tab_id: tab_id.into(),
        cwd: None,
        title: Some("LM Studio".into()),
        description: None,
        title_level: Some("User".into()),
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
        chat_view: Some(true),
        auto_approve: false,
        audience: TabAudience::User,
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

    let m1 = state.create_tab(&wsid).unwrap();
    let m2 = state.create_tab(&wsid).unwrap();
    state.get_tab_mut(&m1).unwrap().team_id = Some(team.id.clone());
    state.get_tab_mut(&m2).unwrap().team_id = Some(team.id.clone());

    let snap = state.to_session_snapshot();
    let restored = MultiplexerState::from_session_snapshot(&snap);

    assert_eq!(
        restored.get_team("team_abc123"),
        Some(&team),
        "teams registry must survive the snapshot round-trip"
    );
    assert_eq!(restored.team_count(), 1, "exactly one team survives");
    assert_eq!(
        restored.tabs.get(&m1).unwrap().team_id.as_deref(),
        Some("team_abc123"),
        "member Tab team_id must survive the round-trip"
    );
    assert_eq!(
        restored.tabs.get(&m2).unwrap().team_id.as_deref(),
        Some("team_abc123"),
        "second member Tab team_id must survive too"
    );
}

#[test]
fn pending_record_snapshot_round_trip() {
    use crate::multiplexer::types::{
        DeliveryAttemptProvenance, PendingDeliveryChannel, PendingDeliveryRecord,
    };

    let (mut state, tab_id) = MultiplexerState::new();
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
    state.get_tab_mut(&tab_id).unwrap().pending_delivery = Some(pending);

    let snapshot = state.to_session_snapshot();
    let before = serde_json::to_vec(&snapshot.workspaces[0].tabs[0].split_tree).unwrap();
    let restored = MultiplexerState::from_session_snapshot(&snapshot);
    let after_snapshot = restored.to_session_snapshot();
    let after = serde_json::to_vec(&after_snapshot.workspaces[0].tabs[0].split_tree).unwrap();

    assert_eq!(before, after);
}

#[test]
fn pending_record_absent_round_trips_absent() {
    let (state, tab_id) = MultiplexerState::new();
    assert_eq!(state.get_tab(&tab_id).unwrap().pending_delivery, None);

    let snapshot = state.to_session_snapshot();
    let restored = MultiplexerState::from_session_snapshot(&snapshot);
    assert_eq!(restored.get_tab(&tab_id).unwrap().pending_delivery, None);
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
                "split_tree": { "type": "Leaf", "tab_id": "tab", "cwd": null }
            }],
            "active_tab_id": "tab"
        }],
        "active_workspace_id": "ws"
    }))
    .unwrap();

    let restored = MultiplexerState::from_session_snapshot(&snapshot);
    assert_eq!(restored.get_tab("tab").unwrap().pending_delivery, None);
}

#[test]
fn manager_close_survives_snapshot_restore_without_resurrection() {
    use crate::multiplexer::types::AgentRole;

    let (mut state, root_id) = MultiplexerState::new();
    let workspace_id = state.list_workspaces()[0].id.clone();
    state
        .set_tab_role(&root_id, AgentRole::Orchestrator, None)
        .unwrap();
    let team_id = state
        .resolve_or_mint_team_for_parent(&root_id)
        .unwrap()
        .team_id;
    let manager_id = state.create_tab(&workspace_id).unwrap();
    state
        .set_tab_role(&manager_id, AgentRole::Manager, Some(&root_id))
        .unwrap();
    let worker_id = state.create_tab(&workspace_id).unwrap();
    state
        .set_tab_role(&worker_id, AgentRole::Worker, Some(&root_id))
        .unwrap();
    state.repoint_parent(&worker_id, &manager_id);

    state.close_tab(&manager_id).unwrap();
    let restored = MultiplexerState::from_session_snapshot(&state.to_session_snapshot());

    assert!(restored.get_tab(&manager_id).is_none());
    let worker = restored.get_tab(&worker_id).expect("worker restores");
    assert!(worker.parent_tab_id.is_none());
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
    let tab_id = state.create_tab(&workspace_id).unwrap();
    state.get_tab_mut(&tab_id).unwrap().team_id = Some(team_id.clone());

    let snapshot = serde_json::to_value(state.snapshot()).unwrap();
    assert_eq!(snapshot["teams"][0]["id"], team_id);
    assert_eq!(snapshot["tabs"][tab_id]["team_id"], team_id);
}

#[test]
fn legacy_snapshot_no_team_fields_restores() {
    // A pre-team last.json lacks both the top-level `teams` array and each
    // leaf's `team_id`. Emulate one by stripping those keys from a current
    // snapshot, then prove it still restores cleanly and teamless.
    let (mut state, _root) = MultiplexerState::new();
    let wsid = state.list_workspaces()[0].id.clone();
    let tab_id = state.create_tab(&wsid).unwrap();

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
        restored.tabs.contains_key(&tab_id),
        "legacy Tab must restore"
    );
    assert_eq!(
        restored.tabs.get(&tab_id).unwrap().team_id,
        None,
        "legacy Tab restores teamless"
    );
}

#[test]
fn watcher_spec_round_trips_through_snapshot() {
    use crate::multiplexer::types::{WatcherPolicy, WatcherSpec};

    let (mut state, _root) = MultiplexerState::new();
    let wsid = state.list_workspaces()[0].id.clone();
    let tab_id = state.create_tab(&wsid).unwrap();
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
    state.get_tab_mut(&tab_id).unwrap().watcher = Some(spec.clone());

    let snap = state.to_session_snapshot();
    let restored = MultiplexerState::from_session_snapshot(&snap);
    let tab = restored.tabs.get(&tab_id).expect("Tab restored");
    assert_eq!(
        tab.watcher,
        Some(spec.clone()),
        "WatcherSpec must survive session snapshot round-trip"
    );

    let round = restored.to_session_snapshot();
    let again = MultiplexerState::from_session_snapshot(&round);
    assert_eq!(
        again.tabs.get(&tab_id).unwrap().watcher,
        Some(spec),
        "second round-trip must preserve WatcherSpec identically"
    );
}

#[test]
fn chatbackend_backing_restores_from_snapshot() {
    let snap = snapshot_with(chatbackend_leaf("p1", "lmstudio", "sess-7"));
    let state = MultiplexerState::from_session_snapshot(&snap);
    let tab = state.tabs.get("p1").expect("Tab restored");
    assert_eq!(
        tab.backing,
        TabBacking::ChatBackend {
            plugin_id: "lmstudio".into(),
            session_id: "sess-7".into(),
        },
        "restored as chatBackend, not Pty"
    );
    // never mounts a terminal — the "comes back as a terminal" guard.
    assert!(tab.is_chat_backend(), "renders chat, not terminal");
    assert!(tab.restored_from_snapshot, "marked for lazy re-open");
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
fn stale_legacy_pty_snapshot_restores_sandboxed_without_chatbackend_fields() {
    // backing on round-trip.
    let leaf = SplitNodeSnapshot::Leaf {
        tab_id: "p2".into(),
        cwd: None,
        title: Some("claude".into()),
        description: None,
        title_level: Some("User".into()),
        agent_type: Some("claude".into()),
        agent_type_locked: true,
        claude_session_id: Some("c-1".into()),
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
        auto_approve: true,
        audience: TabAudience::User,
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
    let tab = state.tabs.get("p2").expect("Tab restored");
    assert_eq!(tab.backing, TabBacking::Pty);
    assert!(
        !tab.auto_approve,
        "snapshot trust never authorizes a resumed launch"
    );
}

#[test]
fn automation_audience_round_trips_and_legacy_defaults_fail_closed() {
    let (mut state, tab_id) = MultiplexerState::new();
    let tab = state.get_tab_mut(&tab_id).expect("initial tab");
    tab.agent_type = Some("claude".into());
    tab.agent_session_id = Some("automation-session".into());
    tab.audience = TabAudience::Automation;
    tab.auto_approve = true;

    let snapshot = state.to_session_snapshot();
    match &snapshot.workspaces[0].tabs[0].split_tree {
        SplitNodeSnapshot::Leaf { audience, .. } => {
            assert_eq!(*audience, TabAudience::Automation);
        }
        _ => panic!("expected a leaf"),
    }
    let persisted: SessionSnapshot =
        serde_json::from_str(&serde_json::to_string(&snapshot).unwrap()).unwrap();
    let restored = MultiplexerState::from_session_snapshot(&persisted);
    let restored_tab = restored.get_tab(&tab_id).expect("restored automation tab");
    assert_eq!(restored_tab.audience, TabAudience::Automation);
    assert!(
        !restored_tab.auto_approve,
        "restored automation remains sandboxed"
    );

    let mut legacy_json = serde_json::to_value(persisted).unwrap();
    legacy_json["workspaces"][0]["tabs"][0]["split_tree"]
        .as_object_mut()
        .unwrap()
        .remove("audience");
    let legacy_snapshot: SessionSnapshot = serde_json::from_value(legacy_json).unwrap();
    let legacy_restored = MultiplexerState::from_session_snapshot(&legacy_snapshot);
    let legacy_tab = legacy_restored
        .get_tab(&tab_id)
        .expect("legacy automation tab");
    assert_eq!(legacy_tab.audience, TabAudience::Automation);
    assert!(
        !legacy_tab.auto_approve,
        "missing audience must fail closed"
    );
}

#[test]
fn tab_description_round_trips_through_snapshot() {
    let leaf = SplitNodeSnapshot::Leaf {
        tab_id: "p-desc".into(),
        cwd: None,
        title: Some("Worker".into()),
        description: Some("running focused tests".into()),
        title_level: Some("User".into()),
        agent_type: Some("claude".into()),
        agent_type_locked: true,
        claude_session_id: None,
        agent_identity: None,
        role: Some("Worker".into()),
        role_profile: Some("planner".into()),
        team_id: None,
        parent_tab_id: None,
        spawned_by_tab_id: None,
        report_target_tab_id: None,
        report_target_mesh_peer: None,
        ownership: None,
        can_spawn: false,
        depth: 2,
        chat_view: Some(false),
        auto_approve: false,
        audience: TabAudience::User,
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
    let tab = state.tabs.get("p-desc").expect("Tab restored");
    assert_eq!(tab.description.as_deref(), Some("running focused tests"));
    assert_eq!(tab.role_profile.as_deref(), Some("planner"));
    assert_eq!(
        state
            .get_tab(&state.workspaces[0].tabs[0])
            .unwrap()
            .description
            .as_deref(),
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
        tab_id: "worker-1".into(),
        cwd: None,
        title: Some("Worker".into()),
        description: None,
        title_level: Some("User".into()),
        agent_type: Some("claude".into()),
        agent_type_locked: true,
        claude_session_id: None,
        agent_identity: None,
        role: Some("Worker".into()),
        role_profile: None,
        team_id: None,
        parent_tab_id: Some("orch-1".into()),
        spawned_by_tab_id: Some("spawner-1".into()),
        report_target_tab_id: Some("target-1".into()),
        report_target_mesh_peer: None,
        ownership: None,
        can_spawn: false,
        depth: 2,
        chat_view: Some(false),
        auto_approve: false,
        audience: TabAudience::User,
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
    let tab = state.tabs.get("worker-1").expect("Tab restored");
    assert_eq!(
        tab.spawned_by_tab_id.as_deref(),
        Some("spawner-1"),
        "spawned_by must survive restore, not reset to None"
    );
    assert_eq!(
        tab.report_target_tab_id.as_deref(),
        Some("target-1"),
        "report_target must survive restore, not reset to None"
    );
    assert_eq!(tab.parent_tab_id.as_deref(), Some("orch-1"));
}

#[test]
fn legacy_tab_title_seeds_default_provider_tab_title() {
    let leaf = SplitNodeSnapshot::Leaf {
        tab_id: "legacy-worker".into(),
        cwd: None,
        title: Some("claude".into()),
        description: None,
        title_level: Some("Default".into()),
        agent_type: Some("claude".into()),
        agent_type_locked: true,
        claude_session_id: None,
        agent_identity: None,
        role: Some("Worker".into()),
        role_profile: None,
        team_id: None,
        parent_tab_id: None,
        spawned_by_tab_id: None,
        report_target_tab_id: None,
        report_target_mesh_peer: None,
        ownership: None,
        can_spawn: false,
        depth: 2,
        chat_view: Some(false),
        auto_approve: false,
        audience: TabAudience::User,
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
    let tab = state.tabs.get("legacy-worker").expect("Tab restored");
    assert_eq!(tab.title, "Euler");
    assert_eq!(tab.title_level, TitleLevel::Default);
}

#[test]
fn remote_report_target_round_trips_through_snapshot() {
    // restarted worker still relays its reports back across the mesh.
    let (mut state, _root) = MultiplexerState::new();
    let wsid = state.list_workspaces()[0].id.clone();
    let tab_id = state.create_tab(&wsid).unwrap();
    if let Some(p) = state.get_tab_mut(&tab_id) {
        p.report_target_tab_id = Some("main".into());
        p.report_target_mesh_peer = Some("Mac-Air".into());
    }
    let snap = state.to_session_snapshot();
    let restored = MultiplexerState::from_session_snapshot(&snap);
    let tab = restored.tabs.get(&tab_id).expect("Tab restored");
    assert_eq!(tab.report_target_tab_id.as_deref(), Some("main"));
    assert_eq!(
        tab.report_target_mesh_peer.as_deref(),
        Some("Mac-Air"),
        "remote report peer must survive restore"
    );
}

#[test]
fn remote_ownership_round_trips_through_snapshot() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = state.list_workspaces()[0].id.clone();
    let tab_id = state.create_tab(&wsid).unwrap();
    let ownership = codeterm_types::pty::RemoteOwnership {
        controller_title: "Puccini".into(),
        controller_tab_id: Some("af5919cb".into()),
        peer_id: "peer-mac".into(),
        peer_name: "Mac-Air".into(),
        team: Some(codeterm_types::pty::RemoteOwnershipTeam {
            id: "team-remote".into(),
            title: Some("Remote Tab Portal".into()),
        }),
    };
    state.get_tab_mut(&tab_id).unwrap().ownership = Some(ownership.clone());

    let restored = MultiplexerState::from_session_snapshot(&state.to_session_snapshot());
    assert_eq!(restored.get_tab(&tab_id).unwrap().ownership, Some(ownership));
}

#[test]
fn session_snapshot_no_longer_carries_closed_history() {
    // session snapshot must stay lean and not round-trip it.
    let (mut state, _root) = MultiplexerState::new();
    let wsid = state.list_workspaces()[0].id.clone();
    let tab_a = state.create_tab(&wsid).unwrap();
    state.close_tab(&tab_a).unwrap();
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

/// A chatBackend tab stays chatBackend after a save + restore cycle.
/// `tab_chat_backing` returning `Some` is what gates the send path into the
/// plugin route instead of the PTY route — this invariant must hold after
/// /new triggers a snapshot re-save (simulated as a full round-trip here).
#[test]
fn chatbackend_tab_stays_chat_backend_after_snapshot_resave() {
    // Start with a live chatBackend tab (simulates post-spawn state).
    let (mut state, _root) = MultiplexerState::new();
    let wsid = state.list_workspaces()[0].id.clone();
    let tab_id = state.create_tab(&wsid).unwrap();
    {
        let tab = state.get_tab_mut(&tab_id).unwrap();
        tab.backing = TabBacking::ChatBackend {
            plugin_id: "lmstudio".into(),
            session_id: "sess-abc".into(),
        };
        tab.chat_view = true;
    }

    // Save → restore (what happens after /new triggers the 30s save + restart).
    let snap = state.to_session_snapshot();
    let restored = MultiplexerState::from_session_snapshot(&snap);
    let tab = restored.tabs.get(&tab_id).expect("Tab survived restore");

    // Backing must still be ChatBackend — not Pty — after the round-trip.
    assert!(
        tab.is_chat_backend(),
        "chatBackend tab must stay chatBackend after save+restore; \
         tab_chat_backing() returns None when backing=Pty, causing the \
         send path to mis-route to PTY and throw 'Tab has no PTY'"
    );
    assert_eq!(
        tab.backing,
        TabBacking::ChatBackend {
            plugin_id: "lmstudio".into(),
            session_id: "sess-abc".into(),
        }
    );
    // Routing invariant: only a ChatBackend backing routes to the plugin.
    // A Pty backing would fall through to ensure_tab_pty_for_send and fail.
    let routes_to_plugin = matches!(tab.backing, TabBacking::ChatBackend { .. });
    assert!(routes_to_plugin, "send must route to plugin, not PTY path");
}

#[test]
fn snapshot_with_status_mirrors_tracked_pty_status_only() {
    use codeterm_types::pty::{TabStatus, TabStatusTracker};

    let (mut state, _root) = MultiplexerState::new();
    let wsid = state.list_workspaces()[0].id.clone();
    let waiting_tab = state.create_tab(&wsid).unwrap();
    let working_tab = state.create_tab(&wsid).unwrap();
    let untracked_tab = state.create_tab(&wsid).unwrap();
    state.get_tab_mut(&waiting_tab).unwrap().pty_id = Some("pty-waiting".into());
    state.get_tab_mut(&working_tab).unwrap().pty_id = Some("pty-working".into());
    state.get_tab_mut(&untracked_tab).unwrap().pty_id = Some("pty-untracked".into());

    let mut tracker = TabStatusTracker::new();
    tracker.update_output("pty-waiting", [1u8; 32]);
    tracker.set_waiting("pty-waiting");
    tracker.update_output("pty-working", [2u8; 32]);

    let snap = state.snapshot_with_status(&tracker);
    assert_eq!(
        snap.tab_statuses.get(&waiting_tab),
        Some(&TabStatus::Waiting),
        "tracked pending_prompt pty's status must be mirrored into the snapshot"
    );
    assert_eq!(
        snap.tab_statuses.get(&working_tab),
        Some(&TabStatus::Working),
        "tracked working pty's status must be mirrored into the snapshot too"
    );
    assert!(
        !snap.tab_statuses.contains_key(&untracked_tab),
        "a pty the tracker has never seen must be absent, not defaulted to Dead"
    );
}

#[test]
fn snapshot_status_resolver_distinguishes_repaint_from_long_turn() {
    use codeterm_types::pty::{
        reconcile_status, BootStage, TabStatus, TabStatusTracker, TabVerdict, TimelineState,
    };

    let (mut state, _root) = MultiplexerState::new();
    let wsid = state.list_workspaces()[0].id.clone();
    let idle_tab = state.create_tab(&wsid).unwrap();
    let working_tab = state.create_tab(&wsid).unwrap();
    for (tab_id, pty_id, agent_state) in [
        (&idle_tab, "pty-idle", TimelineState::Idle),
        (&working_tab, "pty-working", TimelineState::Working),
    ] {
        let tab = state.get_tab_mut(tab_id).unwrap();
        tab.pty_id = Some(pty_id.into());
        tab.agent_type = Some("fixture".into());
        tab.boot_stage = Some(BootStage::AgentReady);
        tab.agent_state = Some(
            TabVerdict {
                state: agent_state,
                subtype: None,
            }
            .into(),
        );
    }

    let mut tracker = TabStatusTracker::new();
    tracker.update_output("pty-idle", [1u8; 32]);
    tracker.update_output("pty-working", [2u8; 32]);

    let snap = state.snapshot_with_status_resolver(&tracker, |tab, raw| {
        reconcile_status(
            raw,
            tab.boot_stage,
            tab.agent_state.as_ref().map(|state| state.state),
            true,
            tab.pending_prompt.is_some(),
        )
    });
    assert_eq!(snap.tab_statuses.get(&idle_tab), Some(&TabStatus::Idle));
    assert_eq!(
        snap.tab_statuses.get(&working_tab),
        Some(&TabStatus::Working)
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
    use codeterm_types::pty::{TabStatus, TabStatusTracker};

    let (mut state, _root) = MultiplexerState::new();
    let wsid = state.list_workspaces()[0].id.clone();
    let tab_id = state.create_tab(&wsid).unwrap();
    {
        let tab = state.get_tab_mut(&tab_id).unwrap();
        tab.pty_id = Some("pty-prompted".into());
        tab.pending_prompt = Some(test_pending_prompt());
    }

    let mut tracker = TabStatusTracker::new();
    tracker.update_output("pty-prompted", [3u8; 32]);
    assert_eq!(tracker.get_status("pty-prompted"), TabStatus::Working);

    let snap = state.snapshot_with_status(&tracker);
    assert_eq!(
        snap.tab_statuses.get(&tab_id),
        Some(&TabStatus::Waiting),
        "pending_prompt must win over a raw Working tracker status in the snapshot"
    );
}

#[test]
fn snapshot_with_status_derives_waiting_from_pending_prompt_when_untracked() {
    use codeterm_types::pty::{TabStatus, TabStatusTracker};

    let (mut state, _root) = MultiplexerState::new();
    let wsid = state.list_workspaces()[0].id.clone();
    let tab_id = state.create_tab(&wsid).unwrap();
    state.get_tab_mut(&tab_id).unwrap().pending_prompt = Some(test_pending_prompt());

    let tracker = TabStatusTracker::new();
    let snap = state.snapshot_with_status(&tracker);
    assert_eq!(
        snap.tab_statuses.get(&tab_id),
        Some(&TabStatus::Waiting),
        "untracked/no-pty Tab with a pending_prompt must still hydrate as Waiting"
    );
}

#[test]
fn snapshot_with_status_keeps_dead_over_stale_pending_prompt() {
    // pending_prompt that never got cleared on exit — Dead is terminal.
    use codeterm_types::pty::{TabStatus, TabStatusTracker};

    let (mut state, _root) = MultiplexerState::new();
    let wsid = state.list_workspaces()[0].id.clone();
    let tab_id = state.create_tab(&wsid).unwrap();
    {
        let tab = state.get_tab_mut(&tab_id).unwrap();
        tab.pty_id = Some("pty-dead".into());
        tab.pending_prompt = Some(test_pending_prompt());
    }

    let mut tracker = TabStatusTracker::new();
    tracker.update_output("pty-dead", [4u8; 32]);
    tracker.set_dead("pty-dead");

    let snap = state.snapshot_with_status(&tracker);
    assert_eq!(
        snap.tab_statuses.get(&tab_id),
        Some(&TabStatus::Dead),
        "a confirmed-dead pty must stay Dead even with a stale pending_prompt"
    );
}

#[test]
fn spawned_by_round_trips_through_snapshot() {
    // state → snapshot → state preserves the hierarchy linkage fields.
    let leaf = SplitNodeSnapshot::Leaf {
        tab_id: "w".into(),
        cwd: None,
        title: Some("Worker".into()),
        description: None,
        title_level: Some("User".into()),
        agent_type: Some("claude".into()),
        agent_type_locked: true,
        claude_session_id: None,
        agent_identity: None,
        role: Some("Worker".into()),
        role_profile: None,
        team_id: None,
        parent_tab_id: Some("orch".into()),
        spawned_by_tab_id: Some("spawner".into()),
        report_target_tab_id: None,
        report_target_mesh_peer: None,
        ownership: None,
        can_spawn: false,
        depth: 2,
        chat_view: Some(false),
        auto_approve: false,
        audience: TabAudience::User,
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
            spawned_by_tab_id,
            report_target_tab_id,
            ..
        } => {
            assert_eq!(spawned_by_tab_id.as_deref(), Some("spawner"));
            assert_eq!(report_target_tab_id.as_deref(), None);
        }
        _ => panic!("expected a leaf"),
    }
}

#[test]
fn requested_and_actual_model_identity_round_trips_through_snapshot() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = state.list_workspaces()[0].id.clone();
    let tab_id = state.create_tab(&wsid).unwrap();
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
        let tab = state.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("codex".into());
        tab.agent_session_model = Some("gpt-5.6-terra".into());
        tab.agent_planned_model = Some("codex/gpt-5.6-luna".into());
        tab.agent_requested_model = Some("gpt-5.6-luna".into());
        tab.agent_provider_resolved_model = Some("gpt-5.6-luna".into());
        tab.model_fallback = Some(fallback.clone());
        tab.model_identity_mismatch = Some(mismatch.clone());
    }

    let snap = state.to_session_snapshot();
    let restored = MultiplexerState::from_session_snapshot(&snap);
    let tab = restored.tabs.get(&tab_id).expect("Tab restored");

    assert_eq!(tab.agent_session_model.as_deref(), Some("gpt-5.6-terra"));
    assert_eq!(
        tab.agent_planned_model.as_deref(),
        Some("codex/gpt-5.6-luna")
    );
    assert_eq!(tab.agent_requested_model.as_deref(), Some("gpt-5.6-luna"));
    assert_eq!(
        tab.agent_provider_resolved_model.as_deref(),
        Some("gpt-5.6-luna")
    );
    assert_eq!(tab.model_fallback, Some(fallback));
    assert_eq!(tab.model_identity_mismatch, Some(mismatch));
}

#[test]
fn provisioned_cwd_round_trips_through_snapshot() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = state.list_workspaces()[0].id.clone();
    let tab_id = state.create_tab(&wsid).unwrap();
    let cwd = state
        .get_tab(&tab_id)
        .and_then(|tab| tab.cwd.clone())
        .expect("tab cwd");
    state.get_tab_mut(&tab_id).unwrap().provisioned_cwd = Some(cwd.clone());

    let restored = MultiplexerState::from_session_snapshot(&state.to_session_snapshot());
    let tab = restored.get_tab(&tab_id).expect("Tab restored");

    assert_eq!(tab.cwd.as_deref(), Some(cwd.as_str()));
    assert_eq!(tab.provisioned_cwd.as_deref(), Some(cwd.as_str()));
}

// ── Ghost-tab repro harness ───────────────────────────────────────────────

/// Repro: a tab marked skip_snapshot (first-run picker or any ephemeral
/// onboarding tab) must be absent from the serialised session snapshot.
/// Without the fix this test fails — the tab IS included, then restores as
/// a ghost after a daemon restart.
#[test]
fn ghost_repro_skip_snapshot_tab_excluded_from_snapshot() {
    let mut state = MultiplexerState::empty();
    let wsid = state.create_workspace("Test".to_string(), None).id;
    let tab_id = state.create_tab(&wsid).unwrap();
    state.get_tab_mut(&tab_id).unwrap().skip_snapshot = true;

    let snap = state.to_session_snapshot();
    assert_eq!(
        snap.workspaces[0].tabs.len(),
        0,
        "skip_snapshot Tab must be excluded from the session snapshot — \
         if it weren't, a daemon restart would restore it as a ghost tab"
    );
}

/// Happy path: a regular (non-ephemeral) tab must survive the round-trip
/// and not be accidentally dropped by the ghost-tab fix.
#[test]
fn ghost_repro_normal_tab_survives_snapshot_round_trip() {
    let mut state = MultiplexerState::empty();
    let wsid = state.create_workspace("Test".to_string(), None).id;
    let tab_id = state.create_tab(&wsid).unwrap();
    state.get_tab_mut(&tab_id).unwrap().title = "My session".to_string();
    // skip_snapshot defaults to false

    let snap = state.to_session_snapshot();
    assert_eq!(
        snap.workspaces[0].tabs.len(),
        1,
        "regular Tab must survive the session snapshot round-trip"
    );

    let restored = MultiplexerState::from_session_snapshot(&snap);
    assert!(
        restored.tabs.contains_key(&tab_id),
        "restored state must include the regular Tab"
    );
}

/// Mixed workspace: only the ephemeral tab is dropped; the real tab survives.
#[test]
fn ghost_repro_mixed_workspace_drops_only_ephemeral() {
    let mut state = MultiplexerState::empty();
    let wsid = state.create_workspace("Test".to_string(), None).id;

    let tab_real = state.create_tab(&wsid).unwrap();
    state.get_tab_mut(&tab_real).unwrap().title = "Real session".to_string();

    let tab_picker = state.create_tab(&wsid).unwrap();
    state.get_tab_mut(&tab_picker).unwrap().skip_snapshot = true;

    let snap = state.to_session_snapshot();
    assert_eq!(
        snap.workspaces[0].tabs.len(),
        1,
        "only the real Tab must survive; the ephemeral picker must be excluded"
    );
    let surviving_leaf = &snap.workspaces[0].tabs[0].split_tree;
    match surviving_leaf {
        crate::session::store::SplitNodeSnapshot::Leaf { tab_id, .. } => {
            assert_eq!(tab_id, &tab_real, "surviving tab must be the real Tab");
        }
        _ => panic!("expected a leaf"),
    }
}

#[test]
fn snapshots_exclude_orphaned_and_duplicate_live_tabs() {
    let mut state = MultiplexerState::empty();
    let workspace_id = state.create_workspace("Clean".to_string(), None).id;
    let tab_id = state.create_tab(&workspace_id).unwrap();
    let workspace = state
        .workspaces
        .iter_mut()
        .find(|workspace| workspace.id == workspace_id)
        .unwrap();
    workspace.tabs.push(tab_id.clone());
    workspace.tabs.push("closed-tab".to_string());
    workspace.active_tab_id = Some("closed-tab".to_string());

    let live = state.snapshot();
    assert_eq!(live.workspaces[0].tabs.len(), 1);
    assert_eq!(live.workspaces[0].tabs[0], tab_id);
    assert_eq!(
        live.workspaces[0].active_tab_id.as_deref(),
        Some(tab_id.as_str())
    );

    let persisted = state.to_session_snapshot();
    assert_eq!(persisted.workspaces[0].tabs.len(), 1);
    assert_eq!(persisted.workspaces[0].tabs[0].id, tab_id);
}
