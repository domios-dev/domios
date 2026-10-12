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

#[test]
fn codex_followup_menu_round_trips_native_prompt_and_response_shapes() {
    if require_plugins().is_none() {
        return;
    }
    let provider = load_js("codex");
    let screen = fixture("codex", "reasoning-submenu-0.160.0.txt");
    let result = classify(&provider, "codex", screen.clone());
    assert_eq!(result.state, TabState::ClarifyingQuestion);
    assert_eq!(result.confidence, 1.0);
    assert_single_surface(&result, "clarifying_question");
    let prompt = provider.parse_prompt(&screen).expect("active submenu");
    assert_eq!(prompt.options.len(), 5);
    assert_eq!(
        prompt
            .options
            .iter()
            .map(|option| option.value.as_str())
            .collect::<Vec<_>>(),
        vec!["1", "2", "3", "4", "5"]
    );
    assert_eq!(
        provider.build_prompt_response(1, &screen),
        vec![b"\r".to_vec()]
    );
    assert_eq!(
        provider.build_prompt_response(4, &screen),
        vec![
            b"\x1b[B".to_vec(),
            b"\x1b[B".to_vec(),
            b"\x1b[B".to_vec(),
            b"\r".to_vec()
        ]
    );

    let echoed = fixture("codex", "echoed-numbered-message.txt");
    assert!(provider.parse_prompt(&echoed).is_none());
    let result = classify(&provider, "codex", echoed);
    assert_eq!(result.state, TabState::Working);
    assert!(result.action.is_none());
}

#[test]
fn codex_routed_numbered_list_is_not_an_interactive_prompt() {
    if require_plugins().is_none() {
        return;
    }
    let provider = load_js("codex");
    let routed = fixture("codex", "routed-numbered-list.txt");
    let picker = fixture("codex", "reasoning-submenu-0.160.0.txt");
    for screen in [routed.clone(), format!("{picker}\n{routed}")] {
        assert!(provider.parse_prompt(&screen).is_none());
        let result = classify(&provider, "codex", screen);
        assert_eq!(result.state, TabState::Working);
        assert!(result.action.is_none());
    }
    assert!(provider.parse_prompt(&picker).is_some());
}
