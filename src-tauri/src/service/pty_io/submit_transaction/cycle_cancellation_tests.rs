use super::*;

#[cfg(target_os = "windows")]
fn real_pty_fixture(pty_id: &str) -> (crate::service::test_fixture::ServiceFixture, String) {
    let (mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture
        .service
        .pty
        .lock()
        .spawn_with_id(
            Some(pty_id.to_string()),
            None,
            None,
            80,
            24,
            std::collections::HashMap::new(),
        )
        .expect("spawn real PTY fixture");
    fixture
        .service
        .mux
        .write()
        .link_pty(&tab_id, pty_id.to_string());
    (fixture, tab_id)
}

#[cfg(target_os = "windows")]
#[test]
fn cancelled_submit_cycle_cannot_write_trailing_enter_into_next_prompt() {
    let pty_id = "cancelled-cycle-next-prompt";
    let (fixture, tab_id) = real_pty_fixture(pty_id);
    let old_claim = claim_submit_cycle(pty_id);
    let (at_boundary_tx, at_boundary_rx) = std::sync::mpsc::channel();
    let (release_retry_tx, release_retry_rx) = std::sync::mpsc::channel();
    old_claim.set_boundary_hook(move || {
        at_boundary_tx.send(()).unwrap();
        release_retry_rx.recv().unwrap();
    });
    let events = std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
    let retry_events = std::sync::Arc::clone(&events);
    let retry_service = fixture.service.clone();
    let retry_claim = old_claim.clone();
    let retry_pty_id = pty_id.to_string();
    let retry = std::thread::spawn(move || {
        let transport = PtySubmitTransport {
            svc: &retry_service,
            pty_id: &retry_pty_id,
            signals: Mutex::new(None),
            permit: None,
            submit_cycle: Arc::clone(&retry_claim.cycle),
            submit_generation: retry_claim.generation,
        };
        let wrote = transport
            .write_submit_key_if_active(b"\r", Instant::now() + Duration::from_secs(1))
            .unwrap();
        if wrote {
            retry_events.lock().push("old:key-write");
        }
        wrote
    });

    at_boundary_rx.recv().unwrap();
    fixture.service.write_raw(&tab_id, b"/model").unwrap();
    events.lock().push("new:ingress-acknowledged");
    events.lock().push("new:/model-picker");
    release_retry_tx.send(()).unwrap();

    assert!(!retry.join().unwrap());
    assert_eq!(
        &*events.lock(),
        &["new:ingress-acknowledged", "new:/model-picker"]
    );
    fixture.service.pty.lock().kill(pty_id).unwrap();
}

#[cfg(target_os = "windows")]
#[test]
fn new_action_input_cancels_prior_submit_cycle_before_worker_first_key() {
    let pty_id = "cancelled-cycle-action-input";
    let (fixture, tab_id) = real_pty_fixture(pty_id);
    let old_claim = claim_submit_cycle(pty_id);

    fixture
        .service
        .inject_raw_keys_async(&tab_id, vec![b"/model".to_vec()])
        .unwrap();

    assert!(!old_claim.is_current());
    fixture.service.pty.lock().kill(pty_id).unwrap();
}

#[cfg(target_os = "windows")]
#[test]
fn superseded_pre_paste_claim_has_no_parser_side_effects_on_real_transport() {
    let pty_id = "superseded-parser-side-effects";
    let (fixture, tab_id) = real_pty_fixture(pty_id);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("codex"),
    ));
    let old_claim = claim_submit_cycle(pty_id);
    let (at_boundary_tx, at_boundary_rx) = std::sync::mpsc::channel();
    let (release_paste_tx, release_paste_rx) = std::sync::mpsc::channel();
    old_claim.set_boundary_hook(move || {
        at_boundary_tx.send(()).unwrap();
        release_paste_rx.recv().unwrap();
    });
    let old_service = fixture.service.clone();
    let old_pty_id = pty_id.to_string();
    let old_send = std::thread::spawn(move || {
        submit_text_with_key_claimed(
            &old_service,
            &old_pty_id,
            "codex",
            true,
            b"\r",
            VerifySubmitStrategy::None,
            VerifyOpts::default(),
            None,
            false,
            old_claim,
        )
    });

    at_boundary_rx.recv().unwrap();
    fixture.service.write_raw(&tab_id, b"/model").unwrap();
    release_paste_tx.send(()).unwrap();

    let error = old_send.join().unwrap().unwrap_err();
    assert!(matches!(
        &error,
        InjectionError::AgentInputUnavailable { pty_id: actual } if actual == pty_id
    ));
    let tab = fixture.service.mux.read().get_tab(&tab_id).unwrap().clone();
    assert!(!tab.agent_type_locked);
    assert!(tab.agent_type.is_none());
    let delivery = crate::service::delivery_error::DeliveryError::from_injection(pty_id, error);
    assert!(matches!(
        delivery,
        crate::service::delivery_error::DeliveryError::TabBusyRefused { .. }
    ));
    assert!(!delivery.bytes_written());
    fixture.service.pty.lock().kill(pty_id).unwrap();
}

