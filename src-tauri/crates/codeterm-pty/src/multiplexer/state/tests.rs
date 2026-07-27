use super::*;
use crate::multiplexer::types::{AgentRole, TabBacking, TabType};

#[test]
fn test_new_state_has_default_workspace() {
    let (state, initial_tab_id) = MultiplexerState::new();
    assert_eq!(state.list_workspaces().len(), 1);
    assert!(state.get_tab(&initial_tab_id).is_some());
    assert_eq!(state.list_workspaces()[0].name, "Main");
    assert!(state.list_workspaces()[0].is_active);
}

#[test]
fn test_new_state_has_focused_tab() {
    let (state, initial_tab_id) = MultiplexerState::new();
    assert_eq!(state.get_focused_tab_id(), Some(initial_tab_id.as_str()));
}

#[test]
fn test_new_state_tab_is_terminal() {
    let (state, initial_tab_id) = MultiplexerState::new();
    let tab = state.get_tab(&initial_tab_id).unwrap();
    assert_eq!(tab.tab_type, TabType::Terminal);
    assert!(tab.pty_id.is_none());
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

    let (tab_id, tab_id) = result.unwrap();
    assert!(!tab_id.is_empty());
    assert!(!tab_id.is_empty());
    assert!(state.get_tab(&tab_id).is_some());
    let tab = state.get_tab(&tab_id).unwrap();
    assert_eq!(tab.role, None, "user-created tab must start roleless");
    assert!(!tab.can_spawn, "roleless tab must not inherit can_spawn");

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
fn test_claim_existing_tab_reuses_blank_tab_in_place() {
    let (mut state, _) = MultiplexerState::new();
    let ws_id = state.list_workspaces()[0].id.clone();
    let (tab_id, tab_id) = state.create_tab(&ws_id).unwrap();
    let tabs_before = state.get_workspace(&ws_id).unwrap().tabs.len();

    let (claimed_tab_id, claimed_tab_id) = state.claim_existing_tab(&ws_id, &tab_id).unwrap();
    assert_eq!(claimed_tab_id, tab_id, "claim must not mint a new tab id");
    assert_eq!(claimed_tab_id, tab_id, "claim must not mint a new Tab id");
    assert_eq!(
        state.get_workspace(&ws_id).unwrap().tabs.len(),
        tabs_before,
        "claim must not create a stray tab"
    );
}

#[test]
fn test_claim_existing_tab_rejects_tab_with_agent_already_attached() {
    let (mut state, _) = MultiplexerState::new();
    let ws_id = state.list_workspaces()[0].id.clone();
    let (tab_id, tab_id) = state.create_tab(&ws_id).unwrap();
    state.get_tab_mut(&tab_id).unwrap().agent_type = Some("claude".into());

    let result = state.claim_existing_tab(&ws_id, &tab_id);
    assert!(
        result.is_err(),
        "an already-spawned tab must not be claimable"
    );
    assert_eq!(
        state.get_tab(&tab_id).unwrap().agent_type.as_deref(),
        Some("claude"),
        "the rejected attach must preserve the original agent"
    );
    assert_eq!(
        state
            .get_workspace(&ws_id)
            .unwrap()
            .tabs
            .iter()
            .filter(|tab| tab.id == tab_id)
            .count(),
        1,
        "the rejected attach must leave exactly one Tab record"
    );
}

#[test]
fn test_claim_existing_tab_rejects_unknown_tab() {
    let (mut state, _) = MultiplexerState::new();
    let ws_id = state.list_workspaces()[0].id.clone();
    let result = state.claim_existing_tab(&ws_id, "nonexistent-tab");
    assert!(result.is_err());
}

#[test]
fn test_claim_existing_tab_rejects_protected_tab() {
    let (mut state, _) = MultiplexerState::new();
    let ws_id = state.list_workspaces()[0].id.clone();
    let (tab_id, tab_id) = state.create_tab(&ws_id).unwrap();
    state.get_tab_mut(&tab_id).unwrap().protected = true;

    let result = state.claim_existing_tab(&ws_id, &tab_id);
    assert!(result.is_err(), "a protected Tab must not be claimable");
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
        let title = state.get_tab(&tab.tab_id).unwrap().title.as_str();
        assert_ne!(title, "Terminal");
        assert!(titles.insert(title), "duplicate Tab title: {}", title);
    }
}

