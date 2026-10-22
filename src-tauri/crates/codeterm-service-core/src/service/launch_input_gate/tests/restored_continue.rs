use super::*;
use crate::lock_hold::TimedRwLock;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::request_continue;
use crate::service::boot_stage::BootStage;
use crate::service::turn_resume::{RuntimeReadiness, Step};

fn step(mux: &crate::multiplexer::commands::MuxState, tab_id: &str, start_epoch: &str) -> Step {
    crate::service::turn_resume::step(mux, tab_id, start_epoch, RuntimeReadiness::Unsupported)
}
use std::sync::Arc;

#[test]
fn restored_owed_continue_waits_for_first_composer_then_claims_once() {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("sqlite-lazy-fixture".into());
    tab.agent_session_id = Some("own-saved-session".into());
    tab.pty_id = Some("restored-pty".into());
    tab.session_binding_nonce = Some("current-nonce".into());
    tab.reported_session_binding_nonce = Some("current-nonce".into());
    tab.boot_stage = Some(BootStage::ProviderReady);
    request_continue(
        &mut tab.turn_resume,
        "own-saved-session",
        "previous-start",
        "opaque-continue".into(),
    );
    let mux = Arc::new(TimedRwLock::new("restore-composer-test", mux));

    for session_bound in [false, true] {
        assert_eq!(
            decide_launch_input(
                LaunchInputObservation {
                    session_bound,
                    composer_ready: false,
                    ..input_observation()
                },
                LaunchInputMode::LazySessionStart,
            ),
            Ok(None),
        );
        assert_eq!(step(&mux, &tab_id, "current-start"), Step::Waiting);
    }
    assert_eq!(
        decide_launch_input(
            LaunchInputObservation {
                session_bound: true,
                ..input_observation()
            },
            LaunchInputMode::LazySessionStart,
        ),
        Ok(Some(LaunchInputEvidence::ProviderComposer)),
    );
    mux.write().get_tab_mut(&tab_id).unwrap().boot_stage = Some(BootStage::AgentReady);
    let Step::Claimed(claim) = step(&mux, &tab_id, "current-start") else {
        panic!("composer-ready own session must release the owed original");
    };
    assert_eq!(claim.session_id, "own-saved-session");
    assert_eq!(claim.pty_id.as_deref(), Some("restored-pty"));
    assert_eq!(claim.binding_nonce, "current-nonce");
    assert_eq!(claim.prompt.as_deref(), Some("opaque-continue"));
    assert_eq!(step(&mux, &tab_id, "current-start"), Step::Settled);
}

#[test]
fn restored_retained_session_composer_evidence_releases_one_queued_send() {
    use crate::agent::provider::ProviderReadinessStrategy;
    use crate::agent::test_provider::MockProvider;
    use crate::service::parked_user_delivery::test_support::{
        attach_fixture_pty, await_owner_idle, queue_fixture, UserSubmitBoundary,
    };
    use codeterm_types::pty::BootStage;
    use std::sync::mpsc;
    use std::time::Duration;

    let (fixture, tab_id) = queue_fixture(true, None);
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.boot_stage = Some(BootStage::PtyStarting);
        tab.restored_from_snapshot = true;
        tab.agent_state = Some(
            codeterm_types::pty::TabVerdict {
                state: codeterm_types::pty::TimelineState::Idle,
                subtype: None,
            }
            .into(),
        );
    }
    fixture.service.agent_registry.register_provider(Box::new(
        MockProvider::new("opaque-provider")
            .with_session_starts_on_first_user_turn()
            .with_provider_readiness_strategy(ProviderReadinessStrategy::StructuralComposer {
                timeout: Duration::from_secs(30),
            }),
    ));
    attach_fixture_pty(&fixture, &tab_id);
    let authority = fixture.service.user_send_authority(&tab_id).unwrap();
    let (writes_tx, writes_rx) = mpsc::channel();
    let _boundary = UserSubmitBoundary::install(
        &tab_id,
        Arc::new(move |_, text| {
            writes_tx.send(text.to_owned()).unwrap();
            Ok(())
        }),
    );
    let queued = fixture
        .service
        .reserve_user_send(&tab_id, None)
        .and_then(|reservation| {
            fixture.service.prepare_and_admit_user_send(
                reservation,
                crate::service::messenger::HumanInput::composer("opaque queued send"),
                false,
            )
        })
        .unwrap()
        .status;
    await_owner_idle(&fixture, &tab_id);
    assert!(!fixture.service.user_authority_is_ready(&authority));
    assert_eq!(
        fixture
            .service
            .pending_delivery_payloads
            .user_sends
            .records(&tab_id)
            .unwrap()
            .iter()
            .map(|record| record.send_id.clone())
            .collect::<Vec<_>>(),
        vec![queued.send_id.clone()],
    );
    assert!(matches!(
        writes_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));

    let mut observation = input_observation();
    observation.session_bound = true;
    observation.composer_ready = false;
    assert_eq!(
        decide_launch_input(observation, LaunchInputMode::LazySessionStart),
        Ok(None),
        "the restored own-session binding does not bypass composer readiness",
    );
    assert!(!fixture.service.user_authority_is_ready(&authority));

    let mut observation = input_observation();
    observation.session_bound = true;
    assert_eq!(
        decide_launch_input(observation, LaunchInputMode::LazySessionStart),
        Ok(Some(LaunchInputEvidence::ProviderComposer)),
        "the retained launch becomes input ready from structural composer evidence",
    );
    assert_eq!(
        crate::service::orchestrator::launch_waits_for_first_turn_binding(
            &fixture.service,
            &tab_id,
            "opaque-provider",
            false,
            crate::service::launch_diagnostics::AgentLaunchOrigin::RestoredSnapshot,
        ),
        false,
        "the retained session does not wait for a first user turn to bind",
    );

    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .reported_session_binding_nonce = Some(authority.binding_nonce.clone());
    fixture
        .service
        .emit_boot_stage(&tab_id, BootStage::AgentReady, None);
    {
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&tab_id).unwrap();
        assert_eq!(tab.boot_stage, Some(BootStage::AgentReady));
        assert_eq!(
            tab.agent_state.as_ref().map(|state| state.state),
            Some(codeterm_types::pty::TimelineState::Idle),
            "restoring readiness preserves the observed idle state without a user turn",
        );
    }
    assert_eq!(
        writes_rx.recv_timeout(Duration::from_secs(10)).unwrap(),
        "opaque queued send",
    );
    await_owner_idle(&fixture, &tab_id);
    fixture.service.wake_parked_user_send(&tab_id);
    await_owner_idle(&fixture, &tab_id);
    assert!(matches!(
        writes_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    assert!(fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .records(&tab_id)
        .unwrap()
        .is_empty());
}
