use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::TabBacking;
use crate::plan::TaskExecutionAttempt;
use crate::service::plan::task_attempts::{
    close_live_attempts_for_optional_tab, close_live_attempts_for_tab,
    record_noncompetitive_replacement_attempt,
};

fn attempt(id: &str, tab_id: &str) -> TaskExecutionAttempt {
    TaskExecutionAttempt {
        actual_reasoning_effort: None,
        id: id.into(),
        executor_tab_id: tab_id.into(),
        ..Default::default()
    }
}

#[test]
fn replacement_dispatch_closes_prior_noncompetitive_attempt() {
    let mut attempts = vec![attempt("attempt-a", "tab-a")];
    record_noncompetitive_replacement_attempt(&mut attempts, attempt("attempt-b", "tab-b"), 20);

    assert_eq!(attempts[0].closed_at, Some(20));
    assert_eq!(attempts[1].closed_at, None);
}

#[test]
fn reconcile_closes_dead_attempt_without_touching_active_replacement() {
    let mut attempts = vec![
        attempt("attempt-dead", "tab-dead"),
        attempt("attempt-live", "tab-live"),
    ];

    assert_eq!(
        close_live_attempts_for_tab(&mut attempts, "tab-dead", 50),
        1
    );
    assert_eq!(
        close_live_attempts_for_tab(&mut attempts, "tab-dead", 60),
        0
    );
    assert_eq!(attempts[0].closed_at, Some(50));
    assert_eq!(attempts[1].closed_at, None);
}

#[test]
fn replacement_closure_does_not_collapse_live_competition_group() {
    let mut peer_a = attempt("peer-a", "peer-a-tab");
    peer_a.competition_group_id = Some("group".into());
    let mut peer_b = attempt("peer-b", "peer-b-tab");
    peer_b.competition_group_id = Some("group".into());
    let mut attempts = vec![attempt("ordinary", "ordinary-tab"), peer_a, peer_b];

    record_noncompetitive_replacement_attempt(
        &mut attempts,
        attempt("replacement", "replacement-tab"),
        60,
    );
    assert_eq!(attempts[0].closed_at, Some(60));
    assert_eq!(attempts[1].closed_at, None);
    assert_eq!(attempts[2].closed_at, None);
    assert_eq!(attempts[3].closed_at, None);
    close_live_attempts_for_tab(&mut attempts, "peer-a-tab", 70);
    assert_eq!(attempts[2].closed_at, None);
}

#[test]
fn missing_executor_does_not_close_latest_open_attempt() {
    let mut attempts = vec![attempt("attempt-a", "tab-a"), attempt("attempt-b", "tab-b")];

    assert_eq!(
        close_live_attempts_for_optional_tab(&mut attempts, None, 90),
        0
    );
    assert_eq!(
        close_live_attempts_for_optional_tab(&mut attempts, Some("  "), 90),
        0
    );
    assert!(attempts.iter().all(|attempt| attempt.closed_at.is_none()));
}

#[test]
fn live_attempt_session_requires_assigned_tab() {
    let mut open = attempt("attempt-open", "tab-open");
    open.session_id = Some("session-open".into());

    assert_eq!(
        live_attempt_session_for_assignment(None, &[open.clone()]),
        None
    );
    assert_eq!(
        live_attempt_session_for_assignment(Some("  "), &[open]),
        None
    );
}

#[test]
fn live_attempt_session_selects_exact_parallel_competition_peer() {
    let mut peer_a = attempt("peer-a", "tab-a");
    peer_a.session_id = Some("session-a".into());
    peer_a.competition_group_id = Some("group".into());
    let mut peer_b = attempt("peer-b", "tab-b");
    peer_b.session_id = Some("session-b".into());
    peer_b.competition_group_id = Some("group".into());

    assert_eq!(
        live_attempt_session_for_assignment(Some("tab-a"), &[peer_a, peer_b]),
        Some("session-a".into())
    );
}

#[test]
fn live_attempt_session_rejects_ambiguous_assigned_attempts() {
    let mut first = attempt("first", "tab-shared");
    first.session_id = Some("session-first".into());
    let mut second = attempt("second", "tab-shared");
    second.session_id = Some("session-second".into());

    assert_eq!(
        live_attempt_session_for_assignment(Some("tab-shared"), &[first, second]),
        None
    );
}

#[test]
fn tab_has_live_agent_requires_verified_session_and_supports_both_backings() {
    let (mut mux, tab_id) = MultiplexerState::new();
    assert!(!tab_has_live_agent(mux.get_tab(&tab_id).unwrap()));

    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("provider-neutral".into());
    tab.agent_session_id = Some("session-1".into());
    tab.session_binding_nonce = Some("nonce-1".into());
    tab.reported_session_binding_nonce = Some("nonce-1".into());
    tab.chat_view = true;
    tab.boot_stage = Some(codeterm_types::pty::BootStage::AgentReady);
    tab.pty_id = Some("pty-1".into());
    assert!(tab_has_live_agent(tab));
    tab.reported_session_binding_nonce = Some("stale-nonce".into());
    assert!(!tab_has_live_agent(tab));
    tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();

    tab.agent_state = Some(
        codeterm_types::pty::TabVerdict {
            state: codeterm_types::pty::TimelineState::Question,
            subtype: None,
        }
        .into(),
    );
    assert!(tab_has_live_agent(tab));

    tab.exit_suspected_at = Some(std::time::Instant::now());
    assert!(!tab_has_live_agent(tab));
    tab.exit_suspected_at = None;

    tab.pty_id = None;
    tab.backing = TabBacking::ChatBackend {
        plugin_id: "provider-neutral".into(),
        session_id: "session-1".into(),
    };
    assert!(tab_has_live_agent(tab));

    tab.backing = TabBacking::ChatBackend {
        plugin_id: "foreign-provider".into(),
        session_id: "session-1".into(),
    };
    assert!(!tab_has_live_agent(tab));
    tab.backing = TabBacking::ChatBackend {
        plugin_id: "provider-neutral".into(),
        session_id: "foreign-session".into(),
    };
    assert!(!tab_has_live_agent(tab));
}