// split_tab removed: each tab owns one tab.
// Regressions are covered by close_tab-focused tests below.

#[test]
fn test_close_tab_preserves_active_tab_when_unrelated_single_leaf_tab_closes() {
    // Probe tabs are single-leaf; closing them must keep the current active tab, not switch to ws.tabs.last().
    let (mut state, _) = MultiplexerState::new();
    let ws_id = state.list_workspaces()[0].id.clone();
    let (tab2_id, _) = state.create_tab(&ws_id).unwrap();
    let (_tab3_id, tab3_tab_id) = state.create_tab(&ws_id).unwrap();

    // User parks on tab 2 (the middle one). tab 3 is a hidden probe tab.
    state.set_active_tab(&ws_id, &tab2_id).unwrap();
    assert_eq!(
        state
            .get_workspace(&ws_id)
            .and_then(|w| w.active_tab_id.clone()),
        Some(tab2_id.clone())
    );

    // Probe finishes → its single-leaf tab gets closed via close_tab.
    state.close_tab(&tab3_tab_id).unwrap();

    assert_eq!(
        state
            .get_workspace(&ws_id)
            .and_then(|w| w.active_tab_id.clone()),
        Some(tab2_id),
        "Closing an unrelated single-leaf tab must not move active_tab_id"
    );
}

#[test]
fn test_close_tab_returns_pty_id() {
    let (mut state, _initial_tab_id) = MultiplexerState::new();
    let ws_id = state.list_workspaces()[0].id.clone();
    let (_tab, new_tab_id) = state.create_tab(&ws_id).unwrap();
    state.link_pty(&new_tab_id, "pty-123".to_string());

    let result = state.close_tab(&new_tab_id).unwrap();
    assert_eq!(result, vec!["pty-123".to_string()]);
}

#[test]
fn test_close_tab_clears_focus_when_no_general() {
    // No general alias -> focus becomes None after closing the tab.
    // Preserves behavior for callers that still rely on this fallback.
    let (mut state, _initial_tab_id) = MultiplexerState::new();
    let ws_id = state.list_workspaces()[0].id.clone();
    let (_tab, new_tab_id) = state.create_tab(&ws_id).unwrap();

    state.set_focused_tab(&new_tab_id).unwrap();
    assert_eq!(state.get_focused_tab_id(), Some(new_tab_id.as_str()));

    state.close_tab(&new_tab_id).unwrap();
    assert_eq!(state.get_focused_tab_id(), None);
}

#[test]
fn test_close_tab_falls_back_to_general() {
    // With `general` alias registered, closing the focused tab moves focus to the General orchestrator tab instead of leaving the UI deselected.
    let (mut state, general_tab_id) = MultiplexerState::new();
    state.register_alias("general", &general_tab_id);

    let ws_id = state.list_workspaces()[0].id.clone();
    let (_tab, other_tab_id) = state.create_tab(&ws_id).unwrap();
    state.set_focused_tab(&other_tab_id).unwrap();

    state.close_tab(&other_tab_id).unwrap();
    assert_eq!(state.get_focused_tab_id(), Some(general_tab_id.as_str()));
}

#[test]
fn test_close_tab_falls_back_to_general() {
    let (mut state, general_tab_id) = MultiplexerState::new();
    state.register_alias("general", &general_tab_id);

    let ws_id = state.list_workspaces()[0].id.clone();
    let (tab2_id, tab2_tab_id) = state.create_tab(&ws_id).unwrap();
    state.set_focused_tab(&tab2_tab_id).unwrap();

    state.close_tab(&ws_id, &tab2_id).unwrap();
    assert_eq!(state.get_focused_tab_id(), Some(general_tab_id.as_str()));
}

