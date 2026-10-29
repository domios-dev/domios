use super::*;
use crate::service::test_fixture::ServiceFixture;

#[test]
fn lazy_first_turn_requires_the_exact_saved_session_until_binding() {
    let (fixture, id) = pre_ready_fixture();
    let mut authority = fixture.service.user_send_authority(&id).unwrap();
    let mut tab = fixture.service.mux.read().get_tab(&id).unwrap().clone();
    assert!(lazy_first_turn_session_matches(&tab, &authority));
    tab.agent_session_id = Some("foreign-session".into());
    assert!(!lazy_first_turn_session_matches(&tab, &authority));
    tab.agent_session_id = None;
    assert!(!lazy_first_turn_session_matches(&tab, &authority));
    authority.observed_session_id = None;
    assert!(lazy_first_turn_session_matches(&tab, &authority));
    authority.session_id = Some("bound-session".into());
    assert!(!lazy_first_turn_session_matches(&tab, &authority));
}

pub(super) fn mark_lazy_composer_ready(fixture: &ServiceFixture, tab_id: &str) {
    attach_fixture_pty(fixture, tab_id);
    let attempt = {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(tab_id).unwrap();
        tab.agent_session_id = None;
        tab.reported_session_binding_nonce = None;
        crate::service::launch_readiness::LaunchAttempt {
            tab_id: tab_id.to_string(),
            pty_id: tab.pty_id.clone().unwrap(),
            binding_nonce: tab.session_binding_nonce.clone().unwrap(),
        }
    };
    fixture.service.record_first_turn_input_ready(
        &attempt,
        "provider composer accepts the first turn; session binding is pending",
    );
}

#[test]
fn lazy_composer_admits_one_first_turn_then_binding_releases_the_queue() {
    for resumed in [false, true] {
        restart_first_send_case(resumed);
    }
}

