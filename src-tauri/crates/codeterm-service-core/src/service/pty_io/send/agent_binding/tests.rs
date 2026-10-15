use super::*;
use crate::multiplexer::{state::MultiplexerState, types::Tab};
use crate::service::messenger::{AgentDeliveryPayload, QueuedAgentPayload};
use codeterm_types::pty::BootStage;

fn launching_tab() -> Tab {
    let (state, id) = MultiplexerState::new();
    let mut tab = state.get_tab(&id).unwrap().clone();
    tab.agent_type = Some("late-session-provider".into());
    tab.pty_id = Some("launch-pty".into());
    tab.session_binding_nonce = Some("launch-nonce".into());
    tab.agent_booting = true;
    tab.boot_stage = Some(BootStage::LaunchWritten);
    tab
}

fn pinned(tab: &Tab) -> AgentDeliveryPayload {
    let (pty, session, nonce) = delivery_binding(tab).unwrap();
    AgentDeliveryPayload {
        trace_id: None,
        transport: QueuedAgentPayload::Text {
            text: "opaque-payload".into(),
        },
        observer: None,
        idempotency_key: None,
        expected_pty_id: pty,
        expected_session_id: session,
        expected_binding_nonce: Some(nonce),
    }
}

#[test]
fn incoming_launch_does_not_pin_the_outgoing_pty() {
    let mut tab = launching_tab();
    tab.boot_stage = Some(BootStage::PtyStarting);
    tab.agent_session_id = Some("saved-own-session".into());
    let payload = pinned(&tab);
    assert!(payload.expected_pty_id.is_none());
    assert!(queued_delivery_target(&tab, &payload).is_err());
    tab.pty_id = Some("replacement-pty".into());
    tab.boot_stage = Some(BootStage::AgentReady);
    tab.agent_booting = false;
    assert!(queued_delivery_target(&tab, &payload).is_err());
    tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
    assert_eq!(
        queued_delivery_target(&tab, &payload).unwrap(),
        ("replacement-pty".into(), "late-session-provider".into())
    );
    tab.session_binding_nonce = Some("unrelated-launch".into());
    assert!(queued_delivery_target(&tab, &payload).is_err());
}

#[test]
fn reserved_nonce_admits_before_pty_registration_and_refuses_ended_launch() {
    let mut tab = launching_tab();
    tab.pty_id = None;
    tab.boot_stage = Some(BootStage::PtyStarting);
    let payload = pinned(&tab);
    assert!(payload.expected_pty_id.is_none());
    assert_eq!(
        payload.expected_binding_nonce.as_deref(),
        Some("launch-nonce")
    );
    assert!(queued_delivery_target(&tab, &payload).is_err());
    tab.pty_id = Some("launch-pty".into());
    tab.boot_stage = Some(BootStage::AgentReady);
    tab.agent_booting = false;
    assert!(queued_delivery_target(&tab, &payload).is_ok());
    tab.boot_stage = Some(BootStage::SpawnFailed);
    assert!(delivery_binding(&tab).is_none());
    assert!(queued_delivery_target(&tab, &payload).is_err());
}

#[test]
fn fresh_ready_agent_accepts_routed_input_before_first_session_turn() {
    let mut tab = launching_tab();
    let payload = pinned(&tab);
    assert!(queued_delivery_target(&tab, &payload).is_err());
    tab.boot_stage = Some(BootStage::AgentReady);
    tab.agent_booting = false;
    assert_eq!(
        queued_delivery_target(&tab, &payload).unwrap(),
        ("launch-pty".into(), "late-session-provider".into())
    );
    assert!(tab.agent_session_id.is_none());
    assert!(tab.reported_session_binding_nonce.is_none());
}