#[test]
fn test_close_workspace_falls_back_to_general() {
    let (mut state, general_tab_id) = MultiplexerState::new();
    state.register_alias("general", &general_tab_id);

    let other_ws = state.create_workspace("Other".to_string(), None);
    let (_tab_id, other_tab_id) = state.create_tab(&other_ws.id).unwrap();
    state.set_focused_tab(&other_tab_id).unwrap();

    state.close_workspace(&other_ws.id).unwrap();
    assert_eq!(state.get_focused_tab_id(), Some(general_tab_id.as_str()));
}

#[test]
fn test_fallback_focus_to_general_no_op_without_alias() {
    let (mut state, tab_id) = MultiplexerState::new();
    state.set_focused_tab(&tab_id).unwrap();

    let result = state.fallback_focus_to_general();
    assert_eq!(result, None);
    // Focus left untouched (preferable to silent null).
    assert_eq!(state.get_focused_tab_id(), Some(tab_id.as_str()));
}

#[test]
fn test_fallback_focus_to_general_sets_focus() {
    let (mut state, general_tab_id) = MultiplexerState::new();
    state.register_alias("general", &general_tab_id);

    // Clear focus first to make the assignment observable.
    let other = state.create_tab();
    state.set_focused_tab(&other).unwrap();

    let result = state.fallback_focus_to_general();
    assert_eq!(result.as_deref(), Some(general_tab_id.as_str()));
    assert_eq!(state.get_focused_tab_id(), Some(general_tab_id.as_str()));
}

#[test]
fn test_close_workspace_returns_pty_ids() {
    let (mut state, initial_tab_id) = MultiplexerState::new();
    state.link_pty(&initial_tab_id, "pty-abc".to_string());

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
    let (tab2_id, tab2_tab_id) = state.create_tab(&ws_id).unwrap();

    state.link_pty(&tab2_tab_id, "pty-tab2".to_string());

    let pty_ids = state.close_tab(&ws_id, &tab2_id).unwrap();
    assert_eq!(pty_ids, vec!["pty-tab2"]);

    let ws = state.get_workspace(&ws_id).unwrap();
    assert_eq!(ws.tabs.len(), 1);
    assert!(state.get_tab(&tab2_tab_id).is_none());
}

#[test]
fn test_set_focused_tab() {
    let (mut state, _initial_tab_id) = MultiplexerState::new();
    let new_tab_id = state.create_tab();

    assert!(state.set_focused_tab(&new_tab_id).is_ok());
    assert_eq!(state.get_focused_tab_id(), Some(new_tab_id.as_str()));
}

#[test]
fn test_set_focused_tab_not_found() {
    let (mut state, _) = MultiplexerState::new();
    let result = state.set_focused_tab("nonexistent");
    assert!(result.is_err());
}

#[test]
fn test_link_pty() {
    let (mut state, initial_tab_id) = MultiplexerState::new();
    state.link_pty(&initial_tab_id, "pty-xyz".to_string());

    let tab = state.get_tab(&initial_tab_id).unwrap();
    assert_eq!(tab.pty_id, Some("pty-xyz".to_string()));
}

#[test]
fn test_snapshot() {
    let (state, initial_tab_id) = MultiplexerState::new();
    let snap = state.snapshot();
    assert_eq!(snap.workspaces.len(), 1);
    assert!(snap.tabs.contains_key(&initial_tab_id));
    assert!(snap.active_workspace_id.is_some());
    assert_eq!(snap.focused_tab_id, Some(initial_tab_id));
}

#[test]
fn test_get_tab_mut() {
    let (mut state, initial_tab_id) = MultiplexerState::new();
    {
        let tab = state.get_tab_mut(&initial_tab_id).unwrap();
        tab.title = "Custom Title".to_string();
    }
    let tab = state.get_tab(&initial_tab_id).unwrap();
    assert_eq!(tab.title, "Custom Title");
}

#[test]
fn test_set_orchestrator_role() {
    let (mut state, tab_id) = MultiplexerState::new();
    state
        .set_tab_role(&tab_id, AgentRole::Orchestrator, None)
        .unwrap();
    let tab = state.get_tab(&tab_id).unwrap();
    assert_eq!(tab.role, Some(AgentRole::Orchestrator));
    assert_eq!(tab.depth, 1);
    assert!(tab.can_spawn);
    assert!(tab.parent_tab_id.is_none());
}

