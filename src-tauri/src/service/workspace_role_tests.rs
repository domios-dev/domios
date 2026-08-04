//! Regression: user-created tabs/sessions start roleless; explicit promotion/spawn still works.

use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::AgentRole;

#[test]
fn user_created_tab_starts_without_role() {
    let (mut mux, _) = MultiplexerState::new();
    let ws = mux.list_workspaces()[0].id.clone();
    let tab_id = mux.create_tab(&ws).unwrap();
    let tab = mux.get_tab(&tab_id).unwrap();
    assert_eq!(tab.role, None);
    assert!(!tab.can_spawn);
}

#[test]
fn explicit_orchestrator_promotion_sets_role_and_can_spawn() {
    let (mut mux, _) = MultiplexerState::new();
    let ws = mux.list_workspaces()[0].id.clone();
    let tab_id = mux.create_tab(&ws).unwrap();
    mux.set_orchestrator_role(&tab_id)
        .expect("explicit promotion");
    let tab = mux.get_tab(&tab_id).unwrap();
    assert_eq!(tab.role, Some(AgentRole::Orchestrator));
    assert!(tab.can_spawn);
}

#[test]
fn explicit_worker_spawn_role_still_applies() {
    let (mut mux, _) = MultiplexerState::new();
    let ws = mux.list_workspaces()[0].id.clone();
    let orch = mux.create_tab(&ws).unwrap();
    mux.set_orchestrator_role(&orch)
        .expect("parent orchestrator");
    let worker = mux.create_tab(&ws).unwrap();
    mux.set_tab_role(&worker, AgentRole::Worker, Some(&orch))
        .expect("explicit worker role");
    let worker_tab = mux.get_tab(&worker).unwrap();
    assert_eq!(worker_tab.role, Some(AgentRole::Worker));
    assert!(!worker_tab.can_spawn);
    assert_eq!(worker_tab.parent_tab_id.as_deref(), Some(orch.as_str()));
}