#[test]
fn pending_routed_input_survives_session_binding_only_in_its_own_launch() {
    let mut tab = launching_tab();
    let payload = pinned(&tab);
    tab.agent_session_id = Some("new-own-session".into());
    tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
    tab.boot_stage = Some(BootStage::AgentReady);
    tab.agent_booting = false;
    assert!(queued_delivery_target(&tab, &payload).is_ok());
    let bound = pinned(&tab);
    tab.agent_session_id = Some("foreign-session".into());
    assert!(queued_delivery_target(&tab, &bound).is_err());
    tab.session_binding_nonce = Some("next-launch".into());
    assert!(queued_delivery_target(&tab, &payload).is_err());
    tab.session_binding_nonce = Some("launch-nonce".into());
    tab.pty_id = Some("replacement-pty".into());
    assert!(queued_delivery_target(&tab, &payload).is_err());
}

#[test]
fn failed_or_unmaterialized_restore_never_accepts_a_launch_binding() {
    let mut tab = launching_tab();
    tab.boot_stage = Some(BootStage::SpawnFailed);
    assert!(delivery_binding(&tab).is_none());
    tab.boot_stage = Some(BootStage::AgentReady);
    tab.restored_from_snapshot = true;
    assert!(delivery_binding(&tab).is_none());
    tab.restored_from_snapshot = false;
    tab.agent_type = None;
    assert!(delivery_binding(&tab).is_none());
}

#[test]
fn restored_routed_send_waits_for_its_saved_session_to_reauthenticate() {
    let mut tab = launching_tab();
    tab.agent_session_id = Some("saved-own-session".into());
    tab.boot_stage = Some(BootStage::ReaderReady);
    let payload = pinned(&tab);
    assert!(queued_delivery_target(&tab, &payload).is_err());
    tab.boot_stage = Some(BootStage::AgentReady);
    assert!(queued_delivery_target(&tab, &payload).is_err());
    tab.boot_stage = Some(BootStage::LaunchBindingUnconfirmed);
    tab.agent_booting = false;
    tab.provisional_launch = Some(codeterm_types::pty::ProvisionalLaunch {
        pty_id: "launch-pty".into(),
        binding_nonce: "launch-nonce".into(),
        reason: "binding pending".into(),
        awaiting_binding_ack: false,
    });
    assert!(payload_matches_binding(
        &payload,
        &delivery_binding(&tab).unwrap()
    ));
    assert!(queued_delivery_target(&tab, &payload).is_err());
    tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
    tab.boot_stage = Some(BootStage::AgentReady);
    assert!(queued_delivery_target(&tab, &payload).is_ok());
    tab.agent_session_id = Some("replacement-session".into());
    assert!(queued_delivery_target(&tab, &payload).is_err());
}

#[test]
fn live_resumed_launch_retains_queued_delivery_until_exact_binding() {
    let mut tab = launching_tab();
    tab.agent_session_id = Some("own-session".into());
    let payload = pinned(&tab);
    tab.agent_booting = false;
    let attempt = crate::service::launch_readiness::LaunchAttempt {
        tab_id: tab.id.clone(),
        pty_id: "launch-pty".into(),
        binding_nonce: "launch-nonce".into(),
    };
    tab.provisional_launch = crate::service::boot_stage::provisional_launch_for(
        &tab,
        &attempt.pty_id,
        &attempt.binding_nonce,
        "opaque-reason",
    );
    tab.boot_stage = Some(BootStage::LaunchBindingUnconfirmed);
    assert!(payload_matches_binding(
        &payload,
        &delivery_binding(&tab).unwrap()
    ));
    assert!(queued_delivery_target(&tab, &payload).is_err());
    tab.reported_session_binding_nonce = Some("launch-nonce".into());
    let evidence = crate::service::ProviderSessionBinding {
        pty_id: "launch-pty".into(),
        binding_nonce: "launch-nonce".into(),
    };
    assert!(crate::service::boot_stage::late_readiness_retires_provisional_launch(&tab, &evidence));
    crate::service::boot_stage::apply_boot_stage(&mut tab, BootStage::AgentReady, None);
    assert!(queued_delivery_target(&tab, &payload).is_ok());
    tab.session_binding_nonce = Some("foreign-launch".into());
    assert!(!payload_matches_binding(
        &payload,
        &delivery_binding(&tab).unwrap()
    ));
}
