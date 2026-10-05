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
    assert!(!native.delivery_modal_visible(&busy));
    assert!(!js.delivery_modal_visible(&busy));
    assert!(native.delivery_modal_visible(&prompt));
    assert!(js.delivery_modal_visible(&prompt));
}
