use super::*;
use crate::agent::registry::AgentRegistry;
use crate::agent::test_provider::MockProvider;
use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
use codeterm_types::pty::PromptOption;
use std::sync::Arc;

fn prompt() -> PendingPrompt {
    PendingPrompt {
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

fn bundled_provider() -> JsProvider {
    let mut manifest: PluginManifest = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../plugins/provider-codex/plugin.json"
    )))
    .unwrap();
    manifest.id = "prompt-fixture".into();
    let vm = PluginVm::from_source(
        "prompt-fixture",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../plugins/provider-codex/plugin.js"
        ))
        .into(),
    )
    .unwrap();
    JsProvider::new(manifest, Arc::new(vm))
}

#[test]
fn delivered_numbered_brief_is_not_an_interactive_prompt() {
    let provider = bundled_provider();
    for suffix in ["", "\n• Working (esc to interrupt)\n›"] {
        let screen = format!("› <domios from=\"parent\">\nWhich checks should be run?\n1. Inspect the classifier\n2. Verify the regression\n</domios>{suffix}");
        assert!(provider.parse_prompt(&screen).is_none());
        let resolved = parse_live_prompt(&provider, &screen);
        assert!(resolved.is_none());
        let mut detector = crate::agent::prompt::PromptDetector::new();
        let decision = detector.evaluate_pty_output_parsed(
            "worker",
            true,
            true,
            resolved.map(|(prompt, _)| crate::agent::prompt::ResolvedPrompt {
                prompt,
                explicit: false,
            }),
        );
        assert!(matches!(
            decision,
            crate::agent::prompt::PromptDecision::None
        ));
        assert!(detector.active_prompt("worker").is_none());
    }
}

#[test]
fn real_provider_menu_still_is_an_interactive_prompt() {
    let provider = bundled_provider();
    let screen = "Choose a mode?\n› 1. Default\n  2. Restricted\nPress enter to confirm";
    let (parsed, kind) = parse_live_prompt(&provider, screen).expect("active provider menu");
    assert_eq!(kind, PromptKind::Provider);
    assert_eq!(parsed.options.len(), 2);
    let mut detector = crate::agent::prompt::PromptDetector::new();
    let decision = detector.evaluate_pty_output_parsed(
        "worker",
        true,
        true,
        Some(crate::agent::prompt::ResolvedPrompt {
            prompt: parsed,
            explicit: false,
        }),
    );
    assert!(matches!(
        decision,
        crate::agent::prompt::PromptDecision::NewPrompt { .. }
    ));
}

#[test]
fn menu_echo_followed_by_work_is_not_a_live_menu() {
    let provider = bundled_provider();
    let screen = "Choose a mode?\n› 1. Default\n  2. Restricted\nPress enter to confirm\n• Working (esc to interrupt)\n›";
    assert!(parse_live_prompt(&provider, screen).is_none());
}

#[test]
fn captured_followup_menu_reaches_pending_prompt_but_numbered_echo_does_not() {
    let provider = bundled_provider();
    let screen = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../plugins/provider-codex/tests/fixtures/tab-state/reasoning-submenu-0.160.0.txt"
    ));
    let (prompt, kind) = parse_live_prompt(&provider, screen).expect("live follow-up menu");
    assert_eq!(kind, PromptKind::Provider);
    assert_eq!(prompt.options.len(), 5);
    let mut detector = crate::agent::prompt::PromptDetector::new();
    let decision = detector.evaluate_pty_output_parsed(
        "worker",
        true,
        true,
        Some(crate::agent::prompt::ResolvedPrompt {
            prompt,
            explicit: false,
        }),
    );
    assert!(matches!(
        decision,
        crate::agent::prompt::PromptDecision::NewPrompt { .. }
    ));
    assert_eq!(detector.active_prompt("worker").unwrap().options.len(), 5);
    let echo = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../plugins/provider-codex/tests/fixtures/tab-state/echoed-numbered-message.txt"
    ));
    assert!(parse_live_prompt(&provider, echo).is_none());
}

#[test]
fn retained_model_pickers_reach_live_pending_prompt_with_structural_evidence() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    for (provider_id, fixture, count) in [
        ("claude", "model-picker-r3", 5),
        ("codex", "model-picker-r3", 7),
        ("cursor", "model-picker", 10),
    ] {
        let plugin_dir = root.join(format!("plugins/provider-{provider_id}"));
        let mut manifest: PluginManifest =
            serde_json::from_str(&std::fs::read_to_string(plugin_dir.join("plugin.json")).unwrap())
                .unwrap();
        manifest.id = format!("picker-fixture-{provider_id}");
        let vm = PluginVm::from_source(
            &manifest.id,
            std::fs::read_to_string(plugin_dir.join("plugin.js")).unwrap(),
        )
        .unwrap();
        let provider = JsProvider::new(manifest, Arc::new(vm));
        let screen = std::fs::read_to_string(root.join(format!(
            "testbench/snapshots/{provider_id}/{fixture}/screen.txt"
        )))
        .unwrap();
        let (prompt, kind) = parse_live_prompt(&provider, &screen)
            .unwrap_or_else(|| panic!("{provider_id}: retained picker resolves on the live path"));
        assert_eq!(kind, PromptKind::Provider);
        assert!(!prompt.question.is_empty());
        assert_eq!(prompt.options.len(), count, "{provider_id}");
        assert!(crate::agent::state_reactor::has_structural_prompt_evidence(
            &screen, &prompt,
        ));
        assert!(!crate::agent::state_reactor::prompt_is_composer_text(
            &screen, &prompt,
        ));
        let mut detector = crate::agent::prompt::PromptDetector::new();
        let decision = detector.evaluate_pty_output_parsed(
            provider_id,
            true,
            true,
            Some(crate::agent::prompt::ResolvedPrompt {
                prompt: prompt.clone(),
                explicit: false,
            }),
        );
        assert!(matches!(
            decision,
            crate::agent::prompt::PromptDecision::NewPrompt { .. }
        ));
        assert_eq!(detector.active_prompt(provider_id), Some(&prompt));
        assert!(
            !crate::agent::state_reactor::has_structural_prompt_evidence(
                &format!("{screen}\n→"),
                &prompt,
            )
        );
    }
}
