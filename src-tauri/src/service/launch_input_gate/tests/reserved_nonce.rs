use super::*;
use crate::agent::provider::ProviderReadinessStrategy;
use crate::service::boot_stage::{apply_boot_stage, BootStage};

#[test]
fn fresh_lazy_provider_with_reserved_nonce_reaches_composer_readiness() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("boxed-composer-provider".into());
    tab.agent_booting = true;
    tab.session_binding_nonce = Some("reserved-before-registration".into());
    tab.pty_id = Some("registered-pty".into());
    let attempt = LaunchAttempt {
        tab_id,
        pty_id: "registered-pty".into(),
        binding_nonce: "reserved-before-registration".into(),
    };
    assert!(attempt_owns_tab(tab, &attempt));

    let mut screens = codeterm_pty::pty::screen_state::ScreenState::new();
    screens.resize(&attempt.pty_id, 120, 40);
    let screen = include_str!("fixtures/boxed-idle-composer.txt")
        .lines()
        .collect::<Vec<_>>()
        .join("\r\n");
    screens.process(
        &attempt.pty_id,
        format!("\x1b[?1049h\x1b[2J\x1b[H{screen}\x1b[3;5H\x1b[?25h").as_bytes(),
    );
    let input = screens.composer_input_state(&attempt.pty_id).unwrap();
    let terminal_ready =
        launch_composer_input_ready(&input, ComposerCursorPosition::VisibleAnywhere, None, 1);
    let mode = LaunchInputMode::LazySessionStart;
    assert!(!provider_composer_is_ready(
        mode,
        false,
        terminal_ready,
        true
    ));
    let provider = MockProvider::new("boxed-composer-provider").with_provider_readiness_strategy(
        ProviderReadinessStrategy::StructuralComposer {
            timeout: std::time::Duration::from_secs(30),
        },
    );
    let composer_ready = provider_composer_is_ready(
        mode,
        provider.provider_readiness_strategy().requires_composer(),
        terminal_ready,
        true,
    );
    assert_eq!(
        decide_launch_input(
            LaunchInputObservation {
                composer_ready,
                ..input_observation()
            },
            mode,
        ),
        Ok(Some(LaunchInputEvidence::ProviderComposer)),
    );
    apply_boot_stage(tab, BootStage::AgentReady, None);
    assert_eq!(tab.boot_stage, Some(BootStage::AgentReady));
    assert!(tab.agent_session_id.is_none());
    assert!(tab.reported_session_binding_nonce.is_none());
    assert!(attempt_owns_tab(tab, &attempt));
    tab.session_binding_nonce = Some("replacement-launch".into());
    assert!(!attempt_owns_tab(tab, &attempt));
}
