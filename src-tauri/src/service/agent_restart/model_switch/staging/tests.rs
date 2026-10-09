use super::*;
use crate::multiplexer::state::MultiplexerState;

fn fixture() -> (SwitchRequest, Tab) {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("provider".into());
    tab.agent_session_id = Some("session".into());
    tab.pty_id = Some("pty".into());
    tab.session_binding_nonce = Some("nonce".into());
    tab.reported_session_binding_nonce = Some("nonce".into());
    let request = SwitchRequest {
        id: "request".into(),
        tab_id,
        provider: "provider".into(),
        session_id: "session".into(),
        pty_id: "pty".into(),
        nonce: "nonce".into(),
        launch: tab.restorable_launch.clone(),
        model: "model".into(),
        effort: None,
        force: false,
        restart_fallback: false,
        mechanism: ModelSwitchMode::Live,
        deadline: Instant::now() + STAGING_DEADLINE,
        completion_generation: 0,
    };
    (request, tab.clone())
}

#[test]
fn replacement_disconnects_old_waiter_and_only_latest_can_claim() {
    let (first, _) = fixture();
    let mut second = first.clone();
    second.id = "replacement".into();
    second.model = "new-model".into();
    let mut queue = PendingSwitches::default();
    let (tx1, rx1) = channel();
    let (tx2, rx2) = channel();
    queue.replace(first.clone(), tx1);
    queue.replace(second.clone(), tx2);
    assert_eq!(rx1.recv(), Ok(()));
    assert!(rx1.recv().is_err());
    assert!(!queue.claim(&first));
    queue.signal_completed(&second.tab_id, &second.session_id, &second.nonce);
    queue.signal_idle(&second.tab_id);
    assert_eq!(rx2.try_recv(), Ok(()));
    assert!(queue.claim(&second));
    assert!(!queue.claim(&second));
}

#[test]
fn cancel_disconnects_wait_and_idle_cannot_claim_cancelled_request() {
    let (request, _) = fixture();
    let (tx, rx) = channel();
    let mut queue = PendingSwitches::default();
    queue.replace(request.clone(), tx);
    queue.cancel(&request.tab_id);
    assert_eq!(rx.recv(), Ok(()));
    assert!(rx.recv().is_err());
    queue.signal_idle(&request.tab_id);
    assert!(!queue.claim(&request));
}

#[test]
fn idle_before_own_turn_completion_never_wakes_the_staged_switch() {
    let (request, _) = fixture();
    let (tx, rx) = channel();
    let mut queue = PendingSwitches::default();
    queue.replace(request.clone(), tx);
    queue.signal_idle("foreign-tab");
    assert_eq!(rx.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty));
    queue.signal_idle(&request.tab_id);
    assert_eq!(rx.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty));
    queue.signal_completed("foreign-tab", &request.session_id, &request.nonce);
    queue.signal_completed(&request.tab_id, "foreign-session", &request.nonce);
    queue.signal_completed(&request.tab_id, &request.session_id, "foreign-nonce");
    assert_eq!(rx.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty));
    queue.signal_completed(&request.tab_id, &request.session_id, &request.nonce);
    assert_eq!(rx.try_recv(), Ok(()));
    assert!(queue.claim(&request));
}

#[test]
fn changed_session_pty_nonce_provider_or_launch_refuses_request() {
    let (request, tab) = fixture();
    let now = Instant::now();
    assert!(request.matches(&tab, now));
    for change in 0..6 {
        let mut stale = tab.clone();
        match change {
            0 => stale.agent_session_id = Some("foreign-session".into()),
            1 => stale.pty_id = Some("new-pty".into()),
            2 => stale.session_binding_nonce = Some("new-nonce".into()),
            3 => stale.reported_session_binding_nonce = None,
            4 => stale.agent_type = Some("foreign-provider".into()),
            _ => stale.restorable_launch.model = Some("other-model".into()),
        }
        assert!(!request.matches(&stale, now));
    }
}

#[test]
fn deadline_refuses_even_an_unchanged_binding() {
    let (request, tab) = fixture();
    assert!(request.matches(&tab, request.deadline - Duration::from_nanos(1)));
    assert!(!request.matches(&tab, request.deadline));
}

