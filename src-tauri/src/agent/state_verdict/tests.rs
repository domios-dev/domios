use std::sync::Arc;

use super::*;
use crate::agent::provider::{HookEventBinding, HookIngestionRoute};
use crate::multiplexer::state::MultiplexerState;
use codeterm_types::pty::{reconcile_status, BootStage, TabStatus};
use codeterm_util::lock_hold::TimedRwLock;

fn agent_tab() -> (MuxState, String) {
    let (state, tab_id) = MultiplexerState::new();
    let mux: MuxState = Arc::new(TimedRwLock::new("mux-test", state));
    {
        let mut guard = mux.write();
        let tab = guard.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("fixture".into());
        tab.agent_session_id = Some("ses_tab".into());
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
    for event in [
        "session.deleted",
        "message.part.updated",
        "server.connected",
    ] {
        assert_eq!(declared_state_verdict(&roster, event), None, "{event}");
    }
    // An event declared off the session_state route yields no verdict either.
    assert_eq!(declared_state_verdict(&roster, "SessionStart"), None);
    assert_eq!(tab_state(&mux, &tab_id), None);
}

#[test]
fn permission_request_declared_verdict_moves_the_tab_once() {
    let (mux, tab_id) = agent_tab();

    assert!(apply(
        &mux,
        &tab_id,
        "ses_tab",
        ProviderStateVerdict::Working
    ));
    assert_eq!(tab_state(&mux, &tab_id), Some(TimelineState::Working));

    assert!(apply(&mux, &tab_id, "ses_tab", ProviderStateVerdict::Idle));
    assert_eq!(tab_state(&mux, &tab_id), Some(TimelineState::Idle));

    // A repeated verdict is not a change, so nothing re-emits.
    assert!(!apply(&mux, &tab_id, "ses_tab", ProviderStateVerdict::Idle));
}

#[test]
fn opencode_session_status_promotes_quiescent_sidebar_to_working() {
    let verdict = declared_state_verdict(&opencode_like(), "session.status").unwrap();
    let state = live_state_for(verdict).state;

    assert_eq!(
        reconcile_status(
            TabStatus::Idle,
            Some(BootStage::AgentReady),
            Some(state),
            true,
            false,
        ),
        TabStatus::Working
    );
}

#[test]
fn declared_verdict_changes_only_the_tab_bound_session() {
    let (mux, tab_id) = agent_tab();

    assert!(!apply(
        &mux,
        &tab_id,
        "ses_sibling",
        ProviderStateVerdict::Working
    ));
    assert_eq!(tab_state(&mux, &tab_id), None);

    assert!(apply(
        &mux,
        &tab_id,
        "ses_tab",
        ProviderStateVerdict::Working
    ));
    assert_eq!(tab_state(&mux, &tab_id), Some(TimelineState::Working));
}

#[test]
fn permission_request_verdict_never_touches_a_non_agent_tab() {
    let (state, tab_id) = MultiplexerState::new();
    let mux: MuxState = Arc::new(TimedRwLock::new("mux-test", state));
    assert!(!apply(
        &mux,
        &tab_id,
        "ses_missing",
        ProviderStateVerdict::Working
    ));
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

#[test]
fn codex_rpc_control_waiting_maps_to_question_state() {
    assert_eq!(
        live_state_for(ProviderStateVerdict::Waiting).state,
        TimelineState::Question
    );
}

#[test]
fn runtime_turn_details_map_to_shared_live_state() {
    use codeterm_types::pty::QuestionSubtype;
    let cases = [
        (
            RuntimeTurnState::Working,
            None,
            TimelineState::Working,
            None,
            None,
        ),
        (
            RuntimeTurnState::Working,
            Some(RuntimeTurnDetail::Compacting),
            TimelineState::Working,
            None,
            Some("compacting"),
        ),
        (
            RuntimeTurnState::NeedsInput,
            Some(RuntimeTurnDetail::AwaitingApproval),
            TimelineState::Question,
            Some(QuestionSubtype::Permission),
            None,
        ),
        (
            RuntimeTurnState::NeedsInput,
            Some(RuntimeTurnDetail::AwaitingInput),
            TimelineState::Question,
            None,
            None,
        ),
        (
            RuntimeTurnState::NeedsInput,
            Some(RuntimeTurnDetail::SystemError),
            TimelineState::Question,
            Some(QuestionSubtype::Error),
            None,
        ),
        (
            RuntimeTurnState::Idle,
            Some(RuntimeTurnDetail::Interrupted),
            TimelineState::Idle,
            None,
            Some("interrupted"),
        ),
        (
            RuntimeTurnState::Idle,
            Some(RuntimeTurnDetail::Failed),
            TimelineState::Idle,
            None,
            Some("failed"),
        ),
        (
            RuntimeTurnState::Idle,
            None,
            TimelineState::Idle,
            None,
            None,
        ),
    ];
    for (state, detail, timeline, subtype, phase) in cases {
        let live = live_state_for_runtime(&state, detail, None);
        assert_eq!(live.state, timeline, "{state:?} {detail:?}");
        assert_eq!(live.subtype, subtype, "{state:?} {detail:?}");
        assert_eq!(live.phase.as_deref(), phase, "{state:?} {detail:?}");
        assert_eq!(live.status_line, None);
    }
}

#[test]
fn runtime_status_line_is_trimmed_bounded_and_cleared() {
    let long = format!("  {}  ", "é".repeat(400));
    let live = live_state_for_runtime(&RuntimeTurnState::Working, None, Some(&long));
    assert_eq!(
        live.status_line.as_ref().map(|line| line.chars().count()),
        Some(160)
    );
    assert_eq!(
        live_state_for_runtime(&RuntimeTurnState::Working, None, Some("  ")).status_line,
        None
    );
}

#[test]
fn runtime_compaction_then_turn_end_moves_the_bound_tab() {
    let (mux, tab_id) = agent_tab();
    let compacting = live_state_for_runtime(
        &RuntimeTurnState::Working,
        Some(RuntimeTurnDetail::Compacting),
        None,
    );
    assert!(apply_live(&mux, &tab_id, "ses_tab", compacting.clone()));
    assert!(!apply_live(&mux, &tab_id, "ses_tab", compacting));
    let phase = |mux: &MuxState| {
        mux.read()
            .get_tab(&tab_id)
            .unwrap()
            .agent_state
            .as_ref()
            .and_then(|s| s.phase.clone())
    };
    assert_eq!(phase(&mux).as_deref(), Some("compacting"));
    assert!(!apply_live(
        &mux,
        &tab_id,
        "ses_other",
        live_state_for_runtime(&RuntimeTurnState::Idle, None, None)
    ));
    assert!(apply_live(
        &mux,
        &tab_id,
        "ses_tab",
        live_state_for_runtime(
            &RuntimeTurnState::Idle,
            Some(RuntimeTurnDetail::Interrupted),
            None
        )
    ));
    assert_eq!(tab_state(&mux, &tab_id), Some(TimelineState::Idle));
    assert_eq!(phase(&mux).as_deref(), Some("interrupted"));
}
