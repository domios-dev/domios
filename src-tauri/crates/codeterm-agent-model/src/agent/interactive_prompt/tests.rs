use super::*;
use crate::agent::registry::AgentRegistry;
use crate::agent::test_provider::MockProvider;
use codeterm_types::pty::PromptOption;

fn prompt() -> PendingPrompt {
    PendingPrompt {
        request_id: None,
        question: "opaque-question".into(),
        options: vec![PromptOption {
            label: "opaque-option".into(),
            value: "1".into(),
        }],
        sections: None,
        multi_select: None,
        submit_available: None,
        selected_values: None,
    }
}

fn classification(state: TabState, confidence: f32) -> TabStateClassification {
    TabStateClassification {
        state,
        confidence,
        data: None,
        action: None,
    }
}

#[test]
fn noninteractive_classification_rejects_parsed_transcript_options() {
    for state in [TabState::Working, TabState::Unknown, TabState::IdleStuck] {
        assert!(resolve_live_prompt(
            Some((prompt(), PromptKind::Provider)),
            Some(&classification(state, 1.0)),
        )
        .is_none());
    }
}

#[test]
fn confident_provider_menu_remains_interactive() {
    for state in [
        TabState::ClarifyingQuestion,
        TabState::UpdatePrompt,
        TabState::WaitingOnAuth,
        TabState::RateLimited,
    ] {
        let parsed = Some((prompt(), PromptKind::Provider));
        assert_eq!(
            resolve_live_prompt(parsed.clone(), Some(&classification(state, 0.8))),
            parsed
        );
        assert!(resolve_live_prompt(parsed, Some(&classification(state, 0.79))).is_none());
    }
}

#[test]
fn provider_classifier_cannot_invent_options() {
    assert!(resolve_live_prompt(
        None,
        Some(&classification(TabState::ClarifyingQuestion, 1.0))
    )
    .is_none());
}

#[test]
fn parser_only_provider_retains_its_declared_prompt() {
    let parsed = Some((prompt(), PromptKind::DirectoryTrust));
    assert_eq!(resolve_live_prompt(parsed.clone(), None), parsed);
}

#[test]
fn registry_applies_classifier_to_both_prompt_surfaces() {
    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(
        MockProvider::new("fixture")
            .with_parsed_prompt(prompt())
            .with_tab_state(classification(TabState::Working, 1.0)),
    ));
    assert!(registry.parse_prompt("fixture", "screen").is_none());
    assert!(registry
        .parse_interactive_prompt("fixture", "screen")
        .is_none());
}

#[test]
fn directory_trust_capability_is_authoritative_over_generic_classification() {
    for state in [TabState::Working, TabState::Unknown, TabState::IdleStuck] {
        let parsed = Some((prompt(), PromptKind::DirectoryTrust));
        assert_eq!(
            resolve_live_prompt(parsed.clone(), Some(&classification(state, 0.1))),
            parsed
        );
        assert!(resolve_live_prompt(None, Some(&classification(state, 0.1))).is_none());
    }
}
