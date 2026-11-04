use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::service::messenger::{Delivery, MessageEnvelope, Messenger};
use crate::service::test_fixture::ServiceFixture;
use codeterm_types::pty::BootStage;

fn detected(
    provider: &str,
    nonce: &str,
    session: &str,
    source: LaunchSessionSource,
) -> DetectedLaunchSession {
    DetectedLaunchSession {
        provider_id: provider.into(),
        binding_nonce: nonce.into(),
        detection: LaunchSessionDetection {
            session_id: session.into(),
            source,
        },
    }
}

#[test]
fn resumed_pid_evidence_requires_the_exact_provider_launch_and_claimed_session() {
    assert!(detected_resume_matches(
        &detected("own", "nonce", "session", LaunchSessionSource::PidRegistry),
        "own",
        "nonce",
        "session"
    ));
    for (provider, nonce, session, source) in [
        (
            "other",
            "nonce",
            "session",
            LaunchSessionSource::PidRegistry,
        ),
        ("own", "old", "session", LaunchSessionSource::PidRegistry),
        ("own", "nonce", "sibling", LaunchSessionSource::PidRegistry),
        ("own", "nonce", "session", LaunchSessionSource::LaunchMarker),
    ] {
        assert!(!detected_resume_matches(
            &detected(provider, nonce, session, source),
            "own",
            "nonce",
            "session"
        ));
    }
}

#[tokio::test]
async fn resumed_pid_binding_releases_two_held_payloads_once_without_manual_input() {
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("opaque-provider".into());
    tab.agent_session_id = Some("saved-session".into());
    tab.pty_id = Some("own-pty".into());
    tab.session_binding_nonce = Some("current-launch".into());
    tab.boot_stage = Some(BootStage::LaunchWritten);
    let fixture = ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::with_session("opaque-provider", "saved-session")
            .with_hooks(),
    ));
    let attempt = crate::service::launch_readiness::LaunchAttempt {
        tab_id: id.clone(),
        pty_id: "own-pty".into(),
        binding_nonce: "current-launch".into(),
    };
    fixture
        .service
        .record_launch_output_deadline(&attempt, "binding pending");
    let writes = fixture.record_agent_deliveries(&id);
    for body in ["first opaque fragment", "second opaque fragment"] {
        assert!(matches!(
            Messenger::deliver(
                &fixture.service,
                &id,
                MessageEnvelope::tab("sender", "sender-tab"),
                body
            )
            .unwrap(),
            Delivery::Pending { .. }
        ));
    }
    fixture.service.wake_agent_deliveries_on_state_change(&id);
    writes.wait_for_flush();
    assert!(writes.texts.lock().is_empty());
    let exact = detected(
        "opaque-provider",
        "current-launch",
        "saved-session",
        LaunchSessionSource::PidRegistry,
    );
    assert!(detected_resume_matches(
        &exact,
        "opaque-provider",
        "current-launch",
        "saved-session"
    ));
    fixture
        .service
        .commit_detected_launch_session(&id, exact)
        .await;
    writes.wait_for_flush();
    assert_eq!(
        *writes.texts.lock(),
        ["first opaque fragment", "second opaque fragment"]
    );
    assert!(!fixture.has_pending_agent_delivery(&id));
    {
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&id).unwrap();
        assert!(tab.launch_session_authenticated());
        assert_eq!(tab.boot_stage, Some(BootStage::AgentReady));
    }
    fixture.service.reconcile_authenticated_agent_input(&id);
    writes.wait_for_flush();
    assert_eq!(
        *writes.texts.lock(),
        ["first opaque fragment", "second opaque fragment"]
    );
}
