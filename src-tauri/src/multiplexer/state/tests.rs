use super::*;
use crate::multiplexer::types::{AgentRole, Orientation, PaneType};

#[test]
fn test_new_state_has_default_workspace() {
    let (state, initial_pane_id) = MultiplexerState::new();
    assert_eq!(state.list_workspaces().len(), 1);
    assert!(state.get_pane(&initial_pane_id).is_some());
    assert_eq!(state.list_workspaces()[0].name, "Main");
    assert!(state.list_workspaces()[0].is_active);
}

#[test]
fn test_new_state_has_focused_pane() {
    let (state, initial_pane_id) = MultiplexerState::new();
    assert_eq!(state.get_focused_pane_id(), Some(initial_pane_id.as_str()));
}

#[test]
fn test_new_state_pane_is_terminal() {
    let (state, initial_pane_id) = MultiplexerState::new();
    let pane = state.get_pane(&initial_pane_id).unwrap();
    assert_eq!(pane.pane_type, PaneType::Terminal);
    assert!(pane.pty_id.is_none());
}

#[test]
fn test_create_workspace() {
    let (mut state, _) = MultiplexerState::new();
    let info = state.create_workspace("Work".to_string(), None);
    assert_eq!(info.name, "Work");
    assert_eq!(info.tab_count, 0);
    assert!(!info.is_active);
    assert_eq!(state.list_workspaces().len(), 2);
}

#[test]
fn test_set_active_workspace() {
    let (mut state, _) = MultiplexerState::new();
    let info = state.create_workspace("Work".to_string(), None);
    assert!(state.set_active_workspace(&info.id).is_ok());

    let workspaces = state.list_workspaces();
    let active = workspaces.iter().find(|w| w.is_active).unwrap();
    assert_eq!(active.id, info.id);
}

#[test]
fn test_set_active_workspace_not_found() {
    let (mut state, _) = MultiplexerState::new();
    let result = state.set_active_workspace("nonexistent");
    assert!(result.is_err());
}

#[test]
fn test_create_tab() {
    let (mut state, _) = MultiplexerState::new();
    let ws_id = state.list_workspaces()[0].id.clone();

    let result = state.create_tab(&ws_id);
    assert!(result.is_ok());

    let (tab_id, pane_id) = result.unwrap();
    assert!(!tab_id.is_empty());
    assert!(!pane_id.is_empty());
    assert!(state.get_pane(&pane_id).is_some());

    let ws = state.get_workspace(&ws_id).unwrap();
    assert_eq!(ws.tabs.len(), 2);
    assert!(ws.active_tab_id.is_some());
}

#[test]
fn test_create_tab_invalid_workspace() {
    let (mut state, _) = MultiplexerState::new();
    let result = state.create_tab("nonexistent");
    assert!(result.is_err());
}

#[test]
fn test_create_tab_generates_unique_titles() {
    let (mut state, _) = MultiplexerState::new();
    let ws_id = state.list_workspaces()[0].id.clone();

    for _ in 0..20 {
        state.create_tab(&ws_id).unwrap();
    }

    let ws = state.get_workspace(&ws_id).unwrap();
    let mut titles = std::collections::HashSet::new();
    for tab in &ws.tabs {
        assert_ne!(tab.title, "Terminal");
        assert!(
            titles.insert(tab.title.as_str()),
            "duplicate tab title: {}",
            tab.title
        );
    }
}

// split_pane removed: each tab owns one pane.
// Regressions are covered by close_tab-focused tests below.

#[test]
fn test_close_pane_preserves_active_tab_when_unrelated_single_leaf_tab_closes() {
    // Probe tabs are single-leaf; closing them must keep the current active tab, not switch to ws.tabs.last().
    let (mut state, _) = MultiplexerState::new();
    let ws_id = state.list_workspaces()[0].id.clone();
    let (tab2_id, _) = state.create_tab(&ws_id).unwrap();
    let (_tab3_id, tab3_pane_id) = state.create_tab(&ws_id).unwrap();

    // User parks on tab 2 (the middle one). tab 3 is a hidden probe tab.
    state.set_active_tab(&ws_id, &tab2_id).unwrap();
    assert_eq!(
        state
            .get_workspace(&ws_id)
            .and_then(|w| w.active_tab_id.clone()),
        Some(tab2_id.clone())
    );

    // Probe finishes → its single-leaf tab gets closed via close_pane.
    state.close_pane(&tab3_pane_id).unwrap();

    assert_eq!(
        state
            .get_workspace(&ws_id)
            .and_then(|w| w.active_tab_id.clone()),
        Some(tab2_id),
        "Closing an unrelated single-leaf tab must not move active_tab_id"
    );
}

