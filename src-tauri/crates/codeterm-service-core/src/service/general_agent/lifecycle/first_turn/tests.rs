use super::*;
use crate::agent::provider::{
    AgentLaunchParams, AgentProvider, ComposerCursorPosition, ProviderReadinessStrategy,
    ReadinessStrategy, StarterPrompt, SystemPromptDelivery, TaskDelivery,
};
use crate::multiplexer::state::MultiplexerState;
use crate::service::boot_stage::BootStage;
use crate::service::test_fixture::ServiceFixture;
use std::path::PathBuf;
use std::time::Duration;

struct ComposerProvider {
    live_tui: bool,
    structural: bool,
}

impl AgentProvider for ComposerProvider {
    fn id(&self) -> &str {
        "lazy-fixture"
    }
    fn display_name(&self) -> &str {
        self.id()
    }
    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        String::new()
    }
    fn readiness_strategy(&self) -> ReadinessStrategy {
        ReadinessStrategy::FixedDelay {
            duration: Duration::ZERO,
        }
    }
    fn task_delivery(&self) -> TaskDelivery {
        TaskDelivery::PtyInput
    }
    fn detect_from_title(&self, _: &str) -> bool {
        false
    }
    fn detect_from_output(&self, _: &str) -> bool {
        false
    }
    fn session_file_path(&self, _: &str, _: &str) -> Option<PathBuf> {
        None
    }
    fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> {
        panic!("first-turn readiness cannot discover a session")
    }
    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }
    fn session_starts_on_first_user_turn(&self) -> bool {
        true
    }
    fn provider_readiness_strategy(&self) -> ProviderReadinessStrategy {
        if self.structural {
            ProviderReadinessStrategy::StructuralComposer {
                timeout: Duration::ZERO,
            }
        } else {
            ProviderReadinessStrategy::TuiFrame {
                timeout: Duration::ZERO,
            }
        }
    }
    fn composer_cursor_position(&self) -> ComposerCursorPosition {
        ComposerCursorPosition::VisibleAnywhere
    }
    fn screen_has_tui(&self, _: &str) -> bool {
        self.live_tui
    }
}

fn fixture(live_tui: bool, structural: bool, composer: bool) -> (ServiceFixture, LaunchAttempt) {
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("lazy-fixture".into());
    tab.pty_id = Some("owned-pty".into());
    tab.session_binding_nonce = Some("owned-nonce".into());
    tab.boot_stage = Some(BootStage::ProviderReady);
    tab.agent_booting = true;
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(ComposerProvider {
            live_tui,
            structural,
        }));
    if composer {
        let screen =
            include_str!("../../../launch_input_gate/tests/fixtures/boxed-idle-composer.txt")
                .lines()
                .collect::<Vec<_>>()
                .join("\r\n");
        fixture.service.screen_state.resize("owned-pty", 120, 40);
        fixture.service.screen_state.process(
            "owned-pty",
            format!("\x1b[?1049h\x1b[2J\x1b[H{screen}\x1b[3;5H\x1b[?25h").as_bytes(),
        );
    }
    let attempt = LaunchAttempt::current(&fixture.service.mux, &id, "owned-pty").unwrap();
    (fixture, attempt)
}

#[test]
fn fresh_and_resumed_lazy_general_without_bootstrap_wait_for_user_input() {
    assert!(awaiting_user_turn(true, false));
    assert!(!awaiting_user_turn(false, false));
    assert!(!awaiting_user_turn(true, true));
}

#[test]
fn fresh_general_prepare_uses_real_structural_gate_without_synthetic_turn() {
    let (fixture, attempt) = fixture(true, true, true);
    let instructions = super::super::compose_launch_instructions(
        "opaque-general-context".into(),
        None,
        &SystemPromptDelivery::SessionConfig,
    );
    assert_eq!(instructions.starter_prompt, StarterPrompt::NoStarter);
    assert!(instructions.startup_message.is_none());
    assert!(prepare(
        &fixture.service,
        &attempt,
        "lazy-fixture",
        false,
        instructions.starter_prompt != StarterPrompt::NoStarter
    ));
    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&attempt.tab_id).unwrap();
    assert!(!tab.agent_booting);
    assert!(tab.agent_session_id.is_none());
    assert!(tab.pending_delivery.is_none());
    let composer = tab.provisional_launch.as_ref().unwrap();
    assert!(composer.first_turn_input_ready);
    assert!(composer.awaiting_binding_ack);
    assert!(!composer.first_turn_payload_written);
    assert_eq!(composer.pty_id, attempt.pty_id);
    assert_eq!(composer.binding_nonce, attempt.binding_nonce);
}

