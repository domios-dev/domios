use super::*;
use crate::service::messenger::{MessageEnvelope, MessageKind, Messenger};

#[test]
fn durable_send_wakes_sleeping_recipient_and_never_replays_written_or_uncertain_input() {
    for uncertain in [false, true] {
        let (fixture, id, _) = super::durable::restored();
        crate::service::lazy_restore::initialize(&fixture.service, 48);
        let wakes = Arc::new(Mutex::new(0));
        let captured_wakes = wakes.clone();
        let service = fixture.service.clone();
        let target = id.clone();
        let _wake = crate::service::lazy_restore::test_boundary::Boundary::install(
            &id,
            Arc::new(move || {
                *captured_wakes.lock() += 1;
                let mut mux = service.mux.write();
                let tab = mux.get_tab_mut(&target).unwrap();
                assert!(!tab.frozen);
                assert_eq!(tab.agent_session_id.as_deref(), Some("saved-session"));
                tab.agent_booting = true;
                Ok(())
            }),
        );
        let writes = Arc::new(Mutex::new(Vec::new()));
        let captured = writes.clone();
        let (done_tx, done_rx) = mpsc::channel();
        let _write = super::super::test_boundary::Boundary::install(
            &id,
            Arc::new(move |payload| {
                captured.lock().push(payload.transport.body().to_string());
                if uncertain {
                    Err("submit_unconfirmed: bytes written; exact evidence unavailable".into())
                } else {
                    Ok(())
                }
            }),
            Arc::new(move || {
                let _ = done_tx.send(());
            }),
        );
        super::durable::admit(&fixture, &id);
        done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(*wakes.lock(), 1);
        assert!(writes.lock().is_empty());
        fixture.resume_recorded_agent_recipient(&id, "saved-session", "first-launch");
        done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(!fixture.has_pending_agent_delivery(&id));
        assert_eq!(
            *writes.lock(),
            ["Original run: original-run; timestamp: 1000\nRead result.json"]
        );
        fixture.resume_recorded_agent_recipient(&id, "saved-session", "second-launch");
        done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(writes.lock().len(), 1);
        assert_eq!(*wakes.lock(), 1);
    }
}

#[test]
fn routed_messages_wake_restored_tabs_and_write_once_after_own_session_authentication() {
    for kind in [
        MessageKind::Message,
        MessageKind::Report,
        MessageKind::Reminder,
        MessageKind::System,
        MessageKind::Mesh,
        MessageKind::Telegram,
        MessageKind::Pebble,
    ] {
        let (mut mux, id) = MultiplexerState::new();
        let tab = mux.get_tab_mut(&id).unwrap();
        tab.agent_type = Some("opaque-provider".into());
        tab.agent_session_id = Some("saved-session".into());
        tab.session_binding_nonce = Some("old-nonce".into());
        tab.reported_session_binding_nonce = Some("old-nonce".into());
        let restored = MultiplexerState::from_session_snapshot(&mux.to_session_snapshot());
        let fixture = ServiceFixture::new(restored);
        crate::service::lazy_restore::initialize(&fixture.service, 48);
        assert!(fixture.service.mux.read().get_tab(&id).unwrap().frozen);

        let wakes = Arc::new(Mutex::new(0));
        let captured_wakes = wakes.clone();
        let service = fixture.service.clone();
        let target = id.clone();
        let _wake = crate::service::lazy_restore::test_boundary::Boundary::install(
            &id,
            Arc::new(move || {
                *captured_wakes.lock() += 1;
                let mut mux = service.mux.write();
                let tab = mux.get_tab_mut(&target).unwrap();
                assert!(!tab.frozen);
                assert_eq!(tab.agent_session_id.as_deref(), Some("saved-session"));
                tab.restored_from_snapshot = false;
                tab.agent_booting = true;
                tab.pty_id = Some("own-pty".into());
                tab.session_binding_nonce = Some("new-nonce".into());
                tab.reported_session_binding_nonce = None;
                tab.boot_stage = Some(BootStage::AgentLaunching);
                Ok(())
            }),
        );
        let writes = Arc::new(Mutex::new(Vec::new()));
        let captured = writes.clone();
        let (done_tx, done_rx) = mpsc::channel();
        let _write = super::super::test_boundary::Boundary::install(
            &id,
            Arc::new(move |payload| {
                assert_eq!(
                    payload.expected_session_id.as_deref(),
                    Some("saved-session")
                );
                assert_eq!(payload.expected_binding_nonce.as_deref(), Some("new-nonce"));
                captured.lock().push(payload.transport.body().to_string());
                Ok(())
            }),
            Arc::new(move || {
                done_tx.send(()).unwrap();
            }),
        );
        assert!(matches!(
            Messenger::deliver(
                &fixture.service,
                &id,
                MessageEnvelope {
                    kind,
                    ..MessageEnvelope::tab("fixture-sender", "sender-tab")
                },
                "opaque restart fixture payload",
            )
            .unwrap(),
            Delivery::Pending { .. }
        ));
        assert_eq!(*wakes.lock(), 1);
        assert!(writes.lock().is_empty());
        authenticate(&fixture, &id);
        done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(*writes.lock(), ["opaque restart fixture payload"]);
        fixture.service.reconcile_authenticated_agent_input(&id);
        done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(*writes.lock(), ["opaque restart fixture payload"]);
        assert_eq!(*wakes.lock(), 1);
    }
}