#[test]
fn test_close_pane_returns_pty_id() {
    let (mut state, _initial_pane_id) = MultiplexerState::new();
    let ws_id = state.list_workspaces()[0].id.clone();
    let (_tab, new_pane_id) = state.create_tab(&ws_id).unwrap();
    state.link_pty(&new_pane_id, "pty-123".to_string());

    let result = state.close_pane(&new_pane_id).unwrap();
    assert_eq!(result, vec!["pty-123".to_string()]);
}

#[test]
fn test_close_pane_clears_focus_when_no_general() {
    // No general alias -> focus becomes None after closing the pane.
    // Preserves behavior for callers that still rely on this fallback.
    let (mut state, _initial_pane_id) = MultiplexerState::new();
    let ws_id = state.list_workspaces()[0].id.clone();
    let (_tab, new_pane_id) = state.create_tab(&ws_id).unwrap();

    state.set_focused_pane(&new_pane_id).unwrap();
    assert_eq!(state.get_focused_pane_id(), Some(new_pane_id.as_str()));

    state.close_pane(&new_pane_id).unwrap();
    assert_eq!(state.get_focused_pane_id(), None);
}

#[test]
fn test_close_pane_falls_back_to_general() {
    // With `general` alias registered, closing the focused pane moves focus to the General orchestrator pane instead of leaving the UI deselected.
    let (mut state, general_pane_id) = MultiplexerState::new();
    state.register_alias("general", &general_pane_id);

    let ws_id = state.list_workspaces()[0].id.clone();
    let (_tab, other_pane_id) = state.create_tab(&ws_id).unwrap();
    state.set_focused_pane(&other_pane_id).unwrap();

    state.close_pane(&other_pane_id).unwrap();
    assert_eq!(state.get_focused_pane_id(), Some(general_pane_id.as_str()));
}

#[test]
fn test_close_tab_falls_back_to_general() {
    let (mut state, general_pane_id) = MultiplexerState::new();
    state.register_alias("general", &general_pane_id);

    let ws_id = state.list_workspaces()[0].id.clone();
    let (tab2_id, tab2_pane_id) = state.create_tab(&ws_id).unwrap();
    state.set_focused_pane(&tab2_pane_id).unwrap();

    state.close_tab(&ws_id, &tab2_id).unwrap();
    assert_eq!(state.get_focused_pane_id(), Some(general_pane_id.as_str()));
}

#[test]
fn test_close_workspace_falls_back_to_general() {
    let (mut state, general_pane_id) = MultiplexerState::new();
    state.register_alias("general", &general_pane_id);

    let other_ws = state.create_workspace("Other".to_string(), None);
    let (_tab_id, other_pane_id) = state.create_tab(&other_ws.id).unwrap();
    state.set_focused_pane(&other_pane_id).unwrap();

    state.close_workspace(&other_ws.id).unwrap();
    assert_eq!(state.get_focused_pane_id(), Some(general_pane_id.as_str()));
}

#[test]
fn test_fallback_focus_to_general_no_op_without_alias() {
    let (mut state, pane_id) = MultiplexerState::new();
    state.set_focused_pane(&pane_id).unwrap();

    let result = state.fallback_focus_to_general();
    assert_eq!(result, None);
    // Focus left untouched (preferable to silent null).
    assert_eq!(state.get_focused_pane_id(), Some(pane_id.as_str()));
}

#[test]
fn test_fallback_focus_to_general_sets_focus() {
    let (mut state, general_pane_id) = MultiplexerState::new();
    state.register_alias("general", &general_pane_id);

    // Clear focus first to make the assignment observable.
    let other = state.create_pane();
    state.set_focused_pane(&other).unwrap();

    let result = state.fallback_focus_to_general();
    assert_eq!(result.as_deref(), Some(general_pane_id.as_str()));
    assert_eq!(state.get_focused_pane_id(), Some(general_pane_id.as_str()));
}

