use super::{backend_owned_pty_launch, finish_launch, AtomicSpawnParams};
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
fn pty_launch_is_dispatched_by_backend_without_terminal_mount() {
    let mut dispatched = None;

    let result = backend_owned_pty_launch(
        "codex --dangerously-bypass-approvals-and-sandbox".into(),
        |command| {
            dispatched = Some(command);
        },
    );

    assert_eq!(
        result, None,
        "the frontend must receive no deferred PTY command"
    );
    assert_eq!(
        dispatched.as_deref(),
        Some("codex --dangerously-bypass-approvals-and-sandbox"),
        "backend receives the launch command even when no Terminal component exists"
    );
}

#[test]
fn rollback_spawn_tab_removes_tab_tab_and_does_not_record_history() {
    let (mut state, _) = MultiplexerState::new();
    let ws = state
        .snapshot()
        .active_workspace_id
        .expect("default workspace");
    let (tab, tab) = state.create_tab(&ws).expect("create_tab");
    state.set_active_tab(&ws, &tab).expect("activate tab");
    state.set_focused_tab(&tab).expect("focus Tab");

    state.rollback_spawn_tab(&ws, &tab, &tab);

    let snap = state.snapshot();
    assert!(
        !snap.tabs.contains_key(&tab),
        "rolled-back Tab is removed"
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
        snap.focused_tab_id.as_deref(),
        Some(tab.as_str()),
        "focus moved off the rolled-back Tab"
    );
    assert!(
        state
            .list_recently_closed(&ws)
            .iter()
            .all(|p| p.tab_id != tab),
        "a never-launched Tab must not enter recently-closed history"
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

#[test]
fn create_then_snapshot_contains_tab_and_advances_generation() {
    let (mut state, _initial) = MultiplexerState::new();
    let ws = state
        .snapshot()
        .active_workspace_id
        .expect("default workspace");
    // A stale read captured before the new tab existed.
    let stale = state.snapshot();
    let (_tab, tab) = state.create_tab(&ws).expect("create_tab");
    let fresh = state.snapshot();
    assert!(
        fresh.tabs.contains_key(&tab),
        "atomic snapshot must contain the just-created Tab"
    );
    assert!(
        fresh.generation > stale.generation,
        "post-create generation must beat the pre-create one"
    );
}

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