#[test]
fn submit_cycle_cancellation_is_scoped_to_one_pty() {
    let first = claim_submit_cycle("cycle-scope-first");
    let second = claim_submit_cycle("cycle-scope-second");
    let newer_first = claim_submit_cycle("cycle-scope-first");

    assert!(!first.is_current());
    assert!(newer_first.is_current());
    assert!(second.is_current());
    assert!(!first.same_cycle(&second));
}

#[test]
fn submit_cycle_registry_reclaims_pty_key_after_last_claim() {
    let pty_id = "cycle-registry-reclamation";
    assert!(!submit_cycle_registered(pty_id));
    {
        let claim = claim_submit_cycle(pty_id);
        assert!(claim.is_current());
        assert!(submit_cycle_registered(pty_id));
    }
    assert!(!submit_cycle_registered(pty_id));
}

#[test]
fn newer_ingress_claim_does_not_wait_for_prior_write_acknowledgement() {
    let pty_id = "cycle-acknowledgement-separation";
    let old_claim = claim_submit_cycle(pty_id);
    let observed_old_claim = old_claim.clone();
    let (enqueued_tx, enqueued_rx) = std::sync::mpsc::sync_channel(1);
    let (release_ack_tx, release_ack_rx) = std::sync::mpsc::sync_channel(1);

    let old_write = std::thread::spawn(move || {
        let acknowledgment = old_claim
            .enqueue_if_current(|| {
                enqueued_tx.send(()).unwrap();
                release_ack_rx
            })
            .expect("old generation must enqueue before cancellation");
        acknowledgment.recv().unwrap();
    });

    enqueued_rx.recv().unwrap();
    let new_claim = claim_submit_cycle(pty_id);
    assert!(!observed_old_claim.is_current());
    assert!(new_claim.is_current());
    release_ack_tx.send(()).unwrap();
    old_write.join().unwrap();
}

struct SupersededBeforePaste;

impl SubmitTransport for SupersededBeforePaste {
    fn observe(&self) -> Option<Observation> {
        None
    }

    fn draft_evidence_count(&self, _text: &str) -> usize {
        0
    }

    fn generation(&self) -> u64 {
        0
    }

    fn wait_change(&self, _generation: u64, _deadline: Instant) -> bool {
        false
    }

    fn turn_started(&self) -> bool {
        false
    }

    fn write_paste(&self, _text: &str, _deadline: Instant) -> Result<usize, InjectionError> {
        panic!("a superseded claim must not write its payload")
    }

    fn write_paste_if_active(
        &self,
        _text: &str,
        _deadline: Instant,
    ) -> Result<Option<usize>, InjectionError> {
        Ok(None)
    }

    fn write_submit_key(&self, _key: &[u8], _deadline: Instant) -> Result<(), InjectionError> {
        panic!("a superseded pre-paste claim must not write Enter")
    }
}

#[test]
fn superseded_pre_paste_claim_is_zero_byte_actionable_refusal() {
    const PTY_ID: &str = "pty-real-ingress-owner";
    let error = run_with_submit_key(
        &SupersededBeforePaste,
        PTY_ID,
        "payload",
        true,
        b"\r",
        VerifySubmitStrategy::None,
        VerifyOpts::default(),
    )
    .unwrap_err();

    assert!(matches!(
        &error,
        InjectionError::AgentInputUnavailable { pty_id } if pty_id == PTY_ID
    ));
    let delivery = crate::service::delivery_error::DeliveryError::from_injection(PTY_ID, error);
    assert!(matches!(
        &delivery,
        crate::service::delivery_error::DeliveryError::TabBusyRefused { .. }
    ));
    assert!(!delivery.bytes_written());
    assert_eq!(delivery.code(), "tab_busy_refused");
    assert!(delivery.to_string().contains(PTY_ID));
}