#[test]
fn test_close_workspace_returns_pty_ids() {
    let (mut state, initial_pane_id) = MultiplexerState::new();
    state.link_pty(&initial_pane_id, "pty-abc".to_string());

    let ws_id = state.list_workspaces()[0].id.clone();
    let pty_ids = state.close_workspace(&ws_id).unwrap();
    assert_eq!(pty_ids, vec!["pty-abc"]);
    assert_eq!(state.list_workspaces().len(), 0);
}

#[test]
fn test_close_workspace_switches_active() {
    let (mut state, _) = MultiplexerState::new();
    let ws2 = state.create_workspace("Second".to_string(), None);
    let ws1_id = state.list_workspaces()[0].id.clone();

    state.close_workspace(&ws1_id).unwrap();

    let workspaces = state.list_workspaces();
    assert_eq!(workspaces.len(), 1);
    assert_eq!(workspaces[0].id, ws2.id);
    assert!(workspaces[0].is_active);
}

#[test]
fn test_close_workspace_not_found() {
    let (mut state, _) = MultiplexerState::new();
    let result = state.close_workspace("nonexistent");
    assert!(result.is_err());
}

#[test]
fn test_close_tab() {
    let (mut state, _) = MultiplexerState::new();
    let ws_id = state.list_workspaces()[0].id.clone();
    let (tab2_id, tab2_pane_id) = state.create_tab(&ws_id).unwrap();

    state.link_pty(&tab2_pane_id, "pty-tab2".to_string());

    let pty_ids = state.close_tab(&ws_id, &tab2_id).unwrap();
    assert_eq!(pty_ids, vec!["pty-tab2"]);

    let ws = state.get_workspace(&ws_id).unwrap();
    assert_eq!(ws.tabs.len(), 1);
    assert!(state.get_pane(&tab2_pane_id).is_none());
}

#[test]
fn test_set_focused_pane() {
    let (mut state, initial_pane_id) = MultiplexerState::new();
    let new_pane_id = state.create_pane();

    assert!(state.set_focused_pane(&new_pane_id).is_ok());
    assert_eq!(state.get_focused_pane_id(), Some(new_pane_id.as_str()));
}

#[test]
fn test_set_focused_pane_not_found() {
    let (mut state, _) = MultiplexerState::new();
    let result = state.set_focused_pane("nonexistent");
    assert!(result.is_err());
}

#[test]
fn test_link_pty() {
    let (mut state, initial_pane_id) = MultiplexerState::new();
    state.link_pty(&initial_pane_id, "pty-xyz".to_string());

    let pane = state.get_pane(&initial_pane_id).unwrap();
    assert_eq!(pane.pty_id, Some("pty-xyz".to_string()));
}

#[test]
fn test_snapshot() {
    let (state, initial_pane_id) = MultiplexerState::new();
    let snap = state.snapshot();
    assert_eq!(snap.workspaces.len(), 1);
    assert!(snap.panes.contains_key(&initial_pane_id));
    assert!(snap.active_workspace_id.is_some());
    assert_eq!(snap.focused_pane_id, Some(initial_pane_id));
}

#[test]
fn test_get_pane_mut() {
    let (mut state, initial_pane_id) = MultiplexerState::new();
    {
        let pane = state.get_pane_mut(&initial_pane_id).unwrap();
        pane.title = "Custom Title".to_string();
    }
    let pane = state.get_pane(&initial_pane_id).unwrap();
    assert_eq!(pane.title, "Custom Title");
}

#[test]
fn test_set_orchestrator_role() {
    let (mut state, pane_id) = MultiplexerState::new();
    state
        .set_pane_role(&pane_id, AgentRole::Orchestrator, None)
        .unwrap();
    let pane = state.get_pane(&pane_id).unwrap();
    assert_eq!(pane.role, Some(AgentRole::Orchestrator));
    assert_eq!(pane.depth, 1);
    assert!(pane.can_spawn);
    assert!(pane.parent_pane_id.is_none());
}

