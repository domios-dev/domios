use super::select_history;
use crate::agent::chat_types::ChatMsg;

#[test]
fn provider_handoff_requires_explicit_opt_in_and_a_different_provider() {
    assert!(super::should_handoff_history("source", "target", true));
    assert!(!super::should_handoff_history("source", "target", false));
    assert!(!super::should_handoff_history("source", "source", true));
}

#[test]
fn provider_handoff_composes_with_the_existing_launch_task() {
    let composed = super::compose_launch_task(Some("launch task"), "selected history");

    assert!(composed.starts_with("launch task\n\n"));
    assert!(composed.ends_with("selected history"));
}

fn message(kind: &str, content: &str) -> ChatMsg {
    ChatMsg {
        kind: kind.to_string(),
        content: content.to_string(),
        ..Default::default()
    }
}

#[test]
fn select_history_keeps_the_first_user_goal_and_prefers_recent_messages() {
    let messages = vec![
        message("user", "Keep the original goal"),
        message("assistant", "Old progress details"),
        message("user", "The newest clarification"),
        message("assistant", "The latest result"),
    ];

    let selected = select_history(&messages, 34);

    assert!(selected.contains("Keep the original goal"));
    assert!(selected.contains("The latest result"));
    assert!(!selected.contains("Old progress details"));
}

#[test]
fn select_history_stays_within_its_estimated_token_budget() {
    let messages = vec![
        message("user", &"goal ".repeat(50)),
        message("assistant", &"recent ".repeat(100)),
    ];

    let selected = select_history(&messages, 24);

    assert!(selected.len() <= 24 * 3);
    assert!(selected.contains("## User"));
}

#[test]
fn select_history_summarizes_tool_output_to_bounded_lines() {
    let mut tool = message("tool_result", &(0..20).map(|line| format!("line {line}\n")).collect::<String>());
    tool.tool_result = Some(tool.content.clone());

    let selected = select_history(&[tool], 1000);

    assert!(selected.contains("line 0"));
    assert!(selected.contains("line 7"));
    assert!(selected.contains("12 lines omitted"));
    assert!(!selected.contains("line 8"));
}

#[test]
fn select_history_redacts_secrets_and_is_deterministic() {
    let messages = vec![message(
        "user",
        "Use CODETERM_API_TOKEN=abc123 and Authorization: Bearer xyz789",
    )];

    let first = select_history(&messages, 100);
    let second = select_history(&messages, 100);

    assert_eq!(first, second);
    assert!(!first.contains("abc123"));
    assert!(!first.contains("xyz789"));
}
