use super::{finish_launch, AtomicSpawnParams};
use crate::multiplexer::state::MultiplexerState;

#[test]
fn launch_failure_runs_rollback_and_propagates_error() {
    use std::cell::Cell;
    // The result shape a failing AgentTransport::launch returns.
    let rolled_back = Cell::new(false);
    let res = finish_launch(Err("launch boom".to_string()), || rolled_back.set(true));
    assert!(res.is_err(), "launch error propagates to the caller");
    assert!(rolled_back.get(), "tab rollback ran on launch failure");
}

#[test]
fn successful_launch_does_not_roll_back() {
    use std::cell::Cell;
    let rolled_back = Cell::new(false);
    let res = finish_launch(Ok(Some("the-command".to_string())), || {
        rolled_back.set(true)
    });
    assert_eq!(res.unwrap(), Some("the-command".to_string()));
    assert!(!rolled_back.get(), "a successful launch never rolls back");
}

#[test]
fn rollback_spawn_tab_removes_tab_pane_and_does_not_record_history() {
    // The mux-level unwind a failed launch triggers: the just-created tab + pane
    // are gone, focus is moved off the dead pane, and — unlike a real close — the
    // never-launched pane is NOT recorded into the recently-closed history.
    let (mut state, _) = MultiplexerState::new();
    let ws = state
        .snapshot()
        .active_workspace_id
        .expect("default workspace");
    let (tab, pane) = state.create_tab(&ws).expect("create_tab");
    state.set_active_tab(&ws, &tab).expect("activate tab");
    state.set_focused_pane(&pane).expect("focus pane");

    state.rollback_spawn_tab(&ws, &tab, &pane);

    let snap = state.snapshot();
    assert!(
        !snap.panes.contains_key(&pane),
        "rolled-back pane is removed"
    );
    let ws_view = snap
        .workspaces
        .iter()
        .find(|w| w.id == ws)
        .expect("workspace still present");
    assert!(
        !ws_view.tabs.iter().any(|t| t.id == tab),
        "rolled-back tab is removed"
    );
    assert_ne!(
        snap.focused_pane_id.as_deref(),
        Some(pane.as_str()),
        "focus moved off the rolled-back pane"
    );
    assert!(
        state
            .list_recently_closed(&ws)
            .iter()
            .all(|p| p.pane_id != pane),
        "a never-launched pane must not enter recently-closed history"
    );
}

#[test]
fn snapshot_generation_strictly_increases() {
    let (state, _) = MultiplexerState::new();
    let g1 = state.snapshot().generation;
    let g2 = state.snapshot().generation;
    let g3 = state.snapshot().generation;
    assert!(
        g1 < g2 && g2 < g3,
        "each snapshot stamps a higher generation"
    );
}

// Mirrors what `spawn_pane_atomic` does internally: create the pane, then the
// returned snapshot both contains the new pane AND carries a generation higher
// than any snapshot taken before the pane existed. The frontend's monotonic
// guard then drops the pre-pane (stale, lower-generation) snapshot.
#[test]
fn create_then_snapshot_contains_pane_and_advances_generation() {
    let (mut state, _initial) = MultiplexerState::new();
    let ws = state
        .snapshot()
        .active_workspace_id
        .expect("default workspace");
    // A stale read captured before the new pane existed.
    let stale = state.snapshot();
    let (_tab, pane) = state.create_tab(&ws).expect("create_tab");
    let fresh = state.snapshot();
    assert!(
        fresh.panes.contains_key(&pane),
        "atomic snapshot must contain the just-created pane"
    );
    assert!(
        fresh.generation > stale.generation,
        "post-create generation must beat the pre-create one"
    );
}

// The exact predicate the frontend reducer applies: a snapshot whose generation
// is `<=` the applied one is ignored; a strictly newer one wins.
fn would_apply(applied: u64, incoming: u64) -> bool {
    incoming > applied
}

#[test]
fn generation_guard_rejects_stale_accepts_newer() {
    assert!(!would_apply(5, 5), "equal generation is stale → rejected");
    assert!(!would_apply(5, 4), "older generation is stale → rejected");
    assert!(would_apply(5, 6), "newer generation → applied");
}

#[test]
fn atomic_spawn_params_deserialize_camel_case() {
    let p: AtomicSpawnParams = serde_json::from_value(serde_json::json!({
        "agentId": "claude",
        "workspaceId": "ws1",
        "cwd": "/tmp",
        "skipPermissions": false,
    }))
    .expect("camelCase params deserialize");
    assert_eq!(p.agent_id, "claude");
    assert_eq!(p.workspace_id.as_deref(), Some("ws1"));
    assert_eq!(p.cwd.as_deref(), Some("/tmp"));
    assert!(!p.skip_permissions);
}

#[test]
fn atomic_spawn_params_skip_permissions_defaults_true() {
    let p: AtomicSpawnParams =
        serde_json::from_value(serde_json::json!({ "agentId": "claude" })).unwrap();
    assert!(
        p.skip_permissions,
        "skipPermissions omitted → defaults to true"
    );
}