#[test]
fn test_orchestrator_cannot_have_parent() {
    let (mut state, pane_id) = MultiplexerState::new();
    let result = state.set_pane_role(&pane_id, AgentRole::Orchestrator, Some("fake"));
    assert!(result.is_err());
}

#[test]
fn test_worker_requires_orchestrator_parent() {
    let (mut state, orch_id) = MultiplexerState::new();
    state
        .set_pane_role(&orch_id, AgentRole::Orchestrator, None)
        .unwrap();
    let worker_id = state.create_pane();
    state
        .set_pane_role(&worker_id, AgentRole::Worker, Some(&orch_id))
        .unwrap();

    let worker = state.get_pane(&worker_id).unwrap();
    assert_eq!(worker.role, Some(AgentRole::Worker));
    assert_eq!(worker.depth, 2);
    assert!(!worker.can_spawn);
    assert_eq!(worker.parent_pane_id.as_deref(), Some(orch_id.as_str()));
}

#[test]
fn test_worker_parent_must_be_orchestrator() {
    let (mut state, pane_id) = MultiplexerState::new();
    let other_id = state.create_pane();
    let result = state.set_pane_role(&pane_id, AgentRole::Worker, Some(&other_id));
    assert!(result.is_err());
}

#[test]
fn test_worker_parent_must_allow_spawn() {
    let (mut state, orch_id) = MultiplexerState::new();
    state
        .set_pane_role(&orch_id, AgentRole::Orchestrator, None)
        .unwrap();
    state.set_can_spawn(&orch_id, false).unwrap();

    let worker_id = state.create_pane();
    let result = state.set_pane_role(&worker_id, AgentRole::Worker, Some(&orch_id));
    assert!(result.is_err());
}

#[test]
fn worker_under_orchestrator_is_depth_2_and_cannot_parent() {
    // 2-level hierarchy: a Worker sits directly under the Orchestrator, and there is no third level — a Worker can't parent another Worker.
    let (mut state, orch_id) = MultiplexerState::new();
    state
        .set_pane_role(&orch_id, AgentRole::Orchestrator, None)
        .unwrap();

    let worker_id = state.create_pane();
    state
        .set_pane_role(&worker_id, AgentRole::Worker, Some(&orch_id))
        .unwrap();

    let worker = state.get_pane(&worker_id).unwrap();
    assert_eq!(worker.role, Some(AgentRole::Worker));
    assert_eq!(worker.depth, 2);
    assert!(!worker.can_spawn);

    let sub_id = state.create_pane();
    let result = state.set_pane_role(&sub_id, AgentRole::Worker, Some(&worker_id));
    assert!(
        result.is_err(),
        "a Worker must not be allowed to parent another Worker"
    );
}

#[test]
fn test_get_children_ids() {
    let (mut state, orch_id) = MultiplexerState::new();
    state
        .set_pane_role(&orch_id, AgentRole::Orchestrator, None)
        .unwrap();
    let w1 = state.create_pane();
    state
        .set_pane_role(&w1, AgentRole::Worker, Some(&orch_id))
        .unwrap();
    let w2 = state.create_pane();
    state
        .set_pane_role(&w2, AgentRole::Worker, Some(&orch_id))
        .unwrap();

    let children = state.get_children_ids(&orch_id);
    assert_eq!(children.len(), 2);
    assert!(children.contains(&w1));
    assert!(children.contains(&w2));
}

#[test]
fn test_clear_role_orphans_children() {
    let (mut state, orch_id) = MultiplexerState::new();
    state
        .set_pane_role(&orch_id, AgentRole::Orchestrator, None)
        .unwrap();
    let worker_id = state.create_pane();
    state
        .set_pane_role(&worker_id, AgentRole::Worker, Some(&orch_id))
        .unwrap();

    state.clear_pane_role(&orch_id).unwrap();

    let orch = state.get_pane(&orch_id).unwrap();
    assert!(orch.role.is_none());
    assert_eq!(orch.depth, 0);

    let worker = state.get_pane(&worker_id).unwrap();
    assert!(worker.role.is_none());
    assert!(worker.parent_pane_id.is_none());
    assert_eq!(worker.depth, 0);
}

