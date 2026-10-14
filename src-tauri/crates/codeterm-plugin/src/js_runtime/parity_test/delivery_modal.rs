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
    assert!(!native.delivery_modal_visible(&prompt));
    assert!(js.delivery_modal_visible(&prompt));
}

#[test]
fn filled_composer_after_angle_bracket_history_is_not_a_delivery_modal() {
    let native = crate::agent_claude::provider::ClaudeProvider::new();
    let js = load_js("claude");
    let parked = snapshot(fixture("filled-composer-numbered-history.txt"));
    assert!(native.parse_prompt(&parked.screen_text).is_none());
    assert!(js.parse_prompt(&parked.screen_text).is_none());
    assert!(!native.delivery_modal_visible(&parked));
    assert!(!js.delivery_modal_visible(&parked));

    let live = "Do you want to proceed?\n❯ 1. Yes\n  2. No\nEsc to cancel · Tab to amend\n";
    assert!(native.parse_prompt(live).is_some());
    assert!(js.parse_prompt(live).is_some());
    let filled = format!(
        "{live}❯ <domios type=\"attachment\" kind=\"voice\" path=\"voice-note-a\" label=\"voice\" />\n"
    );
    assert!(native.parse_prompt(&filled).is_none());
    assert!(js.parse_prompt(&filled).is_none());
    let filled_snapshot = snapshot(filled);
    assert!(!native.delivery_modal_visible(&filled_snapshot));
    assert!(!js.delivery_modal_visible(&filled_snapshot));
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

    for (name, confident_numbered_prompt) in [
        ("trust-continue-without-permissions.txt", false),
        ("tool-permission.txt", true),
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
            !claude_native.delivery_modal_visible(&claude_snapshot),
            "native Claude declares no classifier, so it never withholds Enter: {name}"
        );
        assert_eq!(
            claude_js.delivery_modal_visible(&claude_snapshot),
            confident_numbered_prompt,
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
