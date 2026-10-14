use super::*;

#[test]
fn failed_launch_retains_provider_for_relaunch_after_identity_cleanup() {
    for provider in ["fixture-plain", "fixture-structured", "fixture-tool-less"] {
        let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some(provider.into());
        tab.boot_stage = Some(BootStage::SpawnFailed);
        assert_eq!(failed_provider(tab), Some(provider));
        assert!(crate::service::tab::clear_agent_identity_for_lifecycle_exit(tab));
        assert!(tab.agent_type.is_none());
        assert_eq!(failed_provider(tab), Some(provider));
        tab.agent_booting = true;
        assert_eq!(failed_provider(tab), None);
    }
}

#[test]
fn ordinary_exited_shell_is_not_a_failed_launch() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("fixture-provider".into());
    tab.boot_stage = Some(BootStage::ShellReady);
    assert_eq!(failed_provider(tab), None);
}

#[test]
fn failed_launch_without_a_pty_prepares_a_fresh_restart() {
    for provider in ["fixture-plain", "fixture-structured", "fixture-tool-less"] {
        let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some(provider.into());
        tab.boot_stage = Some(BootStage::SpawnFailed);
        crate::service::tab::clear_agent_identity_for_lifecycle_exit(tab);
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        fixture.set_agent_full_access(false);
        fixture.service.agent_registry.register_provider(Box::new(
            crate::agent::test_provider::MockProvider::new(provider),
        ));
        let restart = fixture
            .service
            .failed_launch_restart_snapshot(&tab_id)
            .unwrap()
            .unwrap();
        assert_eq!(restart.provider, provider);
        assert_eq!(restart.tab_id, tab_id);
        assert!(restart.pty_id.is_empty());
        assert!(restart.session_id.is_none());
    }
}