#[test]
fn test_close_pane_cascades_children() {
    let (mut state, orch_id) = MultiplexerState::new();
    let ws_id = state.list_workspaces()[0].id.clone();
    state
        .set_pane_role(&orch_id, AgentRole::Orchestrator, None)
        .unwrap();
    state.link_pty(&orch_id, "pty-orch".to_string());

    let (_tab, worker_id) = state.create_tab(&ws_id).unwrap();
    state
        .set_pane_role(&worker_id, AgentRole::Worker, Some(&orch_id))
        .unwrap();
    state.link_pty(&worker_id, "pty-worker".to_string());

    let pty_ids = state.close_pane(&orch_id).unwrap();
    assert!(pty_ids.contains(&"pty-orch".to_string()));
    assert!(pty_ids.contains(&"pty-worker".to_string()));
    assert!(state.get_pane(&orch_id).is_none());
    assert!(state.get_pane(&worker_id).is_none());
}

#[test]
fn test_set_can_spawn_requires_role() {
    let (mut state, _) = MultiplexerState::new();
    let pane_id = state.create_pane();
    let result = state.set_can_spawn(&pane_id, true);
    assert!(result.is_err());
}

#[test]
fn test_set_can_spawn_on_orchestrator() {
    let (mut state, pane_id) = MultiplexerState::new();
    state
        .set_pane_role(&pane_id, AgentRole::Orchestrator, None)
        .unwrap();
    assert!(state.get_pane(&pane_id).unwrap().can_spawn);

    state.set_can_spawn(&pane_id, false).unwrap();
    assert!(!state.get_pane(&pane_id).unwrap().can_spawn);

    state.set_can_spawn(&pane_id, true).unwrap();
    assert!(state.get_pane(&pane_id).unwrap().can_spawn);
}

#[test]
fn test_rename_pane_pins_title() {
    let (mut state, pane_id) = MultiplexerState::new();
    assert_eq!(
        state.get_pane(&pane_id).unwrap().title_level,
        TitleLevel::Default
    );

    state.rename_pane(&pane_id, "Custom").unwrap();
    let pane = state.get_pane(&pane_id).unwrap();
    assert_eq!(pane.title, "Custom");
    assert_eq!(pane.title_level, TitleLevel::User);
}

#[test]
fn test_rename_pane_does_not_change_tab_title() {
    let (mut state, pane_id) = MultiplexerState::new();
    let ws_id = state.list_workspaces()[0].id.clone();
    let original = state.get_workspace(&ws_id).unwrap().tabs[0].title.clone();

    state.rename_pane(&pane_id, "My Pane").unwrap();

    let ws = state.get_workspace(&ws_id).unwrap();
    assert_eq!(
        ws.tabs[0].title, original,
        "tab title must be stable across pane renames"
    );
}

#[test]
fn test_rename_tab_changes_title() {
    let (mut state, _) = MultiplexerState::new();
    let ws_id = state.list_workspaces()[0].id.clone();
    let tab_id = state.get_workspace(&ws_id).unwrap().tabs[0].id.clone();

    state.rename_tab(&ws_id, &tab_id, "Renamed").unwrap();

    let ws = state.get_workspace(&ws_id).unwrap();
    assert_eq!(ws.tabs[0].title, "Renamed");
}

#[test]
fn test_move_tab() {
    let (mut state, _) = MultiplexerState::new();
    let ws1_id = state.list_workspaces()[0].id.clone();
    let ws2 = state.create_workspace("Second".to_string(), None);

    let (tab2_id, _) = state.create_tab(&ws1_id).unwrap();

    state.move_tab(&tab2_id, &ws1_id, &ws2.id).unwrap();

    let ws1 = state.get_workspace(&ws1_id).unwrap();
    let ws2 = state.get_workspace(&ws2.id).unwrap();
    assert_eq!(ws1.tabs.len(), 1);
    assert_eq!(ws2.tabs.len(), 1);
}

#[test]
fn test_move_last_tab_fails() {
    let (mut state, _) = MultiplexerState::new();
    let ws1_id = state.list_workspaces()[0].id.clone();
    let ws2 = state.create_workspace("Second".to_string(), None);
    let tab_id = state.get_workspace(&ws1_id).unwrap().tabs[0].id.clone();

    let result = state.move_tab(&tab_id, &ws1_id, &ws2.id);
    assert!(result.is_err());
}