#[test]
fn prepare_requires_declared_structural_composer_and_live_tui() {
    for (live_tui, structural, composer) in [
        (false, true, true),
        (true, false, true),
        (true, true, false),
    ] {
        let (fixture, attempt) = fixture(live_tui, structural, composer);
        assert!(prepare(
            &fixture.service,
            &attempt,
            "lazy-fixture",
            false,
            false
        ));
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&attempt.tab_id).unwrap();
        assert!(!tab.agent_booting);
        assert!(
            !tab.provisional_launch
                .as_ref()
                .unwrap()
                .first_turn_input_ready
        );
        assert!(tab.agent_session_id.is_none());
        assert!(tab.pending_delivery.is_none());
    }
}

#[test]
fn resumed_general_publishes_composer_readiness_without_a_turn_or_binding_deadline() {
    let (fixture, attempt) = fixture(true, true, true);
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&attempt.tab_id)
        .unwrap()
        .agent_session_id = Some("own-resume-session".into());
    assert!(prepare(
        &fixture.service,
        &attempt,
        "lazy-fixture",
        true,
        false
    ));
    fixture.service.record_provisional_launch(
        &attempt,
        BootStage::LaunchBindingUnconfirmed,
        "binding deadline",
    );
    let tab = fixture
        .service
        .mux
        .read()
        .get_tab(&attempt.tab_id)
        .unwrap()
        .clone();
    assert_eq!(tab.boot_stage, Some(BootStage::ProviderReady));
    assert!(crate::service::boot_stage::first_turn_input_is_ready(&tab));
    assert_eq!(tab.agent_session_id.as_deref(), Some("own-resume-session"));
    assert!(tab.pending_delivery.is_none());
    assert!(!tab.launch_session_authenticated());
}

#[test]
fn prepare_preserves_already_bound_session_and_planned_bootstrap() {
    for bound in [false, true] {
        let (fixture, attempt) = fixture(false, true, false);
        if bound {
            let mut mux = fixture.service.mux.write();
            let tab = mux.get_tab_mut(&attempt.tab_id).unwrap();
            tab.agent_session_id = Some("own-session".into());
            tab.reported_session_binding_nonce = Some(attempt.binding_nonce.clone());
        }
        let events = fixture.ui.0.lock().clone();
        assert!(!prepare(
            &fixture.service,
            &attempt,
            "lazy-fixture",
            false,
            !bound
        ));
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&attempt.tab_id).unwrap();
        assert!(tab.agent_booting);
        assert!(tab.provisional_launch.is_none());
        assert!(tab.pending_delivery.is_none());
        assert_eq!(*fixture.ui.0.lock(), events);
    }
}

#[test]
fn prepare_refuses_original_attempt_after_nonce_or_pty_replacement_without_effects() {
    for replace_pty in [false, true] {
        let (fixture, attempt) = fixture(true, true, true);
        let claim =
            super::super::AutomaticLaunchClaim::acquire(&fixture.service.mux, &attempt.tab_id)
                .unwrap();
        {
            let mut mux = fixture.service.mux.write();
            let tab = mux.get_tab_mut(&attempt.tab_id).unwrap();
            tab.session_binding_nonce = Some("replacement-nonce".into());
            if replace_pty {
                tab.pty_id = Some("replacement-pty".into());
            }
            tab.boot_stage = Some(BootStage::ProviderReady);
            tab.boot_failure_reason = Some("replacement-reason".into());
            tab.agent_booting = true;
        }
        let events = fixture.ui.0.lock().clone();
        assert!(prepare(
            &fixture.service,
            &attempt,
            "lazy-fixture",
            false,
            false
        ));
        drop(claim);
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&attempt.tab_id).unwrap();
        assert!(tab.agent_booting);
        assert_eq!(tab.boot_stage, Some(BootStage::ProviderReady));
        assert_eq!(
            tab.boot_failure_reason.as_deref(),
            Some("replacement-reason")
        );
        assert_eq!(
            tab.session_binding_nonce.as_deref(),
            Some("replacement-nonce")
        );
        assert_eq!(
            tab.pty_id.as_deref(),
            Some(if replace_pty {
                "replacement-pty"
            } else {
                "owned-pty"
            })
        );
        assert!(tab.provisional_launch.is_none());
        assert!(tab.pending_delivery.is_none());
        assert_eq!(*fixture.ui.0.lock(), events);
    }
}
