use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{AgentRole, TabAudience};

#[test]
fn test_session_snapshot_roundtrip() {
    let (mut state, tab_id) = MultiplexerState::new();
    state.link_pty(&tab_id, "pty-1".to_string());
    if let Some(tab) = state.get_tab_mut(&tab_id) {
        tab.cwd = Some("/test/path".to_string());
        tab.agent_type = Some("claude".to_string());
    }
    state
        .set_orchestrator_role(&tab_id)
        .unwrap();

    let _ws2 = state.create_workspace("Second".to_string(), None);

    let snap = state.to_session_snapshot();
    assert_eq!(snap.workspaces.len(), 2);

    let restored = MultiplexerState::from_session_snapshot(&snap);
    let ws_list = restored.list_workspaces();
    assert_eq!(ws_list.len(), 2);

    let tab = restored.get_tab(&tab_id).unwrap();
    assert_eq!(tab.cwd.as_deref(), Some("/test/path"));
    // Restore agent_type even when session_id is missing.
    // Without it, a previously silent tab can reappear as plain powershell after restart.
    assert_eq!(tab.agent_type.as_deref(), Some("claude"));
    assert_eq!(tab.role, Some(AgentRole::Manager));
    assert!(tab.anchors_team(), "orchestrator kind anchors its Team");
    assert_eq!(tab.depth, 1);
    assert!(tab.can_spawn);
    assert!(tab.pty_id.is_none());
}

#[test]
fn test_session_snapshot_preserves_agent_type_with_session_id() {
    // Preserve `agent_type` on snapshot restore.
    // Dropping to default claude prevented codex/opencode sessions from resuming.
    for provider in ["claude", "codex", "opencode"] {
        let (mut state, tab_id) = MultiplexerState::new();
        if let Some(tab) = state.get_tab_mut(&tab_id) {
            tab.cwd = Some("/work".to_string());
            tab.agent_type = Some(provider.to_string());
            tab.agent_session_id = Some(format!("session-{}", provider));
        }

        let snap = state.to_session_snapshot();
        let restored = MultiplexerState::from_session_snapshot(&snap);
        let tab = restored.get_tab(&tab_id).unwrap();

        assert_eq!(
            tab.agent_type.as_deref(),
            Some(provider),
            "agent_type must survive snapshot roundtrip when session_id is present (provider={})",
            provider,
        );
        assert_eq!(
            tab.agent_session_id.as_deref(),
            Some(format!("session-{}", provider).as_str()),
        );
    }
}

#[test]
fn test_session_snapshot_preserves_chat_view() {
    let (mut state, tab_id) = MultiplexerState::new();
    if let Some(tab) = state.get_tab_mut(&tab_id) {
        tab.agent_type = Some("codex".to_string());
        tab.agent_session_id = Some("session-codex".to_string());
        tab.chat_view = true;
    }

    let snap = state.to_session_snapshot();
    let restored = MultiplexerState::from_session_snapshot(&snap);
    let tab = restored.get_tab(&tab_id).unwrap();
    assert!(tab.chat_view, "chat_view must survive snapshot roundtrip");
}

#[test]
fn test_session_snapshot_never_derives_agent_type_from_title() {
    use crate::session::store::{
        SessionSnapshot, SplitNodeSnapshot, TabSnapshot, WorkspaceSnapshot,
    };

    for title in ["codex", "opencode session", "claude", "Codex — /repo"] {
        let snap = SessionSnapshot {
            version: "test".to_string(),
            timestamp: 0,
            active_workspace_id: Some("ws-1".to_string()),
            teams: vec![],
            workspaces: vec![WorkspaceSnapshot {
                id: "ws-1".to_string(),
                name: "W".to_string(),
                active_tab_id: Some("tab-1".to_string()),
                cwd: None,
                protected: false,
                sort_order: 0,
                tabs: vec![TabSnapshot {
                    id: "tab-1".to_string(),
                    title: "Tab".to_string(),
                    description: None,
                    sort_order: 0,
                    split_tree: SplitNodeSnapshot::Leaf {
                        tab_id: "tab-1".to_string(),
                        cwd: None,
                        title: Some(title.to_string()),
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
                    },
                }],
            }],
        };

        let restored = MultiplexerState::from_session_snapshot(&snap);
        let tab = restored.get_tab("tab-1").unwrap();
        assert_eq!(tab.agent_type, None, "title {title:?} must not grant identity");
    }
}