#[test]
fn test_session_snapshot_roundtrip() {
    let (mut state, pane_id) = MultiplexerState::new();
    state.link_pty(&pane_id, "pty-1".to_string());
    if let Some(pane) = state.get_pane_mut(&pane_id) {
        pane.cwd = Some("/test/path".to_string());
        pane.agent_type = Some("claude".to_string());
    }
    state
        .set_pane_role(&pane_id, AgentRole::Orchestrator, None)
        .unwrap();

    let _ws2 = state.create_workspace("Second".to_string(), None);

    let snap = state.to_session_snapshot();
    assert_eq!(snap.workspaces.len(), 2);

    let restored = MultiplexerState::from_session_snapshot(&snap);
    let ws_list = restored.list_workspaces();
    assert_eq!(ws_list.len(), 2);

    let pane = restored.get_pane(&pane_id).unwrap();
    assert_eq!(pane.cwd.as_deref(), Some("/test/path"));
    // Restore agent_type even when session_id is missing.
    // Without it, a previously silent pane can reappear as plain powershell after restart.
    assert_eq!(pane.agent_type.as_deref(), Some("claude"));
    assert_eq!(pane.role, Some(AgentRole::Orchestrator));
    assert_eq!(pane.depth, 1);
    assert!(pane.can_spawn);
    assert!(pane.pty_id.is_none());
}

#[test]
fn test_session_snapshot_preserves_agent_type_with_session_id() {
    // Preserve `agent_type` on snapshot restore.
    // Dropping to default claude prevented codex/opencode sessions from resuming.
    for provider in ["claude", "codex", "opencode"] {
        let (mut state, pane_id) = MultiplexerState::new();
        if let Some(pane) = state.get_pane_mut(&pane_id) {
            pane.cwd = Some("/work".to_string());
            pane.agent_type = Some(provider.to_string());
            pane.agent_session_id = Some(format!("session-{}", provider));
        }

        let snap = state.to_session_snapshot();
        let restored = MultiplexerState::from_session_snapshot(&snap);
        let pane = restored.get_pane(&pane_id).unwrap();

        assert_eq!(
            pane.agent_type.as_deref(),
            Some(provider),
            "agent_type must survive snapshot roundtrip when session_id is present (provider={})",
            provider,
        );
        assert_eq!(
            pane.agent_session_id.as_deref(),
            Some(format!("session-{}", provider).as_str()),
        );
    }
}

#[test]
fn test_session_snapshot_preserves_chat_view() {
    let (mut state, pane_id) = MultiplexerState::new();
    if let Some(pane) = state.get_pane_mut(&pane_id) {
        pane.agent_type = Some("codex".to_string());
        pane.agent_session_id = Some("session-codex".to_string());
        pane.chat_view = true;
    }

    let snap = state.to_session_snapshot();
    let restored = MultiplexerState::from_session_snapshot(&snap);
    let pane = restored.get_pane(&pane_id).unwrap();
    assert!(pane.chat_view, "chat_view must survive snapshot roundtrip");
}

