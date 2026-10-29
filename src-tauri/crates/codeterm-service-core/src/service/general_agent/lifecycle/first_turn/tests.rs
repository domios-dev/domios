use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::service::boot_stage::BootStage;
use crate::service::test_fixture::ServiceFixture;

#[test]
fn only_a_fresh_lazy_general_without_bootstrap_waits_for_user_input() {
    assert!(awaiting_user_turn(true, false, false));
    assert!(!awaiting_user_turn(false, false, false));
    assert!(!awaiting_user_turn(true, true, false));
    assert!(!awaiting_user_turn(true, false, true));
}

#[test]
fn composer_readiness_exposes_first_turn_without_a_session_or_payload() {
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("lazy-fixture".into());
    tab.pty_id = Some("owned-pty".into());
    tab.session_binding_nonce = Some("owned-nonce".into());
    tab.boot_stage = Some(BootStage::ProviderReady);
    tab.agent_booting = true;
    let fixture = ServiceFixture::new(mux);
    let attempt = LaunchAttempt::current(&fixture.service.mux, &id, "owned-pty").unwrap();
    publish(&fixture.service, &attempt);
    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&id).unwrap();
    assert!(!tab.agent_booting);
    assert!(tab.agent_session_id.is_none());
    assert!(tab.pending_delivery.is_none());
    let composer = tab.provisional_launch.as_ref().unwrap();
    assert!(composer.first_turn_input_ready);
    assert!(composer.awaiting_binding_ack);
    assert!(!composer.first_turn_payload_written);
    assert_eq!(composer.pty_id, "owned-pty");
    assert_eq!(composer.binding_nonce, "owned-nonce");
}
