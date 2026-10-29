use super::*;
use crate::service::launch_readiness::LaunchAttempt;
use crate::service::test_fixture::ServiceFixture;

#[test]
fn fresh_and_resume_restart_wait_accept_exact_composer_without_binding() {
    for session in [None, Some("owned-resume-session")] {
        let (mut mux, id) = crate::multiplexer::state::MultiplexerState::new();
        let tab = mux.get_tab_mut(&id).unwrap();
        tab.pty_id = Some("replacement-pty".into());
        tab.session_binding_nonce = Some("replacement-nonce".into());
        tab.agent_session_id = session.map(str::to_string);
        tab.boot_stage = Some(BootStage::ProviderReady);
        let fixture = ServiceFixture::new(mux);
        let attempt = LaunchAttempt::current(&fixture.service.mux, &id, "replacement-pty").unwrap();
        fixture
            .service
            .record_first_turn_input_ready(&attempt, "opaque composer evidence");
        for stage in [
            BootStage::LaunchDeadlineElapsed,
            BootStage::LaunchBindingUnconfirmed,
        ] {
            fixture
                .service
                .record_provisional_launch(&attempt, stage, "binding deadline");
        }
        assert_eq!(
            wait_for_restart_readiness(
                &fixture.service.mux,
                &id,
                Some("old-pty"),
                std::time::Duration::ZERO
            ),
            RestartReadiness::Ready {
                pty_id: "replacement-pty".into()
            }
        );
        let tab = fixture.service.mux.read().get_tab(&id).unwrap().clone();
        assert_eq!(tab.boot_stage, Some(BootStage::ProviderReady));
        assert!(tab.boot_failure_reason.is_none());
        assert_eq!(tab.agent_session_id.as_deref(), session);
        assert!(!tab.launch_session_authenticated());
        assert!(tab.pending_delivery.is_none());
        let mut written = tab.clone();
        written.provisional_launch.as_mut().unwrap().first_turn_payload_written = true;
        assert!(first_turn_input_is_ready(&written));
        assert!(classify_tab_restart_readiness(&tab, Some("replacement-pty")).is_none());
        for change in 0..5 {
            let mut stale = tab.clone();
            match change {
                0 => stale.pty_id = Some("foreign-pty".into()),
                1 => stale.session_binding_nonce = Some("foreign-nonce".into()),
                2 => stale.agent_booting = true,
                3 => stale.exit_suspected_at = Some(std::time::Instant::now()),
                _ => stale.frozen = true,
            }
            assert!(classify_tab_restart_readiness(&stale, Some("old-pty")).is_none());
        }
    }
}