#[test]
fn test_session_snapshot_derives_agent_type_from_title_when_missing() {
    use crate::session::store::{
        SessionSnapshot, SplitNodeSnapshot, TabSnapshot, WorkspaceSnapshot,
    };

    // Legacy snapshot path: agent_type is None (saved before reliable tracking), but the title preserved when the snapshot was captured carries
    // the provider name. Restore must recover agent_type so tab-bar icons show.
    for (title, expected) in [
        ("codex", "codex"),
        ("opencode session", "opencode"),
        ("claude", "claude"),
        ("Codex — /repo", "codex"),
    ] {
        let snap = SessionSnapshot {
            version: "test".to_string(),
            timestamp: 0,
            active_workspace_id: Some("ws-1".to_string()),
            workspaces: vec![WorkspaceSnapshot {
                id: "ws-1".to_string(),
                name: "W".to_string(),
                active_tab_id: Some("tab-1".to_string()),
                cwd: None,
                protected: false,
                tabs: vec![TabSnapshot {
                    id: "tab-1".to_string(),
                    title: "Tab".to_string(),
                    split_tree: SplitNodeSnapshot::Leaf {
                        pane_id: "pane-1".to_string(),
                        cwd: None,
                        title: Some(title.to_string()),
                        title_level: None,
                        agent_type: None,
                        agent_type_locked: false,
                        claude_session_id: None,
                        role: None,
                        parent_pane_id: None,
                        can_spawn: false,
                        depth: 0,
                        chat_view: None,
                        auto_approve: false,
                        agent_session_model: None,
                        chatbackend_plugin_id: None,
                        chatbackend_session_id: None,
                    },
                }],
            }],
        };

        let restored = MultiplexerState::from_session_snapshot(&snap);
        let pane = restored.get_pane("pane-1").unwrap();
        assert_eq!(
            pane.agent_type.as_deref(),
            Some(expected),
            "title {:?} should derive agent_type {:?}",
            title,
            expected,
        );
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
        workspaces: vec![WorkspaceSnapshot {
            id: "ws-1".to_string(),
            name: "W".to_string(),
            active_tab_id: Some("tab-1".to_string()),
            cwd: None,
            protected: false,
            tabs: vec![TabSnapshot {
                id: "tab-1".to_string(),
                title: "Tab".to_string(),
                split_tree: SplitNodeSnapshot::Leaf {
                    pane_id: "pane-1".to_string(),
                    cwd: None,
                    title: Some("~/repo — zsh".to_string()),
                    title_level: None,
                    agent_type: None,
                    agent_type_locked: false,
                    claude_session_id: None,
                    role: None,
                    parent_pane_id: None,
                    can_spawn: false,
                    depth: 0,
                    chat_view: None,
                    auto_approve: false,
                    agent_session_model: None,
                    chatbackend_plugin_id: None,
                    chatbackend_session_id: None,
                },
            }],
        }],
    };
    let restored = MultiplexerState::from_session_snapshot(&snap);
    assert!(restored.get_pane("pane-1").unwrap().agent_type.is_none());
}

#[test]
fn test_session_snapshot_preserves_agent_type_locked() {
    let (mut state, pane_id) = MultiplexerState::new();
    if let Some(pane) = state.get_pane_mut(&pane_id) {
        pane.agent_type = Some("codex".to_string());
        pane.agent_type_locked = true;
    }
    let snap = state.to_session_snapshot();
    let restored = MultiplexerState::from_session_snapshot(&snap);
    let pane = restored.get_pane(&pane_id).unwrap();
    assert_eq!(pane.agent_type.as_deref(), Some("codex"));
    assert!(pane.agent_type_locked, "locked flag must survive roundtrip");
}

#[test]
fn test_session_snapshot_legacy_agent_type_treated_as_locked() {
    use crate::session::store::{
        SessionSnapshot, SplitNodeSnapshot, TabSnapshot, WorkspaceSnapshot,
    };
    // Snapshot saved BEFORE agent_type_locked existed: only agent_type populated. Restore code must treat it as locked — pre-lock snapshots only
    // persisted agent_type via a real spawn or detection, both qualify as explicit intent.
    let snap = SessionSnapshot {
        version: "test".to_string(),
        timestamp: 0,
        active_workspace_id: Some("ws-1".to_string()),
        workspaces: vec![WorkspaceSnapshot {
            id: "ws-1".to_string(),
            name: "W".to_string(),
            active_tab_id: Some("tab-1".to_string()),
            cwd: None,
            protected: false,
            tabs: vec![TabSnapshot {
                id: "tab-1".to_string(),
                title: "Tab".to_string(),
                split_tree: SplitNodeSnapshot::Leaf {
                    pane_id: "pane-1".to_string(),
                    cwd: None,
                    title: None,
                    title_level: None,
                    agent_type: Some("claude".to_string()),
                    agent_type_locked: false,
                    claude_session_id: None,
                    role: None,
                    parent_pane_id: None,
                    can_spawn: false,
                    depth: 0,
                    chat_view: None,
                    auto_approve: false,
                    agent_session_model: None,
                    chatbackend_plugin_id: None,
                    chatbackend_session_id: None,
                },
            }],
        }],
    };
    let restored = MultiplexerState::from_session_snapshot(&snap);
    let pane = restored.get_pane("pane-1").unwrap();
    assert_eq!(pane.agent_type.as_deref(), Some("claude"));
    assert!(pane.agent_type_locked, "legacy agent_type implies lock");
}

