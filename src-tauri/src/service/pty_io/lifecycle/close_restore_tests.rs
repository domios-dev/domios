use super::claim_restored_launch;
use crate::multiplexer::commands::MuxState;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::CloseRestoreTabState;
use codeterm_util::lock_hold::TimedRwLock;
use std::sync::Arc;

fn restored_agent() -> (MuxState, String) {
    let (mut state, id) = MultiplexerState::new();
    let tab = state.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("fixture-provider".into());
    tab.agent_session_id = Some("own-exact-session".into());
    tab.cwd = Some("/fixture".into());
    tab.restored_from_snapshot = true;
    tab.close_restore = Some(CloseRestoreTabState {
        exact_session: true,
        ..Default::default()
    });
    (
        Arc::new(TimedRwLock::new("close-restore-session", state)),
        id,
    )
}

#[test]
fn history_restore_refuses_missing_session_without_claiming_fresh_launch() {
    let (mux, id) = restored_agent();
    assert!(claim_restored_launch(
        &mux,
        &id,
        "fixture-provider",
        Some("own-exact-session"),
        Some("/fixture"),
        false
    )
    .is_err());
    let guard = mux.read();
    let tab = guard.get_tab(&id).unwrap();
    assert_eq!(tab.agent_session_id.as_deref(), Some("own-exact-session"));
    assert!(tab.restored_from_snapshot);
    assert!(!tab.agent_booting);
}

#[test]
fn history_restore_claims_only_its_own_session_once() {
    let (mux, id) = restored_agent();
    assert!(claim_restored_launch(
        &mux,
        &id,
        "fixture-provider",
        Some("latest-global-session"),
        Some("/fixture"),
        true
    )
    .unwrap()
    .is_none());
    let launch = claim_restored_launch(
        &mux,
        &id,
        "fixture-provider",
        Some("own-exact-session"),
        Some("/fixture"),
        true,
    )
    .unwrap()
    .unwrap();
    assert_eq!(launch.session_id.as_deref(), Some("own-exact-session"));
    assert!(claim_restored_launch(
        &mux,
        &id,
        "fixture-provider",
        Some("own-exact-session"),
        Some("/fixture"),
        true
    )
    .unwrap()
    .is_none());
}
