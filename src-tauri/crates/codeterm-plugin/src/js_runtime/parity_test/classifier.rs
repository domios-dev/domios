use super::*;
use crate::tab_state::{TabState, TabStateClassification, TabStateSnapshot};

fn fixture(provider: &str, name: &str) -> String {
    std::fs::read_to_string(
        workspace_plugins_root()
            .join(format!("provider-{provider}"))
            .join("tests")
            .join("fixtures")
            .join("tab-state")
            .join(name),
    )
    .unwrap_or_else(|error| panic!("read {provider} classifier fixture {name}: {error}"))
}

fn classify(
    provider: &JsProvider,
    agent_type: &str,
    screen_text: String,
) -> TabStateClassification {
    provider
        .classify_tab_state(&TabStateSnapshot {
            tab_id: "tab-parity".into(),
            agent_type: agent_type.into(),
            session_id: None,
            cwd: None,
            screen_text,
        })
        .expect("classifier fixture must return a verdict")
}

fn assert_single_surface(classification: &TabStateClassification, kind: &str) {
    let action = classification
        .action
        .as_ref()
        .expect("input request must carry one action");
    assert!(action.keys.is_none(), "input request must not auto-act");
    assert_eq!(
        action.surface.as_ref().map(|surface| surface.kind.as_str()),
        Some(kind)
    );
}

#[test]
fn provider_classifiers_agree_on_shared_input_request_fixtures() {
    if require_plugins().is_none() {
        return;
    }
    let claude = load_js("claude");
    let codex = load_js("codex");

    for (name, expected) in [
        ("stale-auth-ready-composer.txt", TabState::Working),
        ("stale-auth-streaming.txt", TabState::Working),
        (
            "structured-pending-prompt.txt",
            TabState::ClarifyingQuestion,
        ),
        ("active-terminal-choice.txt", TabState::ClarifyingQuestion),
    ] {
        let claude_fixture = fixture("claude", name);
        let codex_fixture = fixture("codex", name);
        assert_eq!(
            claude_fixture, codex_fixture,
            "{name} must be byte-identical across provider fixtures"
        );

        let claude_result = classify(&claude, "claude", claude_fixture);
        let codex_result = classify(&codex, "codex", codex_fixture);
        assert_eq!(claude_result.state, expected, "claude {name}");
        assert_eq!(codex_result.state, expected, "codex {name}");
        assert_eq!(claude_result, codex_result, "provider parity for {name}");

        if expected == TabState::ClarifyingQuestion {
            assert_single_surface(&claude_result, "clarifying_question");
            assert_single_surface(&codex_result, "clarifying_question");
        } else {
            assert!(claude_result.action.is_none(), "claude {name} is inert");
            assert!(codex_result.action.is_none(), "codex {name} is inert");
        }
    }

    let claude_auth = classify(&claude, "claude", fixture("claude", "auth-login.txt"));
    let codex_auth = classify(&codex, "codex", fixture("codex", "auth-login.txt"));
    assert_eq!(claude_auth.state, TabState::WaitingOnAuth);
    assert_eq!(codex_auth.state, TabState::WaitingOnAuth);
    assert_eq!(claude_auth, codex_auth, "genuine login prompt parity");
    assert_single_surface(&claude_auth, "auth_prompt");
    assert_single_surface(&codex_auth, "auth_prompt");
}