#[test]
fn test_orchestrator_cannot_have_parent() {
    let (mut state, tab_id) = MultiplexerState::new();
    let result = state.set_tab_role(&tab_id, AgentRole::Orchestrator, Some("fake"));
    assert!(result.is_err());
}

#[test]
fn test_worker_requires_orchestrator_parent() {
    let (mut state, orch_id) = MultiplexerState::new();
    state
        .set_tab_role(&orch_id, AgentRole::Orchestrator, None)
        .unwrap();
    let worker_id = state.create_tab();
    state
        .set_tab_role(&worker_id, AgentRole::Worker, Some(&orch_id))
        .unwrap();

    let worker = state.get_tab(&worker_id).unwrap();
    assert_eq!(worker.role, Some(AgentRole::Worker));
    assert_eq!(worker.depth, 2);
    assert!(!worker.can_spawn);
    assert_eq!(worker.parent_tab_id.as_deref(), Some(orch_id.as_str()));
}

#[test]
fn test_worker_parent_must_not_be_worker() {
    let (mut state, orch_id) = MultiplexerState::new();
    state
        .set_tab_role(&orch_id, AgentRole::Orchestrator, None)
        .unwrap();
    let worker_parent = state.create_tab();
    state
        .set_tab_role(&worker_parent, AgentRole::Worker, Some(&orch_id))
        .unwrap();
    let other_id = state.create_tab();
    let result = state.set_tab_role(&other_id, AgentRole::Worker, Some(&worker_parent));
    assert!(result.is_err());
}

#[test]
fn test_worker_parent_must_allow_spawn() {
    let (mut state, orch_id) = MultiplexerState::new();
    state
        .set_tab_role(&orch_id, AgentRole::Orchestrator, None)
        .unwrap();
    state.set_can_spawn(&orch_id, false).unwrap();

    let worker_id = state.create_tab();
    let result = state.set_tab_role(&worker_id, AgentRole::Worker, Some(&orch_id));
    assert!(result.is_err());
}

#[test]
fn worker_under_orchestrator_is_depth_2_and_cannot_parent() {
    // 2-level hierarchy: a Worker sits directly under the Orchestrator, and there is no third level — a Worker can't parent another Worker.
    let (mut state, orch_id) = MultiplexerState::new();
    state
        .set_tab_role(&orch_id, AgentRole::Orchestrator, None)
        .unwrap();

    let worker_id = state.create_tab();
    state
        .set_tab_role(&worker_id, AgentRole::Worker, Some(&orch_id))
        .unwrap();

    let worker = state.get_tab(&worker_id).unwrap();
    assert_eq!(worker.role, Some(AgentRole::Worker));
    assert_eq!(worker.depth, 2);
    assert!(!worker.can_spawn);

    let sub_id = state.create_tab();
    let result = state.set_tab_role(&sub_id, AgentRole::Worker, Some(&worker_id));
    assert!(
        result.is_err(),
        "a Worker must not be allowed to parent another Worker"
    );
}

#[test]
fn test_get_children_ids() {
    let (mut state, orch_id) = MultiplexerState::new();
    state
        .set_tab_role(&orch_id, AgentRole::Orchestrator, None)
        .unwrap();
    let w1 = state.create_tab();
    state
        .set_tab_role(&w1, AgentRole::Worker, Some(&orch_id))
        .unwrap();
    let w2 = state.create_tab();
    state
        .set_tab_role(&w2, AgentRole::Worker, Some(&orch_id))
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
        .set_tab_role(&orch_id, AgentRole::Orchestrator, None)
        .unwrap();
    let worker_id = state.create_tab();
    state
        .set_tab_role(&worker_id, AgentRole::Worker, Some(&orch_id))
        .unwrap();

    state.clear_tab_role(&orch_id).unwrap();

    let orch = state.get_tab(&orch_id).unwrap();
    assert!(orch.role.is_none());
    assert_eq!(orch.depth, 0);

    let worker = state.get_tab(&worker_id).unwrap();
    assert!(worker.role.is_none());
    assert!(worker.parent_tab_id.is_none());
    assert_eq!(worker.depth, 0);
}

