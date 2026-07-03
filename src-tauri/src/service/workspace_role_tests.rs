//! Regression: user-created tabs/sessions start roleless; explicit promotion/spawn still works.

use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::AgentRole;

#[test]
fn user_created_tab_starts_without_role() {
    let (mut mux, _) = MultiplexerState::new();
    let ws = mux.list_workspaces()[0].id.clone();
    let (_tab, pane_id) = mux.create_tab(&ws).unwrap();
    let pane = mux.get_pane(&pane_id).unwrap();
    assert_eq!(pane.role, None);
    assert!(!pane.can_spawn);
}

#[test]
fn explicit_orchestrator_promotion_sets_role_and_can_spawn() {
    let (mut mux, _) = MultiplexerState::new();
    let ws = mux.list_workspaces()[0].id.clone();
    let (_tab, pane_id) = mux.create_tab(&ws).unwrap();
    mux.set_pane_role(&pane_id, AgentRole::Orchestrator, None)
        .expect("explicit promotion");
    let pane = mux.get_pane(&pane_id).unwrap();
    assert_eq!(pane.role, Some(AgentRole::Orchestrator));
    assert!(pane.can_spawn);
}

#[test]
fn explicit_worker_spawn_role_still_applies() {
    let (mut mux, _) = MultiplexerState::new();
    let ws = mux.list_workspaces()[0].id.clone();
    let (_orch_tab, orch) = mux.create_tab(&ws).unwrap();
    mux.set_pane_role(&orch, AgentRole::Orchestrator, None)
        .expect("parent orchestrator");
    let (_worker_tab, worker) = mux.create_tab(&ws).unwrap();
    mux.set_pane_role(&worker, AgentRole::Worker, Some(&orch))
        .expect("explicit worker role");
    let worker_pane = mux.get_pane(&worker).unwrap();
    assert_eq!(worker_pane.role, Some(AgentRole::Worker));
    assert!(!worker_pane.can_spawn);
    assert_eq!(worker_pane.parent_pane_id.as_deref(), Some(orch.as_str()));
}
