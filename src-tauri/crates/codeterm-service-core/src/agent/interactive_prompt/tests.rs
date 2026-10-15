use super::*;
use crate::agent::provider::AgentProvider;
use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
use std::sync::Arc;
fn bundled_provider() -> JsProvider {
    let mut manifest: PluginManifest = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../plugins/provider-codex/plugin.json"
    )))
    .unwrap();
    manifest.id = "prompt-fixture".into();
    let vm = PluginVm::from_source(
        "prompt-fixture",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../plugins/provider-codex/plugin.js"
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
        "/../../../plugins/provider-codex/tests/fixtures/tab-state/reasoning-submenu-0.160.0.txt"
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
        "/../../../plugins/provider-codex/tests/fixtures/tab-state/echoed-numbered-message.txt"
    ));
    assert!(parse_live_prompt(&provider, echo).is_none());
}

#[test]
fn retained_model_pickers_reach_live_pending_prompt_with_structural_evidence() {
    let root = crate::test_support::fixture_manifest_dir().join("../../..");
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

#[test]
fn captured_msg07_model_picker_replays_parser_structure_and_prompt_detector() {
    let provider = bundled_provider();
    for screen in [
        include_str!("../../../../../../plugins/provider-codex/tests/fixtures/tab-state/live-model-picker-msg07-after-trigger.txt"),
        include_str!("../../../../../../plugins/provider-codex/tests/fixtures/tab-state/live-model-picker-msg07-result.txt"),
    ] {
        let rewritten = screen.replace("Select Model and Effort", "Opaque heading")
            .replace("enter select · esc back", "enter accept · esc dismiss");
        for input in [screen, rewritten.as_str()] {
            let parsed = provider.parse_prompt(input).expect("provider parser accepts captured menu");
            let (prompt, kind) = parse_live_prompt(&provider, input).expect("classifier admits captured menu");
            assert_eq!(prompt, parsed);
            assert_eq!(kind, PromptKind::Provider);
            assert_eq!(prompt.options.iter().map(|option| option.value.as_str()).collect::<Vec<_>>(),
                vec!["1", "2", "3", "4", "5", "6", "7"]);
            assert!(crate::agent::state_reactor::has_structural_prompt_evidence(input, &prompt));
            assert!(crate::agent::state_reactor::has_numbered_terminal_choice(input, &prompt));
            assert!(!crate::agent::state_reactor::prompt_is_composer_text(input, &prompt));
            let mut detector = crate::agent::prompt::PromptDetector::new();
            let decision = detector.evaluate_pty_output_parsed("capture", true, true,
                Some(crate::agent::prompt::ResolvedPrompt {prompt: prompt.clone(), explicit: false}));
            assert!(matches!(decision, crate::agent::prompt::PromptDecision::NewPrompt { .. }));
            assert_eq!(detector.active_prompt("capture"), Some(&prompt));
            assert!(parse_live_prompt(&provider, &format!("{input}\n› ready composer")).is_none());
        }
    }
}