#[test]
fn test_session_snapshot_preserves_hierarchy() {
    let (mut state, cond_id) = MultiplexerState::new();
    let ws_id = state.list_workspaces()[0].id.clone();
    state
        .set_pane_role(&cond_id, AgentRole::Orchestrator, None)
        .unwrap();
    let (_tab, worker_id) = state.create_tab(&ws_id).unwrap();
    state
        .set_pane_role(&worker_id, AgentRole::Worker, Some(&cond_id))
        .unwrap();

    let snap = state.to_session_snapshot();
    let restored = MultiplexerState::from_session_snapshot(&snap);

    let cond = restored.get_pane(&cond_id).unwrap();
    assert_eq!(cond.role, Some(AgentRole::Orchestrator));

    let worker = restored.get_pane(&worker_id).unwrap();
    assert_eq!(worker.role, Some(AgentRole::Worker));
    assert_eq!(worker.parent_pane_id.as_deref(), Some(cond_id.as_str()));
    assert_eq!(worker.depth, 2);
}

#[test]
fn test_set_active_tab() {
    let (mut state, _) = MultiplexerState::new();
    let ws_id = state.list_workspaces()[0].id.clone();
    let ws = state.get_workspace(&ws_id).unwrap();
    let tab1_id = ws.tabs[0].id.clone();

    let (tab2_id, _) = state.create_tab(&ws_id).unwrap();

    assert!(state.set_active_tab(&ws_id, &tab1_id).is_ok());
    let ws = state.get_workspace(&ws_id).unwrap();
    assert_eq!(ws.active_tab_id, Some(tab1_id));

    assert!(state.set_active_tab(&ws_id, "nonexistent").is_err());
    assert!(state.set_active_tab("nonexistent", &tab2_id).is_err());
}

#[test]
fn test_protected_workspace_cannot_close() {
    let (mut state, _) = MultiplexerState::new();
    let ws_id = state.list_workspaces()[0].id.clone();
    let ws = state.workspaces.iter_mut().find(|w| w.id == ws_id).unwrap();
    ws.protected = true;

    let result = state.close_workspace(&ws_id);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "Cannot close protected workspace");
    assert_eq!(state.list_workspaces().len(), 1);
}

#[test]
fn test_protected_pane_cannot_close() {
    let (mut state, initial_pane_id) = MultiplexerState::new();
    let new_pane_id = state.create_pane();
    state.get_pane_mut(&new_pane_id).unwrap().protected = true;

    let result = state.close_pane(&new_pane_id);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "Cannot close protected pane");
    assert!(state.get_pane(&new_pane_id).is_some());
}

#[test]
fn test_alias_register_and_resolve() {
    let (mut state, pane_id) = MultiplexerState::new();
    state.register_alias("main-terminal", &pane_id);

    let resolved = state.resolve_pane_id("main-terminal");
    assert_eq!(resolved, Some(pane_id.clone()));
}

#[test]
fn test_alias_not_found_falls_back_to_id() {
    let (state, pane_id) = MultiplexerState::new();
    let resolved = state.resolve_pane_id(&pane_id);
    assert_eq!(resolved, Some(pane_id));
}

#[test]
fn test_resolve_nonexistent_returns_none() {
    let (state, _) = MultiplexerState::new();
    let resolved = state.resolve_pane_id("does-not-exist");
    assert_eq!(resolved, None);
}

#[test]
fn test_alias_unregister() {
    let (mut state, pane_id) = MultiplexerState::new();
    state.register_alias("my-pane", &pane_id);
    assert!(state.resolve_pane_id("my-pane").is_some());

    state.unregister_alias("my-pane");
    assert_eq!(state.resolve_pane_id("my-pane"), None);
}

#[test]
fn test_alias_to_deleted_pane_returns_none() {
    let (mut state, _initial_pane_id) = MultiplexerState::new();
    let ws_id = state.list_workspaces()[0].id.clone();
    let (_tab, new_pane_id) = state.create_tab(&ws_id).unwrap();
    state.register_alias("ephemeral", &new_pane_id);

    state.close_pane(&new_pane_id).unwrap();

    assert_eq!(state.resolve_pane_id("ephemeral"), None);
}
