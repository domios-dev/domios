use super::*;
use crate::service::launch_readiness::{LaunchEvidence, LaunchSettlement};
use crate::service::test_fixture::ServiceFixture;
use std::time::Duration;

fn fixture() -> (ServiceFixture, String, LaunchGeneration) {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.pty_id = Some("owned-pty".into());
    tab.agent_type = Some("opaque-provider".into());
    tab.agent_session_id = Some("owned-session".into());
    tab.session_binding_nonce = Some("owned-nonce".into());
    tab.reported_session_binding_nonce = Some("owned-nonce".into());
    tab.agent_booting = true;
    tab.boot_stage = Some(BootStage::LaunchWritten);
    let fixture = ServiceFixture::new(mux);
    let mut launch = LaunchGeneration::begin(&tab_id, "owned-pty".into());
    launch.binding_nonce = Some("owned-nonce".into());
    assert!(LAUNCH_REGISTRY
        .lock()
        .bind_provider_session(&launch, "owned-nonce".into()));
    (fixture, tab_id, launch)
}

#[test]
fn ready_settlement_reconciles_exact_tab_binding_before_delayed_hook_callback() {
    let (fixture, tab_id, mut launch) = fixture();
    assert!(!launch.await_provider_ack(Duration::ZERO));
    let settlement = fixture
        .service
        .settle_launch_readiness(&launch.attempt().unwrap(), LaunchEvidence::Acknowledged);
    assert_eq!(settlement, LaunchSettlement::Ready);
    assert!(!launch.await_provider_ack(Duration::ZERO));
    super::super::startup_readiness::emit_agent_ready(&mut launch, &fixture.service, settlement)
        .unwrap();
    assert!(launch.await_provider_ack(Duration::ZERO));
    let tab = fixture.service.mux.read().get_tab(&tab_id).unwrap().clone();
    assert_eq!(tab.boot_stage, Some(BootStage::AgentReady));
    assert!(tab.boot_failure_reason.is_none());
    LAUNCH_REGISTRY.lock().finish(&launch);
}

#[test]
fn binding_reconciliation_rejects_missing_foreign_and_rotated_proof() {
    for invalid in [
        "missing-pty",
        "foreign-pty",
        "missing-session",
        "empty-session",
        "missing-provider",
        "unreported-nonce",
        "foreign-report",
        "rotated-nonce",
    ] {
        let (fixture, tab_id, launch) = fixture();
        {
            let mut mux = fixture.service.mux.write();
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            match invalid {
                "missing-pty" => tab.pty_id = None,
                "foreign-pty" => tab.pty_id = Some("foreign-pty".into()),
                "missing-session" => tab.agent_session_id = None,
                "empty-session" => tab.agent_session_id = Some(String::new()),
                "missing-provider" => tab.agent_type = None,
                "unreported-nonce" => tab.reported_session_binding_nonce = None,
                "foreign-report" => {
                    tab.reported_session_binding_nonce = Some("foreign-nonce".into())
                }
                "rotated-nonce" => {
                    tab.session_binding_nonce = Some("new-nonce".into());
                    tab.reported_session_binding_nonce = Some("new-nonce".into());
                }
                _ => unreachable!(),
            }
        }
        assert!(
            !launch.reconcile_authenticated_binding(&fixture.service),
            "{invalid}"
        );
        assert!(!launch.await_provider_ack(Duration::ZERO), "{invalid}");
        LAUNCH_REGISTRY.lock().finish(&launch);
    }
}

#[test]
fn authenticated_tab_cannot_reconcile_a_superseded_generation_on_the_same_pty() {
    let (fixture, tab_id, old) = fixture();
    let mut replacement = LaunchGeneration::begin(&tab_id, "owned-pty".into());
    replacement.binding_nonce = Some("owned-nonce".into());
    assert!(LAUNCH_REGISTRY
        .lock()
        .bind_provider_session(&replacement, "owned-nonce".into()));
    assert!(!old.reconcile_authenticated_binding(&fixture.service));
    assert!(!old.await_provider_ack(Duration::ZERO));
    assert!(!replacement.await_provider_ack(Duration::ZERO));
    assert!(replacement.reconcile_authenticated_binding(&fixture.service));
    assert!(replacement.await_provider_ack(Duration::ZERO));
    LAUNCH_REGISTRY.lock().finish(&replacement);
}

#[test]
fn ready_settlement_without_authenticated_tab_proof_still_fails() {
    let (fixture, tab_id, mut launch) = fixture();
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .reported_session_binding_nonce = None;
    let error = super::super::startup_readiness::emit_agent_ready(
        &mut launch,
        &fixture.service,
        LaunchSettlement::Ready,
    )
    .unwrap_err();
    assert!(error.starts_with("SessionBindingNotObserved:"));
    assert!(!launch.await_provider_ack(Duration::ZERO));
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .unwrap()
            .boot_stage,
        Some(BootStage::SpawnFailed)
    );
}