fn restart_first_send_case(resumed: bool) {
    let (fixture, tab) = queue_fixture_with_session_mode(false, None, true);
    mark_lazy_composer_ready(&fixture, &tab);
    if resumed {
        fixture
            .service
            .mux
            .write()
            .get_tab_mut(&tab)
            .unwrap()
            .agent_session_id = Some("first-turn-session".into());
    }
    let pty = fixture
        .service
        .mux
        .read()
        .get_tab(&tab)
        .unwrap()
        .pty_id
        .clone()
        .unwrap();
    let attempt =
        crate::service::launch_readiness::LaunchAttempt::current(&fixture.service.mux, &tab, &pty)
            .unwrap();
    fixture.service.record_provisional_launch(
        &attempt,
        codeterm_types::pty::BootStage::LaunchBindingUnconfirmed,
        "late restart binding deadline",
    );
    assert_eq!(
        crate::service::boot_stage::wait_for_restart_readiness(
            &fixture.service.mux,
            &tab,
            Some("replaced-pty"),
            std::time::Duration::ZERO
        ),
        crate::service::boot_stage::RestartReadiness::Ready {
            pty_id: attempt.pty_id.clone()
        }
    );

    let authority = fixture.service.user_send_authority(&tab).unwrap();
    assert!(fixture.service.latch_user_input_authority(&authority));
    assert!(!fixture.service.user_authority_is_ready(&authority));

    let (writes_tx, writes_rx) = mpsc::channel();
    let _submit = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |record, text| {
            writes_tx
                .send((record.send_id.clone(), text.to_owned()))
                .unwrap();
            Err(DeliveryError::SubmitUnconfirmed {
                tab: "lazy-tab".into(),
                detail: "payload queued; turn evidence pending".into(),
            })
        }),
    );
    let first = fixture
        .service
        .start_human_chat_send(
            &tab,
            crate::service::messenger::HumanInput::composer("first turn"),
            None,
            Some("first-client".into()),
        )
        .unwrap();
    await_owner_idle(&fixture, &tab);
    assert_eq!(
        writes_rx.recv_timeout(TEST_DEADLINE).unwrap(),
        (first.send_id.clone(), "first turn".into())
    );
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&first.send_id)
            .unwrap()
            .state,
        ChatSendState::Unknown,
        "a queued first turn with missing evidence remains submit_unconfirmed"
    );
    let provisional = fixture
        .service
        .mux
        .read()
        .get_tab(&tab)
        .unwrap()
        .provisional_launch
        .clone()
        .unwrap();
    assert!(provisional.first_turn_input_ready);
    assert!(provisional.first_turn_payload_written);

    let second = fixture
        .service
        .start_human_chat_send(
            &tab,
            crate::service::messenger::HumanInput::composer("second turn"),
            None,
            Some("second-client".into()),
        )
        .unwrap();
    let third = fixture
        .service
        .start_human_chat_send(
            &tab,
            crate::service::messenger::HumanInput::composer("third turn"),
            None,
            Some("third-client".into()),
        )
        .unwrap();
    await_owner_idle(&fixture, &tab);
    fixture.service.drain_user_owner(&tab).unwrap();
    assert!(matches!(
        writes_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    let parked = fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .records(&tab)
        .unwrap();
    assert_eq!(parked.len(), 2);
    assert_eq!(parked[0].send_id, second.send_id);
    assert_eq!(parked[1].send_id, third.send_id);

    let (pty_id, binding_nonce) = {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab).unwrap();
        tab.agent_session_id = Some("first-turn-session".into());
        tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
        (
            tab.pty_id.clone().unwrap(),
            tab.session_binding_nonce.clone().unwrap(),
        )
    };
    assert!(fixture.service.retire_provisional_launch(
        &tab,
        &crate::service::ProviderSessionBinding {
            pty_id,
            binding_nonce,
        },
    ));
    await_owner_idle(&fixture, &tab);
    assert_eq!(
        writes_rx.recv_timeout(TEST_DEADLINE).unwrap(),
        (second.send_id.clone(), "\n\nsecond turn".into())
    );
    assert_eq!(
        writes_rx.recv_timeout(TEST_DEADLINE).unwrap(),
        (third.send_id.clone(), "\n\nthird turn".into())
    );
    assert!(matches!(
        writes_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    assert_eq!(
        fixture.service.mux.read().get_tab(&tab).unwrap().boot_stage,
        Some(codeterm_types::pty::BootStage::AgentReady)
    );
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&first.send_id)
            .unwrap()
            .state,
        ChatSendState::Unknown,
        "binding never replays the uncertain first turn"
    );
}

#[test]
fn non_lazy_launch_binding_unconfirmed_does_not_get_first_turn_admission() {
    let (fixture, tab) = queue_fixture_with_session_mode(false, None, false);
    mark_lazy_composer_ready(&fixture, &tab);
    let (writes_tx, writes_rx) = mpsc::channel();
    let _submit = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |record, _| {
            writes_tx.send(record.send_id.clone()).unwrap();
            Ok(())
        }),
    );
    let send = fixture
        .service
        .start_human_chat_send(
            &tab,
            crate::service::messenger::HumanInput::composer("held turn"),
            None,
            Some("held-client".into()),
        )
        .unwrap();
    await_owner_idle(&fixture, &tab);
    assert!(matches!(
        writes_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    assert!(fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .records(&tab)
        .unwrap()
        .iter()
        .any(|record| record.send_id == send.send_id));
}

#[test]
fn dead_lazy_launch_fails_held_send_without_first_turn_admission() {
    let (fixture, tab) = queue_fixture_with_session_mode(false, None, true);
    attach_fixture_pty(&fixture, &tab);
    fixture.service.emit_boot_stage(
        &tab,
        codeterm_types::pty::BootStage::SpawnFailed,
        Some("provider exited"),
    );
    let (writes_tx, writes_rx) = mpsc::channel();
    let _submit = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |record, _| {
            writes_tx.send(record.send_id.clone()).unwrap();
            Ok(())
        }),
    );
    let send = fixture
        .service
        .start_human_chat_send(
            &tab,
            crate::service::messenger::HumanInput::composer("never written"),
            None,
            Some("dead-client".into()),
        )
        .unwrap();
    await_owner_idle(&fixture, &tab);
    assert!(matches!(
        writes_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&send.send_id)
            .unwrap()
            .state,
        ChatSendState::Failed
    );
}