#[test]
fn test_session_snapshot_no_agent_type_for_plain_title() {
    use crate::session::store::{
        SessionSnapshot, SplitNodeSnapshot, TabSnapshot, WorkspaceSnapshot,
    };

    let snap = SessionSnapshot {
        version: "test".to_string(),
        timestamp: 0,
        active_workspace_id: Some("ws-1".to_string()),
        teams: vec![],
        workspaces: vec![WorkspaceSnapshot {
            id: "ws-1".to_string(),
            name: "W".to_string(),
            active_tab_id: Some("tab-1".to_string()),
            cwd: None,
            protected: false,
            sort_order: 0,
            tabs: vec![TabSnapshot {
                id: "tab-1".to_string(),
                title: "Tab".to_string(),
                description: None,
                sort_order: 0,
                split_tree: SplitNodeSnapshot::Leaf {
                    tab_id: "tab-1".to_string(),
                    cwd: None,
                    title: Some("~/repo — zsh".to_string()),
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
                },
            }],
        }],
    };
    let restored = MultiplexerState::from_session_snapshot(&snap);
    assert!(restored.get_tab("tab-1").unwrap().agent_type.is_none());
}

#[test]
fn test_session_snapshot_preserves_agent_type_locked() {
    let (mut state, tab_id) = MultiplexerState::new();
    if let Some(tab) = state.get_tab_mut(&tab_id) {
        tab.agent_type = Some("codex".to_string());
        tab.agent_type_locked = true;
    }
    let snap = state.to_session_snapshot();
    let restored = MultiplexerState::from_session_snapshot(&snap);
    let tab = restored.get_tab(&tab_id).unwrap();
    assert_eq!(tab.agent_type.as_deref(), Some("codex"));
    assert!(tab.agent_type_locked, "locked flag must survive roundtrip");
}

#[test]
fn test_session_snapshot_legacy_agent_type_treated_as_locked() {
    use crate::session::store::{
        SessionSnapshot, SplitNodeSnapshot, TabSnapshot, WorkspaceSnapshot,
    };
    // persisted agent_type via a real spawn or detection, both qualify as explicit intent.
    let snap = SessionSnapshot {
        version: "test".to_string(),
        timestamp: 0,
        active_workspace_id: Some("ws-1".to_string()),
        teams: vec![],
        workspaces: vec![WorkspaceSnapshot {
            id: "ws-1".to_string(),
            name: "W".to_string(),
            active_tab_id: Some("tab-1".to_string()),
            cwd: None,
            protected: false,
            sort_order: 0,
            tabs: vec![TabSnapshot {
                id: "tab-1".to_string(),
                title: "Tab".to_string(),
                description: None,
                sort_order: 0,
                split_tree: SplitNodeSnapshot::Leaf {
                    tab_id: "tab-1".to_string(),
                    cwd: None,
                    title: None,
                    description: None,
                    title_level: None,
                    agent_type: Some("claude".to_string()),
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
                },
            }],
        }],
    };
    let restored = MultiplexerState::from_session_snapshot(&snap);
    let tab = restored.get_tab("tab-1").unwrap();
    assert_eq!(tab.agent_type.as_deref(), Some("claude"));
    assert!(tab.agent_type_locked, "legacy agent_type implies lock");
}

#[test]
fn test_session_snapshot_preserves_hierarchy() {
    let (mut state, cond_id) = MultiplexerState::new();
    let ws_id = state.list_workspaces()[0].id.clone();
    state
        .set_orchestrator_role(&cond_id)
        .unwrap();
    let worker_id = state.create_tab(&ws_id).unwrap();
    state
        .set_tab_role(&worker_id, AgentRole::Worker, Some(&cond_id))
        .unwrap();
    let team_id = state.get_tab(&cond_id).unwrap().team_id.clone().unwrap();
    // A role only survives restore with provider identity behind it, so a
    // hierarchy fixture has to launch both tabs rather than just place them.
    for tab_id in [&cond_id, &worker_id] {
        state.get_tab_mut(tab_id).unwrap().agent_type = Some("claude".into());
    }

    let snap = state.to_session_snapshot();
    let restored = MultiplexerState::from_session_snapshot(&snap);

    let cond = restored.get_tab(&cond_id).unwrap();
    assert_eq!(cond.role, Some(AgentRole::Manager));
    assert_eq!(cond.role_profile.as_deref(), Some("orchestrator"));
    assert!(cond.anchors_team(), "orchestrator kind anchors its Team");
    assert!(cond.parent_tab_id.is_none());
    assert_eq!(cond.depth, 1);
    assert_eq!(cond.team_id.as_deref(), Some(team_id.as_str()));

    let worker = restored.get_tab(&worker_id).unwrap();
    assert_eq!(worker.role, Some(AgentRole::Worker));
    assert_eq!(worker.role_profile, None);
    assert_eq!(worker.parent_tab_id.as_deref(), Some(cond_id.as_str()));
    assert_eq!(worker.depth, 2);
    assert_eq!(worker.team_id.as_deref(), Some(team_id.as_str()));
    assert!(restored.get_team(&team_id).is_some());
}
