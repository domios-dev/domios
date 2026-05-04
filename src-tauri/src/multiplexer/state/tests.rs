use super::*;
use crate::multiplexer::types::{AgentRole, Orientation, PaneType};

#[test]
fn test_new_state_has_default_workspace() {
    let (state, initial_pane_id) = MultiplexerState::new();
    assert_eq!(state.list_workspaces().len(), 1);
    assert!(state.get_pane(&initial_pane_id).is_some());
    assert_eq!(
        state.list_workspaces()[0].name,
        "Main"
    );
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
    assert_eq!(info.tab_count, 1);
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
fn test_split_pane() {
    let (mut state, initial_pane_id) = MultiplexerState::new();
    let result = state.split_pane(&initial_pane_id, Orientation::Horizontal);
    assert!(result.is_ok());

    let new_pane_id = result.unwrap();
    assert!(state.get_pane(&new_pane_id).is_some());

    let ws = state.get_active_workspace().unwrap();
    let tab = &ws.tabs[0];
    let pane_ids = tab.split_tree.collect_pane_ids();
    assert_eq!(pane_ids.len(), 2);
    assert!(pane_ids.contains(&initial_pane_id));
    assert!(pane_ids.contains(&new_pane_id));
}

#[test]
fn test_split_pane_not_found() {
    let (mut state, _) = MultiplexerState::new();
    let result = state.split_pane("nonexistent", Orientation::Horizontal);
    assert!(result.is_err());
}

#[test]
fn test_close_pane_collapses_split() {
    let (mut state, initial_pane_id) = MultiplexerState::new();

    let new_pane_id = state
        .split_pane(&initial_pane_id, Orientation::Horizontal)
        .unwrap();

    let result = state.close_pane(&new_pane_id);
    assert!(result.is_ok());

    let ws = state.get_active_workspace().unwrap();
    let tab = &ws.tabs[0];
    let pane_ids = tab.split_tree.collect_pane_ids();
    assert_eq!(pane_ids.len(), 1);
    assert_eq!(pane_ids[0], initial_pane_id);

    assert!(state.get_pane(&new_pane_id).is_none());
}

#[test]
fn test_close_pane_returns_pty_id() {
    let (mut state, initial_pane_id) = MultiplexerState::new();

    let new_pane_id = state
        .split_pane(&initial_pane_id, Orientation::Horizontal)
        .unwrap();
    state.link_pty(&new_pane_id, "pty-123".to_string());

    let result = state.close_pane(&new_pane_id).unwrap();
    assert_eq!(result, vec!["pty-123".to_string()]);
}

#[test]
fn test_close_pane_clears_focus_when_no_general() {
    // No `general` alias registered → fallback finds nothing → focus drops to None.
    // Matches old behavior so existing callers without General are unaffected.
    let (mut state, initial_pane_id) = MultiplexerState::new();
    let new_pane_id = state
        .split_pane(&initial_pane_id, Orientation::Horizontal)
        .unwrap();

    state.set_focused_pane(&new_pane_id).unwrap();
    assert_eq!(state.get_focused_pane_id(), Some(new_pane_id.as_str()));

    state.close_pane(&new_pane_id).unwrap();
    assert_eq!(state.get_focused_pane_id(), None);
}

#[test]
fn test_close_pane_falls_back_to_general() {
    // With `general` alias registered, closing the focused pane moves focus to
    // the General orchestrator pane instead of leaving the UI deselected.
    let (mut state, general_pane_id) = MultiplexerState::new();
    state.register_alias("general", &general_pane_id);

    let other_pane_id = state
        .split_pane(&general_pane_id, Orientation::Horizontal)
        .unwrap();
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
    let other = state.split_pane(&general_pane_id, Orientation::Horizontal).unwrap();
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
    let new_pane_id = state
        .split_pane(&initial_pane_id, Orientation::Horizontal)
        .unwrap();

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
fn test_nested_splits() {
    let (mut state, initial_pane_id) = MultiplexerState::new();

    let pane_2 = state
        .split_pane(&initial_pane_id, Orientation::Horizontal)
        .unwrap();

    let pane_3 = state
        .split_pane(&pane_2, Orientation::Vertical)
        .unwrap();

    let ws = state.get_active_workspace().unwrap();
    let pane_ids = ws.tabs[0].split_tree.collect_pane_ids();
    assert_eq!(pane_ids.len(), 3);
    assert!(pane_ids.contains(&initial_pane_id));
    assert!(pane_ids.contains(&pane_2));
    assert!(pane_ids.contains(&pane_3));
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
    let worker_id = state
        .split_pane(&orch_id, Orientation::Horizontal)
        .unwrap();
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
    let other_id = state
        .split_pane(&pane_id, Orientation::Horizontal)
        .unwrap();
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

    let worker_id = state
        .split_pane(&orch_id, Orientation::Horizontal)
        .unwrap();
    let result = state.set_pane_role(&worker_id, AgentRole::Worker, Some(&orch_id));
    assert!(result.is_err());
}

#[test]
fn test_subworker_requires_worker_parent() {
    // Lead under Orchestrator, Worker under Lead
    let (mut state, orch_id) = MultiplexerState::new();
    state.set_pane_role(&orch_id, AgentRole::Orchestrator, None).unwrap();

    let lead_id = state.split_pane(&orch_id, Orientation::Horizontal).unwrap();
    state.set_pane_role(&lead_id, AgentRole::Lead, Some(&orch_id)).unwrap();

    let worker_id = state.split_pane(&lead_id, Orientation::Vertical).unwrap();
    state.set_pane_role(&worker_id, AgentRole::Worker, Some(&lead_id)).unwrap();

    let worker = state.get_pane(&worker_id).unwrap();
    assert_eq!(worker.role, Some(AgentRole::Worker));
    assert_eq!(worker.depth, 3);
    assert!(!worker.can_spawn);
}

#[test]
fn test_get_children_ids() {
    let (mut state, orch_id) = MultiplexerState::new();
    state
        .set_pane_role(&orch_id, AgentRole::Orchestrator, None)
        .unwrap();
    let w1 = state
        .split_pane(&orch_id, Orientation::Horizontal)
        .unwrap();
    state
        .set_pane_role(&w1, AgentRole::Worker, Some(&orch_id))
        .unwrap();
    let w2 = state
        .split_pane(&orch_id, Orientation::Horizontal)
        .unwrap();
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
    let worker_id = state
        .split_pane(&orch_id, Orientation::Horizontal)
        .unwrap();
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
    state
        .set_pane_role(&orch_id, AgentRole::Orchestrator, None)
        .unwrap();
    state.link_pty(&orch_id, "pty-orch".to_string());

    let worker_id = state
        .split_pane(&orch_id, Orientation::Horizontal)
        .unwrap();
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
    assert_eq!(state.get_pane(&pane_id).unwrap().title_level, TitleLevel::Default);

    state.rename_pane(&pane_id, "Custom").unwrap();
    let pane = state.get_pane(&pane_id).unwrap();
    assert_eq!(pane.title, "Custom");
    assert_eq!(pane.title_level, TitleLevel::User);
}

#[test]
fn test_rename_pane_updates_tab_title() {
    let (mut state, pane_id) = MultiplexerState::new();
    state.rename_pane(&pane_id, "My Tab").unwrap();

    let ws = state.get_active_workspace().unwrap();
    assert_eq!(ws.tabs[0].title, "My Tab");
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
    // agent_type is dropped when no session_id was saved — without resume
    // there's no agent to attribute, so re-detect from PTY output instead.
    assert_eq!(pane.agent_type, None);
    assert_eq!(pane.role, Some(AgentRole::Orchestrator));
    assert_eq!(pane.depth, 1);
    assert!(pane.can_spawn);
    assert!(pane.pty_id.is_none());
}

#[test]
fn test_session_snapshot_preserves_agent_type_with_session_id() {
    // Regression: codex/opencode panes silently failed to resume on app
    // restart because agent_type was always dropped, so the resume launcher
    // fell back to default_agent_provider (claude) and tried to claude-resume
    // a non-claude session uuid → session_exists() bailed → resume cleared.
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
fn test_session_snapshot_preserves_hierarchy() {
    let (mut state, cond_id) = MultiplexerState::new();
    state
        .set_pane_role(&cond_id, AgentRole::Orchestrator, None)
        .unwrap();
    let worker_id = state
        .split_pane(&cond_id, Orientation::Horizontal)
        .unwrap();
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
fn test_browser_pane() {
    let (mut state, _) = MultiplexerState::new();
    let pane_id = state.create_browser_pane(Some("https://example.com".to_string()));
    let pane = state.get_pane(&pane_id).unwrap();
    assert_eq!(pane.pane_type, PaneType::Browser);
    assert_eq!(pane.url.as_deref(), Some("https://example.com"));
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
    let new_pane_id = state.split_pane(&initial_pane_id, Orientation::Horizontal).unwrap();
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
    let (mut state, initial_pane_id) = MultiplexerState::new();
    let new_pane_id = state.split_pane(&initial_pane_id, Orientation::Horizontal).unwrap();
    state.register_alias("ephemeral", &new_pane_id);

    state.close_pane(&new_pane_id).unwrap();

    assert_eq!(state.resolve_pane_id("ephemeral"), None);
}