#[test]
fn lazy_session_provider_writes_every_send_after_agent_readiness() {
    let (fixture, tab) = queue_fixture_with_session_mode(false, None, true);
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab).unwrap();
        tab.agent_session_id = Some("opaque-session".into());
        tab.reported_session_binding_nonce = None;
    }
    mark_ready(&fixture, &tab);
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&tab)
        .unwrap()
        .launch_session_authenticated());
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Arc::new(Mutex::new(release_rx));
    let attempts = Arc::new(AtomicUsize::new(0));
    let count = attempts.clone();
    let wait_for_release = release_rx.clone();
    let writes = Arc::new(Mutex::new(Vec::new()));
    let observed = writes.clone();
    let _submit = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |record, text| {
            observed
                .lock()
                .push((record.send_id.clone(), text.to_string()));
            if count.fetch_add(1, Ordering::AcqRel) == 0 {
                entered_tx.send(record.send_id.clone()).unwrap();
                wait_for_release.lock().recv_timeout(TEST_DEADLINE).unwrap();
            }
            Err(DeliveryError::SubmitUnconfirmed {
                tab: record.authority.tab_id.clone(),
                detail: "opaque-postwrite".into(),
            })
        }),
    );
    let first_reservation = fixture
        .service
        .reserve_user_send(&tab, Some("lazy-client-0".into()))
        .unwrap();
    let first = fixture
        .service
        .prepare_and_admit_user_send(
            first_reservation,
            crate::service::messenger::HumanInput::composer("first lazy send"),
            false,
        )
        .unwrap()
        .status;
    assert_eq!(
        entered_rx.recv_timeout(TEST_DEADLINE).unwrap(),
        first.send_id
    );
    let later = ["second lazy send", "third lazy send"]
        .iter()
        .enumerate()
        .map(|(index, payload)| {
            let reservation = fixture
                .service
                .reserve_user_send(&tab, Some(format!("lazy-client-{}", index + 1)))
                .unwrap();
            fixture
                .service
                .prepare_and_admit_user_send(
                    reservation,
                    crate::service::messenger::HumanInput::composer(*payload),
                    false,
                )
                .unwrap()
                .status
        })
        .collect::<Vec<_>>();
    assert!(fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .records(&tab)
        .unwrap()
        .iter()
        .all(|record| !record.held_before_readiness));
    release_tx.send(()).unwrap();
    await_owner_idle(&fixture, &tab);

    let sends = std::iter::once(first).chain(later).collect::<Vec<_>>();
    let writes = writes.lock().clone();
    assert_eq!(
        writes
            .iter()
            .map(|(send_id, _)| send_id.as_str())
            .collect::<Vec<_>>(),
        sends
            .iter()
            .map(|status| status.send_id.as_str())
            .collect::<Vec<_>>(),
    );
    assert_eq!(
        writes
            .iter()
            .map(|(_, text)| text.as_str())
            .collect::<Vec<_>>(),
        [
            "first lazy send",
            "\n\nsecond lazy send",
            "\n\nthird lazy send"
        ],
    );
    assert!(fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .records(&tab)
        .unwrap()
        .is_empty());
    assert!(sends.iter().all(|status| {
        fixture
            .service
            .chat_send_ledger
            .status(&status.send_id)
            .unwrap()
            .state
            == ChatSendState::Unknown
    }));
    assert_eq!(attempts.load(Ordering::Acquire), sends.len());
}