#[test]
fn capability_reload_cannot_turn_a_live_request_into_a_restart() {
    let (request, _) = fixture();
    assert!(request.require_mechanism(ModelSwitchMode::Live).is_ok());
    assert!(request
        .require_mechanism(ModelSwitchMode::RestartResume)
        .is_err());
}

#[test]
fn staged_then_already_idle_claims_once_without_another_transition() {
    let (request, _) = fixture();
    let mut queue = PendingSwitches::default();
    let (tx, rx) = channel();
    queue.signal_completed(&request.tab_id, &request.session_id, &request.nonce);
    queue.replace_at_idle(request.clone(), tx, true);
    assert_eq!(rx.try_recv(), Ok(()));
    assert!(queue.claim(&request));
    assert!(!queue.claim(&request));
}

#[test]
fn idle_between_staging_check_and_waiter_subscription_is_retained_once() {
    let (request, _) = fixture();
    let mut queue = PendingSwitches::default();
    let (tx, rx) = channel();
    queue.replace_at_idle(request.clone(), tx, false);
    queue.signal_idle(&request.tab_id);
    assert_eq!(rx.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty));
    queue.signal_completed(&request.tab_id, &request.session_id, &request.nonce);
    queue.signal_idle(&request.tab_id);
    assert_eq!(rx.try_recv(), Ok(()));
    assert!(queue.claim(&request));
    assert_eq!(rx.try_recv(), Ok(()));
    assert!(!queue.claim(&request));
}

#[test]
fn an_earlier_turn_completion_cannot_release_a_later_staged_turn() {
    let (mut request, _) = fixture();
    let mut queue = PendingSwitches::default();
    queue.signal_completed(&request.tab_id, &request.session_id, &request.nonce);
    request.completion_generation =
        queue.completion_generation(&request.tab_id, &request.session_id, &request.nonce);
    let (tx, rx) = channel();
    queue.replace_at_idle(request.clone(), tx, true);
    queue.signal_idle(&request.tab_id);
    assert_eq!(rx.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty));
    queue.signal_completed(&request.tab_id, &request.session_id, &request.nonce);
    assert_eq!(rx.try_recv(), Ok(()));
    assert!(queue.claim(&request));
}

#[test]
fn a_new_turn_invalidates_completed_evidence_before_a_staged_switch_claims_it() {
    let (request, _) = fixture();
    let mut queue = PendingSwitches::default();
    let (tx, rx) = channel();
    queue.replace(request.clone(), tx);
    queue.signal_completed(&request.tab_id, &request.session_id, &request.nonce);
    assert_eq!(rx.try_recv(), Ok(()));
    assert!(queue.completed(&request));
    queue.signal_started(&request.tab_id);
    assert!(!queue.completed(&request));
    queue.signal_idle(&request.tab_id);
    assert_eq!(rx.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty));
    queue.signal_completed(&request.tab_id, &request.session_id, &request.nonce);
    assert_eq!(rx.try_recv(), Ok(()));
    assert!(queue.completed(&request));
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn completion_signal_requires_the_current_claimed_session_and_verified_nonce() {
    let (mut request, source) = fixture();
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = source.agent_type;
    tab.agent_session_id = source.agent_session_id;
    tab.pty_id = source.pty_id;
    tab.session_binding_nonce = source.session_binding_nonce;
    tab.reported_session_binding_nonce = source.reported_session_binding_nonce;
    request.tab_id = tab_id.clone();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let (tx, rx) = channel();
    fixture
        .service
        .restart_tracker
        .lock()
        .staged_models
        .replace(request, tx);
    fixture.service.apply_staged_model_switch_on_idle(&tab_id);
    fixture
        .service
        .staged_model_turn_completed(&tab_id, "foreign", "nonce");
    fixture
        .service
        .staged_model_turn_completed(&tab_id, "session", "foreign");
    assert_eq!(rx.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty));
    fixture
        .service
        .staged_model_turn_completed(&tab_id, "session", "nonce");
    assert_eq!(rx.try_recv(), Ok(()));
}
