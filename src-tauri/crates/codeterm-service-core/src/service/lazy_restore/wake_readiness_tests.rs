use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::service::messenger::{Delivery, MessageEnvelope, Messenger};
use crate::service::test_fixture::ServiceFixture;
use codeterm_types::pty::ProvisionalLaunch;
use std::sync::Arc;

#[test]
fn waking_existing_pty_preserves_its_provider_readiness_and_flushes_held_input_once() {
    let (mux, id) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let writes = fixture.bind_recorded_agent_recipient(&id, "own-session", "own-nonce");
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&id).unwrap();
        tab.boot_stage = Some(BootStage::LaunchWritten);
        tab.agent_booting = true;
    }
    assert!(matches!(
        Messenger::deliver(
            &fixture.service,
            &id,
            MessageEnvelope::codeterm_reminder("fixture"),
            "opaque-held-input",
        )
        .unwrap(),
        Delivery::Pending { .. }
    ));
    assert!(fixture.has_pending_agent_delivery(&id));
    assert!(writes.texts.lock().is_empty());
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&id).unwrap();
        tab.frozen = true;
        tab.agent_booting = false;
        super::super::boot_stage::apply_boot_stage(tab, BootStage::AgentReady, None);
        assert!(tab.launch_session_authenticated());
    }
    let service = fixture.service.clone();
    let target = id.clone();
    let _wake = test_boundary::Boundary::install(
        &id,
        Arc::new(move || {
            let mux = service.mux.read();
            let tab = mux.get_tab(&target).unwrap();
            assert!(!tab.frozen);
            assert_eq!(tab.boot_stage, Some(BootStage::AgentReady));
            assert!(tab.launch_session_authenticated());
            Ok(())
        }),
    );
    fixture.service.wake_tab_on_request(&id).unwrap();
    writes.wait_for_flush();
    assert_eq!(*writes.texts.lock(), ["opaque-held-input"]);
    assert!(!fixture.has_pending_agent_delivery(&id));
    fixture.service.wake_tab_on_request(&id).unwrap();
    fixture.service.wake_agent_deliveries_on_state_change(&id);
    writes.wait_for_flush();
    assert_eq!(*writes.texts.lock(), ["opaque-held-input"]);
}

#[test]
fn waking_existing_pty_retains_provisional_attempt_until_own_binding_arrives() {
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.frozen = true;
    tab.agent_type = Some("fixture-provider".into());
    tab.pty_id = Some("own-pty".into());
    tab.agent_session_id = Some("own-session".into());
    tab.session_binding_nonce = Some("own-nonce".into());
    tab.boot_stage = Some(BootStage::LaunchBindingUnconfirmed);
    let launch = ProvisionalLaunch {
        pty_id: "own-pty".into(),
        binding_nonce: "own-nonce".into(),
        reason: "binding pending".into(),
        awaiting_binding_ack: true,
        first_turn_input_ready: false,
        first_turn_payload_written: false,
        first_turn_admitted_send_id: None,
    };
    tab.provisional_launch = Some(launch.clone());
    let fixture = ServiceFixture::new(mux);
    wake_with(&fixture.service, &id, false, |_, _| Ok(())).unwrap();
    {
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&id).unwrap();
        assert_eq!(tab.boot_stage, Some(BootStage::LaunchBindingUnconfirmed));
        assert_eq!(tab.provisional_launch, Some(launch));
    }
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&id)
        .unwrap()
        .reported_session_binding_nonce = Some("own-nonce".into());
    fixture.service.reconcile_authenticated_agent_input(&id);
    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&id).unwrap();
    assert_eq!(tab.boot_stage, Some(BootStage::AgentReady));
    assert!(tab.provisional_launch.is_none());
}

#[test]
fn waking_without_pty_starts_launch_and_waits_for_provider_binding() {
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.frozen = true;
    tab.restored_from_snapshot = true;
    tab.agent_type = Some("fixture-provider".into());
    tab.agent_session_id = Some("own-session".into());
    tab.boot_stage = Some(BootStage::AgentReady);
    let fixture = ServiceFixture::new(mux);
    let writes = fixture.record_agent_deliveries(&id);
    let service = fixture.service.clone();
    let target = id.clone();
    let _wake = test_boundary::Boundary::install(
        &id,
        Arc::new(move || {
            let mut mux = service.mux.write();
            let tab = mux.get_tab_mut(&target).unwrap();
            assert_eq!(tab.boot_stage, Some(BootStage::PtyStarting));
            assert!(!tab.frozen);
            assert!(tab.pty_id.is_none());
            tab.pty_id = Some("new-pty".into());
            tab.session_binding_nonce = Some("new-nonce".into());
            tab.reported_session_binding_nonce = None;
            tab.restored_from_snapshot = false;
            tab.agent_booting = true;
            tab.boot_stage = Some(BootStage::LaunchWritten);
            Ok(())
        }),
    );
    fixture.service.wake_tab_on_request(&id).unwrap();
    assert!(matches!(
        Messenger::deliver(
            &fixture.service,
            &id,
            MessageEnvelope::codeterm_reminder("fixture"),
            "opaque-new-launch-input",
        )
        .unwrap(),
        Delivery::Pending { .. }
    ));
    assert!(fixture.has_pending_agent_delivery(&id));
    assert!(writes.texts.lock().is_empty());
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&id)
        .unwrap()
        .agent_booting = false;
    fixture
        .service
        .emit_boot_stage(&id, BootStage::AgentReady, None);
    assert_eq!(
        fixture.service.mux.read().get_tab(&id).unwrap().boot_stage,
        Some(BootStage::LaunchBindingUnconfirmed)
    );
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&id)
        .unwrap()
        .reported_session_binding_nonce = Some("new-nonce".into());
    fixture.service.reconcile_authenticated_agent_input(&id);
    writes.wait_for_flush();
    assert_eq!(
        fixture.service.mux.read().get_tab(&id).unwrap().boot_stage,
        Some(BootStage::AgentReady)
    );
    assert_eq!(*writes.texts.lock(), ["opaque-new-launch-input"]);
    assert!(!fixture.has_pending_agent_delivery(&id));
}

#[test]
fn restart_thaw_retires_existing_pty_readiness() {
    let (mux, id) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let _writes = fixture.bind_recorded_agent_recipient(&id, "own-session", "own-nonce");
    fixture.service.mux.write().get_tab_mut(&id).unwrap().frozen = true;
    fixture.service.thaw_tab_for_restart(&id).unwrap();
    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&id).unwrap();
    assert!(!tab.frozen);
    assert_eq!(tab.boot_stage, Some(BootStage::PtyStarting));
}