#[test]
fn manager_close_keeps_children_alive() {
    let (mut state, root_id) = MultiplexerState::new();
    let ws_id = state.list_workspaces()[0].id.clone();
    state
        .set_tab_role(&root_id, AgentRole::Orchestrator, None)
        .unwrap();
    let (_tab, manager_id) = state.create_tab(&ws_id).unwrap();
    state
        .set_tab_role(&manager_id, AgentRole::Manager, Some(&root_id))
        .unwrap();
    state.link_pty(&manager_id, "pty-manager".to_string());

    let (_tab, worker_id) = state.create_tab(&ws_id).unwrap();
    state
        .set_tab_role(&worker_id, AgentRole::Worker, Some(&root_id))
        .unwrap();
    state.link_pty(&worker_id, "pty-worker".to_string());
    state.repoint_parent(&worker_id, &manager_id);

    let (_tab, planner_id) = state.create_tab(&ws_id).unwrap();
    state
        .set_tab_role(&planner_id, AgentRole::Worker, Some(&root_id))
        .unwrap();
    state.link_pty(&planner_id, "pty-planner".to_string());
    state.repoint_parent(&planner_id, &manager_id);

    let pty_ids = state.close_tab(&manager_id).unwrap();
    assert_eq!(pty_ids, vec!["pty-manager".to_string()]);
    assert!(state.get_tab(&manager_id).is_none());
    for child_id in [&worker_id, &planner_id] {
        let child = state.get_tab(child_id).expect("child remains alive");
        assert!(child.parent_tab_id.is_none());
        assert_eq!(child.role, Some(AgentRole::Worker));
    }
}

#[test]
fn manager_close_preserves_team_registry_and_chat_backend_child() {
    let (mut state, root_id) = MultiplexerState::new();
    let ws_id = state.list_workspaces()[0].id.clone();
    state
        .set_tab_role(&root_id, AgentRole::Orchestrator, None)
        .unwrap();
    let team_id = state
        .resolve_or_mint_team_for_parent(&root_id)
        .unwrap()
        .team_id;
    let team_before = state.get_team(&team_id).cloned().unwrap();
    let (_tab, manager_id) = state.create_tab(&ws_id).unwrap();
    state
        .set_tab_role(&manager_id, AgentRole::Manager, Some(&root_id))
        .unwrap();
    let (_tab, chat_id) = state.create_tab(&ws_id).unwrap();
    state
        .set_tab_role(&chat_id, AgentRole::Worker, Some(&root_id))
        .unwrap();
    state.repoint_parent(&chat_id, &manager_id);
    state.get_tab_mut(&chat_id).unwrap().backing = TabBacking::ChatBackend {
        plugin_id: "test-backend".into(),
        session_id: "session".into(),
    };

    assert!(state.close_tab(&manager_id).unwrap().is_empty());
    let child = state.get_tab(&chat_id).expect("chat member survives");
    assert!(child.is_chat_backend());
    assert!(child.parent_tab_id.is_none());
    assert_eq!(child.team_id.as_deref(), Some(team_id.as_str()));
    assert_eq!(state.get_team(&team_id), Some(&team_before));
}

#[test]
fn multi_manager_close_detaches_only_the_closed_managers_children() {
    let (mut state, root_id) = MultiplexerState::new();
    let ws_id = state.list_workspaces()[0].id.clone();
    state
        .set_tab_role(&root_id, AgentRole::Orchestrator, None)
        .unwrap();
    let (_tab, first_manager) = state.create_tab(&ws_id).unwrap();
    state
        .set_tab_role(&first_manager, AgentRole::Manager, Some(&root_id))
        .unwrap();
    let (_tab, second_manager) = state.create_tab(&ws_id).unwrap();
    state
        .set_tab_role(&second_manager, AgentRole::Manager, Some(&root_id))
        .unwrap();
    let (_tab, worker_id) = state.create_tab(&ws_id).unwrap();
    state
        .set_tab_role(&worker_id, AgentRole::Worker, Some(&root_id))
        .unwrap();
    state.repoint_parent(&worker_id, &first_manager);

    state.close_tab(&first_manager).unwrap();
    assert!(state.get_tab(&second_manager).is_some());
    assert!(state.get_tab(&worker_id).is_some());
    assert!(state.get_tab(&worker_id).unwrap().parent_tab_id.is_none());
}

