use std::sync::Arc;

use super::*;
use crate::agent::provider::{HookEventBinding, HookIngestionRoute};
use crate::multiplexer::state::MultiplexerState;
use codeterm_util::lock_hold::TimedRwLock;

fn agent_tab() -> (MuxState, String) {
    let (state, tab_id) = MultiplexerState::new();
    let mux: MuxState = Arc::new(TimedRwLock::new("mux-test", state));
    {
        let mut guard = mux.write();
        guard.get_tab_mut(&tab_id).unwrap().agent_type = Some("fixture".into());
    }
    (mux, tab_id)
}

fn tab_state(mux: &MuxState, tab_id: &str) -> Option<TimelineState> {
    mux.read()
        .get_tab(tab_id)
        .unwrap()
        .agent_state
        .as_ref()
        .map(|state| state.state)
}

fn opencode_like() -> Vec<HookEventBinding> {
    vec![
        HookEventBinding {
            event: "SessionStart".into(),
            route: HookIngestionRoute::SessionStart,
            verdict: None,
        },
        HookEventBinding {
            event: "session.status".into(),
            route: HookIngestionRoute::SessionState,
            verdict: Some(ProviderStateVerdict::Working),
        },
        HookEventBinding {
            event: "session.idle".into(),
            route: HookIngestionRoute::SessionState,
            verdict: Some(ProviderStateVerdict::Idle),
        },
    ]
}

#[test]
fn permission_request_declared_state_events_resolve_to_verdicts() {
    let roster = opencode_like();
    assert_eq!(
        declared_state_verdict(&roster, "session.status"),
        Some(ProviderStateVerdict::Working)
    );
    assert_eq!(
        declared_state_verdict(&roster, "session.idle"),
        Some(ProviderStateVerdict::Idle)
    );
}

#[test]
fn permission_request_unmapped_stream_event_changes_nothing() {
    let roster = opencode_like();
    let (mux, tab_id) = agent_tab();

    // Events the bus carries but the provider never declared.
    for event in ["session.deleted", "message.part.updated", "server.connected"] {
        assert_eq!(declared_state_verdict(&roster, event), None, "{event}");
    }
    // An event declared off the session_state route yields no verdict either.
    assert_eq!(declared_state_verdict(&roster, "SessionStart"), None);
    assert_eq!(tab_state(&mux, &tab_id), None);
}

#[test]
fn permission_request_declared_verdict_moves_the_tab_once() {
    let (mux, tab_id) = agent_tab();

    assert!(apply(&mux, &tab_id, ProviderStateVerdict::Working));
    assert_eq!(tab_state(&mux, &tab_id), Some(TimelineState::Working));

    assert!(apply(&mux, &tab_id, ProviderStateVerdict::Idle));
    assert_eq!(tab_state(&mux, &tab_id), Some(TimelineState::Idle));

    // A repeated verdict is not a change, so nothing re-emits.
    assert!(!apply(&mux, &tab_id, ProviderStateVerdict::Idle));
}

#[test]
fn permission_request_verdict_never_touches_a_non_agent_tab() {
    let (state, tab_id) = MultiplexerState::new();
    let mux: MuxState = Arc::new(TimedRwLock::new("mux-test", state));
    assert!(!apply(&mux, &tab_id, ProviderStateVerdict::Working));
    assert!(mux.read().get_tab(&tab_id).unwrap().agent_state.is_none());
}

#[test]
fn permission_request_verdict_maps_to_the_expected_live_state() {
    assert_eq!(
        live_state_for(ProviderStateVerdict::Working).state,
        TimelineState::Working
    );
    assert_eq!(
        live_state_for(ProviderStateVerdict::Idle).state,
        TimelineState::Idle
    );
}
