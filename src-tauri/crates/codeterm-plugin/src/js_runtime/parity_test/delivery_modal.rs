use crate::provider::AgentProvider;

use super::{load_js, workspace_plugins_root};

fn snapshot(screen_text: String) -> crate::tab_state::TabStateSnapshot {
    crate::tab_state::TabStateSnapshot {
        tab_id: "fixture-tab".into(),
        agent_type: "claude".into(),
        session_id: Some("fixture-session".into()),
        cwd: Some("/fixture".into()),
        screen_text,
    }
}

fn fixture(name: &str) -> String {
    std::fs::read_to_string(
        workspace_plugins_root()
            .join("provider-claude/tests/fixtures/tab-state")
            .join(name),
    )
    .expect("Claude tab-state fixture")
}

#[test]
fn busy_claude_screen_is_not_a_delivery_modal_in_native_or_js() {
    let native = crate::agent_claude::provider::ClaudeProvider::new();
    let js = load_js("claude");
    let busy = snapshot(fixture("normal-working.txt"));
    let prompt = snapshot(fixture("clarifying-question.txt"));

    assert_eq!(
        js.classify_tab_state(&busy)
            .map(|classification| classification.state),
        Some(crate::tab_state::TabState::Working)
    );
    assert!(native.parse_prompt(&prompt.screen_text).is_some());
    assert_eq!(
        js.classify_tab_state(&prompt)
            .map(|classification| classification.state),
        Some(crate::tab_state::TabState::ClarifyingQuestion)
    );
    assert!(!native.delivery_modal_visible(&busy));
    assert!(!js.delivery_modal_visible(&busy));
    assert!(native.delivery_modal_visible(&prompt));
    assert!(js.delivery_modal_visible(&prompt));
}

#[test]
fn idle_claude_composer_after_numbered_chat_history_is_not_a_delivery_modal() {
    let native = crate::agent_claude::provider::ClaudeProvider::new();
    let js = load_js("claude");
    let screen = snapshot(fixture("normal-idle-numbered-history.txt"));

    assert!(native.parse_prompt(&screen.screen_text).is_none());
    assert!(js.parse_prompt(&screen.screen_text).is_none());
    assert!(!native.delivery_modal_visible(&screen));
    assert!(!js.delivery_modal_visible(&screen));
}

#[test]
fn provider_prompt_fixtures_remain_visible_to_recovery_guards() {
    if super::require_plugins().is_none() {
        return;
    }
    let root = super::workspace_plugins_root();
    let claude_native = crate::agent_claude::provider::ClaudeProvider::new();
    let claude_js = super::load_js("claude");
    let codex_js = super::load_js("codex");

    for name in [
        "trust-continue-without-permissions.txt",
        "tool-permission.txt",
    ] {
        let screen = std::fs::read_to_string(
            root.join("provider-claude/tests/fixtures/folder-trust")
                .join(name),
        )
        .expect("Claude recovery prompt fixture");
        assert!(
            claude_native.parse_prompt(&screen).is_some(),
            "native Claude {name}"
        );
        assert!(
            claude_js.parse_prompt(&screen).is_some(),
            "JS Claude {name}"
        );
        let claude_snapshot = snapshot(screen);
        assert!(
            claude_native.delivery_modal_visible(&claude_snapshot),
            "native Claude modal {name}"
        );
        assert!(
            claude_js.delivery_modal_visible(&claude_snapshot),
            "JS Claude modal {name}"
        );
    }

    let model_picker =
        std::fs::read_to_string(root.join("provider-codex/tests/fixtures/model-picker.txt"))
            .expect("Codex model picker fixture");
    let codex_snapshot = crate::tab_state::TabStateSnapshot {
        tab_id: "codex-fixture-tab".into(),
        agent_type: "codex".into(),
        session_id: Some("codex-fixture-session".into()),
        cwd: Some("/fixture".into()),
        screen_text: model_picker.clone(),
    };
    assert_eq!(
        codex_js
            .classify_tab_state(&codex_snapshot)
            .map(|classification| classification.state),
        Some(crate::tab_state::TabState::ClarifyingQuestion)
    );
    assert!(codex_js.delivery_modal_visible(&codex_snapshot));
    assert!(
        codex_js.parse_prompt(&model_picker).is_some(),
        "Codex /model picker is a provider-declared prompt"
    );

    let codex_lookalike = std::fs::read_to_string(
        root.join("provider-codex/tests/fixtures/tab-state/recovery-dialog-lookalike.txt"),
    )
    .expect("Codex recovery lookalike fixture");
    let codex_lookalike_snapshot = crate::tab_state::TabStateSnapshot {
        tab_id: "codex-composer-tab".into(),
        agent_type: "codex".into(),
        session_id: Some("codex-composer-session".into()),
        cwd: Some("/fixture".into()),
        screen_text: codex_lookalike,
    };
    assert!(
        codex_js.delivery_modal_visible(&codex_lookalike_snapshot),
        "screen-only classification recognizes the modal-shaped composer fixture"
    );

    let unclassified = super::load_inline_js(
        "unclassified-dialog-fixture",
        "module.exports.default = {};",
    );
    let unclassified_snapshot = crate::tab_state::TabStateSnapshot {
        tab_id: "unclassified-tab".into(),
        agent_type: "unclassified".into(),
        session_id: None,
        cwd: None,
        screen_text: model_picker,
    };
    assert!(!unclassified.delivery_modal_visible(&unclassified_snapshot));
}