#[test]
fn last_manager_close_keeps_team_and_members() {
    let (mut state, root_id) = MultiplexerState::new();
    let ws_id = state.list_workspaces()[0].id.clone();
    state
        .set_tab_role(&root_id, AgentRole::Orchestrator, None)
        .unwrap();
    let team_id = state
        .resolve_or_mint_team_for_parent(&root_id)
        .unwrap()
        .team_id;
    let (_tab, member_id) = state.create_tab(&ws_id).unwrap();
    state
        .set_tab_role(&member_id, AgentRole::Worker, Some(&root_id))
        .unwrap();

    state.close_tab(&root_id).unwrap();
    assert!(state.get_team(&team_id).is_some());
    let member = state.get_tab(&member_id).expect("member survives");
    assert_eq!(member.team_id.as_deref(), Some(team_id.as_str()));
    assert!(member.parent_tab_id.is_none());
}

#[test]
fn explicit_close_team_still_cascades() {
    let (mut state, root_id) = MultiplexerState::new();
    let workspace_id = state.list_workspaces()[0].id.clone();
    state
        .set_tab_role(&root_id, AgentRole::Orchestrator, None)
        .unwrap();
    state.link_pty(&root_id, "pty-root".into());
    let (_tab, worker_id) = state.create_tab(&workspace_id).unwrap();
    state
        .set_tab_role(&worker_id, AgentRole::Worker, Some(&root_id))
        .unwrap();
    state.link_pty(&worker_id, "pty-worker".into());

    let pty_ids = state.close_tab_for_team_close(&root_id).unwrap();
    assert_eq!(pty_ids.len(), 2);
    assert!(state.get_tab(&root_id).is_none());
    assert!(state.get_tab(&worker_id).is_none());
}

#[test]
fn test_set_can_spawn_requires_role() {
    let (mut state, _) = MultiplexerState::new();
    let tab_id = state.create_tab();
    let result = state.set_can_spawn(&tab_id, true);
    assert!(result.is_err());
}

#[test]
fn test_set_can_spawn_on_orchestrator() {
    let (mut state, tab_id) = MultiplexerState::new();
    state
        .set_tab_role(&tab_id, AgentRole::Orchestrator, None)
        .unwrap();
    assert!(state.get_tab(&tab_id).unwrap().can_spawn);

    state.set_can_spawn(&tab_id, false).unwrap();
    assert!(!state.get_tab(&tab_id).unwrap().can_spawn);

    state.set_can_spawn(&tab_id, true).unwrap();
    assert!(state.get_tab(&tab_id).unwrap().can_spawn);
}

#[test]
fn test_rename_tab_pins_title() {
    let (mut state, tab_id) = MultiplexerState::new();
    assert_eq!(
        state.get_tab(&tab_id).unwrap().title_level,
        TitleLevel::Default
    );

    state.rename_tab(&tab_id, "Custom").unwrap();
    let tab = state.get_tab(&tab_id).unwrap();
    assert_eq!(tab.title, "Custom");
    assert_eq!(tab.title_level, TitleLevel::User);
}

#[test]
fn test_rename_tab_changes_canonical_title() {
    let (mut state, tab_id) = MultiplexerState::new();

    state.rename_tab(&tab_id, "My Tab").unwrap();

    assert_eq!(state.display_label_for_tab(&tab_id), "My Tab");
}

