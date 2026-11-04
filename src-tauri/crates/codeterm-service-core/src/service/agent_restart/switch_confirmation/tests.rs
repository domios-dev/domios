use super::*;

#[test]
fn detected_prompt_is_answered_once_then_session_evidence_stops_the_attempt() {
    let mut attempt = Attempt::default();
    assert!(!attempt.observe(false, false));
    assert!(attempt.observe(false, true));
    assert!(!attempt.observe(false, true));
    assert!(!attempt.confirmed);
    assert!(!attempt.observe(true, true));
    assert!(attempt.confirmed);
    assert!(!attempt.observe(false, true));
}

#[test]
fn absent_prompt_never_authorizes_a_key() {
    let mut attempt = Attempt::default();
    for _ in 0..8 {
        assert!(!attempt.observe(false, false));
    }
    assert!(!attempt.answered);
    assert!(!attempt.confirmed);
    assert!(!attempt.observe(true, false));
    assert!(attempt.confirmed);
}

#[test]
fn evidence_before_prompt_sends_nothing() {
    let mut attempt = Attempt::default();
    assert!(!attempt.observe(true, true));
    assert!(!attempt.answered);
    assert!(attempt.confirmed);
}

#[test]
fn confirmation_parser_dependency_does_not_retain_mux_screen_or_restart_tracker() {
    use crate::multiplexer::state::MultiplexerState;
    use crate::service::test_fixture::ServiceFixture;
    let (mut mux, tab) = MultiplexerState::new();
    mux.link_pty(&tab, "confirmation-pty".into());
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .screen_state
        .process("confirmation-pty", b"opaque prompt");
    let screen = fixture
        .service
        .screen_state
        .existing("confirmation-pty")
        .unwrap();
    let (entered, ready) = std::sync::mpsc::channel();
    let (release, receive) = std::sync::mpsc::channel();
    let service = fixture.service.clone();
    let target = tab.clone();
    let parsing = std::thread::spawn(move || {
        with_confirmation_screen(
            &service,
            &target,
            "confirmation-pty",
            |_| true,
            |text| {
                entered.send(()).unwrap();
                receive.recv_timeout(Duration::from_secs(5)).unwrap();
                text.contains("opaque prompt")
            },
        )
    });
    ready.recv_timeout(Duration::from_secs(2)).unwrap();
    let mux_available = fixture.service.mux.try_write().is_some();
    let screen_available = screen.try_lock().is_some();
    let tracker_available = fixture
        .service
        .ext::<crate::service::agent_restart::RestartTracker>()
        .try_lock()
        .is_some();
    let health = crate::dispatch::health_result().is_ok();
    let listed = fixture
        .service
        .mux
        .try_read()
        .map(|mux| mux.snapshot().tabs.contains_key(&tab))
        .unwrap_or(false);
    let send_resolved = fixture
        .service
        .mux
        .try_read()
        .and_then(|mux| mux.resolve_tab_id(&tab))
        .is_some();
    release.send(()).unwrap();
    assert!(parsing.join().unwrap().unwrap().1);
    assert!(mux_available && screen_available && tracker_available);
    assert!(health && listed && send_resolved);
}
