use super::*;
use crate::agent::test_provider::MockProvider;
use crate::multiplexer::state::MultiplexerState;
use crate::service::boot_stage::BootStage;
use crate::service::launch_readiness::LaunchAttempt;
use crate::service::test_fixture::ServiceFixture;
use crate::service::turn_resume::{self, Step};

fn deadline_fixture() -> (ServiceFixture, String) {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("late-hook-fixture".into());
    tab.agent_session_id = Some("own-session".into());
    tab.session_binding_nonce = Some("own-nonce".into());
    tab.pty_id = Some("own-pty".into());
    tab.agent_booting = true;
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(MockProvider::new("late-hook-fixture")));
    fixture.service.record_provisional_launch(
        &LaunchAttempt {
            tab_id: tab_id.clone(),
            pty_id: "own-pty".into(),
            binding_nonce: "own-nonce".into(),
        },
        BootStage::LaunchDeadlineElapsed,
        "dropped SessionStart",
    );
    (fixture, tab_id)
}

#[tokio::test]
async fn late_verified_hook_retires_deadline_and_claims_owed_continue_once() {
    let (fixture, tab_id) = deadline_fixture();
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        crate::multiplexer::types::request_continue(
            &mut tab.turn_resume,
            "own-session",
            "old-start",
            "opaque-continue".into(),
        );
    }
    assert_eq!(
        turn_resume::step_for_service(&fixture.service, &tab_id, "new-start"),
        Step::Waiting
    );
    recover_verified_hook_binding(
        &fixture.service,
        &tab_id,
        None,
        Some("own-session"),
        Some("own-nonce"),
    )
    .await
    .unwrap();
    {
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&tab_id).unwrap();
        assert_eq!(tab.boot_stage, Some(BootStage::AgentReady));
        assert!(!tab.agent_booting);
        assert!(tab.boot_failure_reason.is_none());
        assert!(tab.provisional_launch.is_none());
        assert!(tab.launch_session_authenticated());
    }
    let Step::Claimed(claim) =
        turn_resume::step_for_service(&fixture.service, &tab_id, "new-start")
    else {
        panic!("owed continuation must be claimed");
    };
    assert_eq!(claim.prompt.as_deref(), Some("opaque-continue"));
    recover_verified_hook_binding(
        &fixture.service,
        &tab_id,
        None,
        Some("own-session"),
        Some("own-nonce"),
    )
    .await
    .unwrap();
    assert_eq!(
        turn_resume::step_for_service(&fixture.service, &tab_id, "new-start"),
        Step::Settled
    );
    let snapshot = fixture.service.mux.read().to_session_snapshot();
    let restored = MultiplexerState::from_session_snapshot(&snapshot);
    assert_eq!(
        restored
            .get_tab(&tab_id)
            .unwrap()
            .turn_resume
            .as_ref()
            .unwrap()
            .settled
            .as_ref()
            .unwrap()
            .outcome,
        crate::multiplexer::types::TurnResumeOutcome::Sent
    );
}

#[tokio::test]
async fn late_hook_requires_current_nonce_provider_and_pty() {
    for (provider, nonce, pty) in [
        (None, None, true),
        (None, Some("prior-nonce"), true),
        (Some("foreign-provider"), Some("own-nonce"), true),
        (None, Some("own-nonce"), false),
    ] {
        let (fixture, tab_id) = deadline_fixture();
        if !pty {
            fixture
                .service
                .mux
                .write()
                .get_tab_mut(&tab_id)
                .unwrap()
                .pty_id = None;
        }
        let _ = recover_verified_hook_binding(
            &fixture.service,
            &tab_id,
            provider,
            Some("own-session"),
            nonce,
        )
        .await;
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&tab_id).unwrap();
        assert_eq!(tab.boot_stage, Some(BootStage::LaunchDeadlineElapsed));
        assert!(!tab.launch_session_authenticated());
        assert!(tab.provisional_launch.is_some());
    }
}

#[tokio::test]
async fn late_verified_hook_without_session_uses_only_its_tabs_retained_session() {
    let (fixture, tab_id) = deadline_fixture();
    recover_verified_hook_binding(&fixture.service, &tab_id, None, None, Some("own-nonce"))
        .await
        .unwrap();
    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&tab_id).unwrap();
    assert_eq!(tab.agent_session_id.as_deref(), Some("own-session"));
    assert_eq!(tab.boot_stage, Some(BootStage::AgentReady));
}