#[test]
fn test_rename_tab_changes_canonical_tab_title() {
    let (mut state, tab_id) = MultiplexerState::new();
    let ws_id = state.list_workspaces()[0].id.clone();
    let tab_id = state.get_workspace(&ws_id).unwrap().tabs[0].id.clone();

    state.rename_tab(&ws_id, &tab_id, "Renamed").unwrap();

    assert_eq!(state.get_tab(&tab_id).unwrap().title, "Renamed");
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
    let (mut state, tab_id) = MultiplexerState::new();
    state.link_pty(&tab_id, "pty-1".to_string());
    if let Some(tab) = state.get_tab_mut(&tab_id) {
        tab.cwd = Some("/test/path".to_string());
        tab.agent_type = Some("claude".to_string());
    }
    state
        .set_tab_role(&tab_id, AgentRole::Orchestrator, None)
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
    assert_eq!(tab.role, Some(AgentRole::Orchestrator));
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
fn test_session_snapshot_derives_agent_type_from_title_when_missing() {
    use crate::session::store::{
        SessionSnapshot, SplitNodeSnapshot, TabSnapshot, WorkspaceSnapshot,
    };

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
                        can_spawn: false,
                        depth: 0,
                        chat_view: None,
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
                    },
                }],
            }],
        };

        let restored = MultiplexerState::from_session_snapshot(&snap);
        let tab = restored.get_tab("tab-1").unwrap();
        assert_eq!(
            tab.agent_type.as_deref(),
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
                    can_spawn: false,
                    depth: 0,
                    chat_view: None,
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
                    can_spawn: false,
                    depth: 0,
                    chat_view: None,
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
        .set_tab_role(&cond_id, AgentRole::Orchestrator, None)
        .unwrap();
    let (_tab, worker_id) = state.create_tab(&ws_id).unwrap();
    state
        .set_tab_role(&worker_id, AgentRole::Worker, Some(&cond_id))
        .unwrap();

    let snap = state.to_session_snapshot();
    let restored = MultiplexerState::from_session_snapshot(&snap);

    let cond = restored.get_tab(&cond_id).unwrap();
    assert_eq!(cond.role, Some(AgentRole::Orchestrator));

    let worker = restored.get_tab(&worker_id).unwrap();
    assert_eq!(worker.role, Some(AgentRole::Worker));
    assert_eq!(worker.parent_tab_id.as_deref(), Some(cond_id.as_str()));
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
fn test_protected_tab_cannot_close() {
    let (mut state, _initial_tab_id) = MultiplexerState::new();
    let new_tab_id = state.create_tab();
    state.get_tab_mut(&new_tab_id).unwrap().protected = true;

    let result = state.close_tab(&new_tab_id);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "Cannot close protected Tab");
    assert!(state.get_tab(&new_tab_id).is_some());
}

#[test]
fn test_alias_register_and_resolve() {
    let (mut state, tab_id) = MultiplexerState::new();
    state.register_alias("main-terminal", &tab_id);

    let resolved = state.resolve_tab_id("main-terminal");
    assert_eq!(resolved, Some(tab_id.clone()));
}

#[test]
fn test_alias_not_found_falls_back_to_id() {
    let (state, tab_id) = MultiplexerState::new();
    let resolved = state.resolve_tab_id(&tab_id);
    assert_eq!(resolved, Some(tab_id));
}

#[test]
fn test_public_tab_id_resolves_to_its_backing_tab() {
    let (state, tab_id) = MultiplexerState::new();
    let tab_id = state.find_tab_for_tab(&tab_id).unwrap();

    assert_eq!(state.resolve_tab_id(&tab_id), Some(tab_id));
}

#[test]
fn test_resolve_nonexistent_returns_none() {
    let (state, _) = MultiplexerState::new();
    let resolved = state.resolve_tab_id("does-not-exist");
    assert_eq!(resolved, None);
}

#[test]
fn test_alias_unregister() {
    let (mut state, tab_id) = MultiplexerState::new();
    state.register_alias("my-tab", &tab_id);
    assert!(state.resolve_tab_id("my-tab").is_some());

    state.unregister_alias("my-tab");
    assert_eq!(state.resolve_tab_id("my-tab"), None);
}

#[test]
fn test_alias_to_deleted_tab_returns_none() {
    let (mut state, _initial_tab_id) = MultiplexerState::new();
    let ws_id = state.list_workspaces()[0].id.clone();
    let (_tab, new_tab_id) = state.create_tab(&ws_id).unwrap();
    state.register_alias("ephemeral", &new_tab_id);

    state.close_tab(&new_tab_id).unwrap();

    assert_eq!(state.resolve_tab_id("ephemeral"), None);
}
