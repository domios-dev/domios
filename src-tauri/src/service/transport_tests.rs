use super::AtomicSpawnParams;
use crate::multiplexer::state::MultiplexerState;

#[test]
fn snapshot_generation_strictly_increases() {
    let (state, _) = MultiplexerState::new();
    let g1 = state.snapshot().generation;
    let g2 = state.snapshot().generation;
    let g3 = state.snapshot().generation;
    assert!(g1 < g2 && g2 < g3, "each snapshot stamps a higher generation");
}

// Mirrors what `spawn_pane_atomic` does internally: create the pane, then the
// returned snapshot both contains the new pane AND carries a generation higher
// than any snapshot taken before the pane existed. The frontend's monotonic
// guard then drops the pre-pane (stale, lower-generation) snapshot.
#[test]
fn create_then_snapshot_contains_pane_and_advances_generation() {
    let (mut state, _initial) = MultiplexerState::new();
    let ws = state.snapshot().active_workspace_id.expect("default workspace");
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
    assert!(p.skip_permissions, "skipPermissions omitted → defaults to true");
}
