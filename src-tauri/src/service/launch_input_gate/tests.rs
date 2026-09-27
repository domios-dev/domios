use super::*;
use crate::agent::test_provider::MockProvider;

fn launch_classification(state: TabState) -> Option<TabStateClassification> {
    Some(TabStateClassification {
        state,
        confidence: 1.0,
        data: None,
        action: None,
    })
}

fn input_observation() -> LaunchInputObservation {
    LaunchInputObservation {
        session_bound: false,
        composer_ready: true,
        pending_prompt: false,
        provider_process_alive: true,
        directory_trust_prompt: false,
        active_interactive_prompt: false,
        classification: None,
    }
}

#[test]
fn launch_gate_allows_unknown_provider_state_with_readiness_evidence() {
    assert_eq!(
        decide_launch_input(
            LaunchInputObservation {
                session_bound: true,
                composer_ready: false,
                classification: launch_classification(TabState::Unknown),
                ..input_observation()
            },
            LaunchInputMode::Standard,
        ),
        Ok(Some(LaunchInputEvidence::SessionBinding)),
    );
    assert_eq!(
        decide_launch_input(input_observation(), LaunchInputMode::Standard),
        Ok(Some(LaunchInputEvidence::ProviderComposer)),
    );
}

#[test]
fn launch_gate_refuses_provider_and_pending_prompt_dialogs() {
    for state in [
        TabState::UpdatePrompt,
        TabState::WaitingOnAuth,
        TabState::RateLimited,
        TabState::ClarifyingQuestion,
    ] {
        let refusal = decide_launch_input(
            LaunchInputObservation {
                classification: launch_classification(state),
                active_interactive_prompt: state == TabState::ClarifyingQuestion,
                ..input_observation()
            },
            LaunchInputMode::Standard,
        )
        .expect_err("classified provider dialog blocks launch input");
        assert_eq!(refusal, LaunchInputRefusal::ProviderDialog(state));
    }

    let refusal = decide_launch_input(
        LaunchInputObservation {
            pending_prompt: true,
            ..input_observation()
        },
        LaunchInputMode::Standard,
    )
    .expect_err("pending provider prompt blocks launch input");
    assert_eq!(refusal, LaunchInputRefusal::PendingPrompt);

    let refusal = decide_launch_input(
        LaunchInputObservation {
            directory_trust_prompt: true,
            ..input_observation()
        },
        LaunchInputMode::Standard,
    )
    .expect_err("provider directory-trust modal blocks launch input");
    assert_eq!(refusal, LaunchInputRefusal::DirectoryTrustPrompt);
}

#[test]
fn lazy_session_start_blocks_every_classified_modal() {
    for state in [
        TabState::UpdatePrompt,
        TabState::WaitingOnAuth,
        TabState::RateLimited,
        TabState::ClarifyingQuestion,
    ] {
        assert_eq!(
            decide_launch_input(
                LaunchInputObservation {
                    classification: launch_classification(state),
                    ..input_observation()
                },
                LaunchInputMode::LazySessionStart,
            ),
            Ok(None),
        );
    }
}

#[test]
fn standard_gate_keeps_transcript_questions_nonblocking() {
    assert_eq!(
        decide_launch_input(
            LaunchInputObservation {
                session_bound: true,
                composer_ready: false,
                classification: launch_classification(TabState::ClarifyingQuestion),
                ..input_observation()
            },
            LaunchInputMode::Standard,
        ),
        Ok(Some(LaunchInputEvidence::SessionBinding)),
    );
}

#[test]
fn lazy_session_start_requires_composer_live_process_and_no_prompt() {
    assert_eq!(
        decide_launch_input(input_observation(), LaunchInputMode::LazySessionStart),
        Ok(Some(LaunchInputEvidence::ProviderComposer)),
    );

    for blocked in [
        LaunchInputObservation {
            provider_process_alive: false,
            ..input_observation()
        },
        LaunchInputObservation {
            pending_prompt: true,
            ..input_observation()
        },
        LaunchInputObservation {
            directory_trust_prompt: true,
            ..input_observation()
        },
        LaunchInputObservation {
            classification: launch_classification(TabState::UpdatePrompt),
            ..input_observation()
        },
    ] {
        assert_eq!(
            decide_launch_input(blocked, LaunchInputMode::LazySessionStart),
            Ok(None),
            "blocked lazy launch remains unready until its bounded deadline",
        );
    }

    assert_eq!(
        decide_launch_input(
            LaunchInputObservation {
                composer_ready: false,
                session_bound: true,
                ..input_observation()
            },
            LaunchInputMode::LazySessionStart,
        ),
        Ok(None),
        "a live provider without its declared composer is not ready",
    );
}

#[test]
fn lazy_composer_readiness_requires_declared_structural_capability() {
    assert!(provider_composer_is_ready(
        LaunchInputMode::LazySessionStart,
        true,
        true,
        false,
    ));
    assert!(!provider_composer_is_ready(
        LaunchInputMode::LazySessionStart,
        false,
        true,
        true,
    ));
    assert!(!provider_composer_is_ready(
        LaunchInputMode::LazySessionStart,
        true,
        false,
        false,
    ));
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
fn lazy_session_spawn_with_modal_or_dead_provider_still_fails_at_deadline() {
    fn await_blocked_launch(
        classification: Option<TabState>,
    ) -> Result<LaunchInputEvidence, LaunchInputRefusal> {
        let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
        let pty_id = "blocked-lazy-session-pty";
        let nonce = "blocked-lazy-session-nonce";
        {
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            tab.agent_type = Some("blocked-lazy-session-fixture".into());
            tab.session_binding_nonce = Some(nonce.into());
            tab.pty_id = Some(pty_id.into());
        }
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        let provider = match classification {
            Some(state) => MockProvider::new("blocked-lazy-session-fixture").with_tab_state(
                TabStateClassification {
                    state,
                    confidence: 1.0,
                    data: None,
                    action: None,
                },
            ),
            None => MockProvider::new("blocked-lazy-session-fixture"),
        };
        fixture
            .service
            .agent_registry
            .register_provider(Box::new(provider));
        let attempt = LaunchAttempt {
            tab_id,
            pty_id: pty_id.into(),
            binding_nonce: nonce.into(),
        };

        fixture.service.await_lazy_session_input_readiness(
            &attempt,
            "blocked-lazy-session-fixture",
            std::time::Duration::ZERO,
        )
    }

    assert_eq!(
        await_blocked_launch(Some(TabState::UpdatePrompt)),
        Err(LaunchInputRefusal::Deadline),
    );
    assert_eq!(
        await_blocked_launch(None),
        Err(LaunchInputRefusal::Deadline),
    );
}
