use super::*;
use crate::agent::test_provider::MockProvider;
use crate::service::test_fixture::ServiceFixture;

#[test]
fn sessionless_model_switch_prepares_fresh_model_and_fences_first_send_or_replacement() {
    const IDENTITY: &str = "opaque-sessionless-model-identity";
    for (general, system_prompt) in [(false, false), (false, true), (true, true)] {
        let (mut mux, mut id) = crate::multiplexer::state::MultiplexerState::new();
        if general {
            crate::service::general_agent::ensure_general_workspace(&mut mux, true);
            id = mux
                .resolve_tab_id(crate::service::general_agent::GENERAL_ALIAS)
                .unwrap();
        }
        let tab = mux.get_tab_mut(&id).unwrap();
        tab.agent_type = Some("lazy-fixture".into());
        tab.pty_id = Some("owned-pty".into());
        tab.session_binding_nonce = Some("owned-nonce".into());
        tab.cwd = Some(std::env::temp_dir().display().to_string());
        tab.boot_stage = Some(crate::service::boot_stage::BootStage::ProviderReady);
        if system_prompt {
            tab.restorable_launch.captured = true;
            tab.restorable_launch.identity_instructions = Some(IDENTITY.into());
        }
        let fixture = ServiceFixture::new(mux);
        let mut provider = MockProvider::new("lazy-fixture")
            .with_session_starts_on_first_user_turn()
            .with_model_switch(ModelSwitchMode::RestartResume);
        if system_prompt {
            provider = provider.with_session_config_delivery();
        }
        fixture
            .service
            .agent_registry
            .register_provider(Box::new(provider));
        let attempt = crate::service::launch_readiness::LaunchAttempt::current(
            &fixture.service.mux,
            &id,
            "owned-pty",
        )
        .unwrap();
        fixture
            .service
            .record_first_turn_input_ready(&attempt, "opaque composer evidence");
        let snap = fixture.service.snapshot_tab_restart_state(&id).unwrap();
        assert!(fixture.service.sessionless_model_switch_ready(&snap));
        let launch =
            ModelSwitchLaunch::prepare(&fixture.service, &snap, "", "vendor/next-model").unwrap();
        assert_eq!(launch.params.model.as_deref(), Some("vendor/next-model"));
        assert!(launch.params.resume_session_id.is_none());
        let expected_identity = system_prompt.then_some(IDENTITY);
        assert_eq!(launch.task.as_deref(), expected_identity);
        assert_eq!(launch.params.system_prompt.as_deref(), expected_identity);
        assert!(launch.params.task.is_none());
        assert_eq!(
            launch.params.starter_prompt,
            crate::agent::provider::StarterPrompt::NoStarter
        );
        let startup = crate::service::startup_turn::resolve(
            &fixture.service.agent_registry,
            &snap.provider,
            launch.params.starter_prompt,
            None,
            launch.task.as_deref(),
            true,
            None,
        );
        assert!(!startup.first_turn_will_be_delivered);
        assert!(startup.expected_turn.is_none());
        assert!(launch.sessionless_is_current(&fixture.service, &snap));
        let command = launch.command(&fixture.service, &snap).unwrap().0;
        assert!(command.contains("vendor/next-model"));
        launch.commit(&fixture.service, &id);
        let tab = fixture.service.mux.read().get_tab(&id).unwrap().clone();
        assert_eq!(
            tab.restorable_launch.model.as_deref(),
            Some("vendor/next-model")
        );
        assert!(tab.pending_model_switch_observation.is_none());
        for change in 0..5 {
            let mut changed = tab.clone();
            match change {
                0 => changed.session_binding_nonce = Some("new-nonce".into()),
                1 => changed.pty_id = Some("new-pty".into()),
                2 => changed.agent_session_id = Some("created-session".into()),
                3 => {
                    changed
                        .provisional_launch
                        .as_mut()
                        .unwrap()
                        .first_turn_admitted_send_id = Some("user-send".into())
                }
                _ => {
                    changed
                        .provisional_launch
                        .as_mut()
                        .unwrap()
                        .first_turn_payload_written = true
                }
            }
            *fixture.service.mux.write().get_tab_mut(&id).unwrap() = changed;
            assert!(!launch.sessionless_is_current(&fixture.service, &snap));
        }
    }
}
