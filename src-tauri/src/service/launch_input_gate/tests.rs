use super::*;
use crate::agent::provider::AgentProvider;
use crate::agent::test_provider::MockProvider;

fn launch_classification(state: TabState) -> Option<TabStateClassification> {
    Some(TabStateClassification {
        state,
        confidence: 1.0,
        data: None,
        action: None,
    })
}

fn provider_fixture(name: &str) -> String {
    std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../plugins/provider-codex/tests/fixtures/directory-trust")
            .join(name),
    )
    .expect("provider dialog fixture")
}

#[test]
fn launch_gate_allows_unknown_provider_state_with_readiness_evidence() {
    assert_eq!(
        decide_launch_input(LaunchInputObservation {
            session_bound: true,
            composer_ready: false,
            directory_trust_prompt: false,
            active_interactive_prompt: false,
            classification: launch_classification(TabState::Unknown),
        }),
        Ok(Some(LaunchInputEvidence::SessionBinding)),
    );
    assert_eq!(
        decide_launch_input(LaunchInputObservation {
            session_bound: false,
            composer_ready: true,
            directory_trust_prompt: false,
            active_interactive_prompt: false,
            classification: None,
        }),
        Ok(Some(LaunchInputEvidence::ProviderComposer)),
    );
}

#[test]
fn launch_gate_refuses_provider_update_and_directory_trust_dialogs() {
    let update = provider_fixture("codex-update-available.txt");
    let trust = provider_fixture("trust-folder-access-0.157.1.txt");
    assert!(!update.is_empty());
    assert!(!trust.is_empty());

    for state in [
        TabState::UpdatePrompt,
        TabState::WaitingOnAuth,
        TabState::RateLimited,
    ] {
        let refusal = decide_launch_input(LaunchInputObservation {
            session_bound: true,
            composer_ready: true,
            directory_trust_prompt: false,
            active_interactive_prompt: false,
            classification: launch_classification(state),
        })
        .expect_err("provider dialog blocks launch input");
        assert_eq!(refusal, LaunchInputRefusal::ProviderDialog(state));
    }

    let trust_refusal = decide_launch_input(LaunchInputObservation {
        session_bound: true,
        composer_ready: true,
        directory_trust_prompt: true,
        active_interactive_prompt: false,
        classification: launch_classification(TabState::ClarifyingQuestion),
    })
    .expect_err("directory-trust modal blocks launch input");
    assert_eq!(
        trust_refusal.reason(),
        "active provider directory-trust prompt"
    );
}

#[test]
fn resumed_transcript_question_does_not_block_without_an_active_structured_prompt() {
    use codeterm_types::pty::{PendingPrompt, PromptOption};

    let transcript = "Earlier response text.\nassistant: What would you like me to do next?\n";
    let classification = launch_classification(TabState::ClarifyingQuestion);
    let transcript_provider = MockProvider::new("fixture")
        .with_tab_state(classification.clone().expect("classification"));
    let active_interactive_prompt = transcript_provider.parse_prompt(transcript).is_some();

    assert!(!active_interactive_prompt);
    assert_eq!(
        decide_launch_input(LaunchInputObservation {
            session_bound: true,
            composer_ready: false,
            directory_trust_prompt: false,
            active_interactive_prompt,
            classification,
        }),
        Ok(Some(LaunchInputEvidence::SessionBinding)),
    );

    let active_prompt = PendingPrompt {
        question: "Active structured choice".into(),
        options: vec![
            PromptOption {
                label: "Option one".into(),
                value: "1".into(),
            },
            PromptOption {
                label: "Option two".into(),
                value: "2".into(),
            },
        ],
        sections: None,
        multi_select: Some(false),
        submit_available: Some(false),
        selected_values: Some(vec!["1".into()]),
    };
    let structured_provider = MockProvider::new("fixture")
        .with_tab_state(launch_classification(TabState::ClarifyingQuestion).unwrap())
        .with_parsed_prompt(active_prompt);
    let active_interactive_prompt = structured_provider
        .parse_prompt("active input region")
        .is_some();
    assert!(active_interactive_prompt);
    assert_eq!(
        decide_launch_input(LaunchInputObservation {
            session_bound: true,
            composer_ready: true,
            directory_trust_prompt: false,
            active_interactive_prompt,
            classification: launch_classification(TabState::ClarifyingQuestion),
        }),
        Err(LaunchInputRefusal::ProviderDialog(
            TabState::ClarifyingQuestion
        )),
    );
}
