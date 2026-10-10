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

#[test]
fn provider_handoff_resets_source_launch_identity_and_permission_state() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.restorable_launch = crate::multiplexer::types::RestorableLaunchParameters {
        captured: true,
        model: Some("source-model".into()),
        args: Some(vec!["--source-flag".into()]),
        identity_instructions: Some("source instructions".into()),
        permission_bypass_requested: true,
        ..Default::default()
    };
    tab.agent_planned_model = Some("source-planned-model".into());
    tab.agent_requested_model = Some("source-requested-model".into());
    tab.agent_provider_resolved_model = Some("source-resolved-model".into());
    tab.agent_session_model = Some("source-live-model".into());
    tab.model_fallback = Some(crate::multiplexer::types::ModelFallbackNotice {
        planned_model: None,
        requested_model: "source-requested-model".into(),
        provider_resolved_model: "source-resolved-model".into(),
        actual_model: None,
        reason: "source fallback".into(),
    });
    tab.model_identity_mismatch = Some(crate::multiplexer::types::ModelIdentityMismatch {
        planned_model: None,
        requested_model: "source-requested-model".into(),
        provider_resolved_model: None,
        actual_model: "source-actual-model".into(),
        reason: "source mismatch".into(),
    });
    tab.reasoning_effort_identity.requested = Some("source-effort".into());
    tab.auto_approve = true;

    super::reset_tab_identity_for_provider_change(tab, false);

    assert!(tab.restorable_launch.is_empty());
    assert_eq!(tab.agent_planned_model, None);
    assert_eq!(tab.agent_requested_model, None);
    assert_eq!(tab.agent_provider_resolved_model, None);
    assert_eq!(tab.agent_session_model, None);
    assert_eq!(tab.model_fallback, None);
    assert_eq!(tab.model_identity_mismatch, None);
    assert!(tab.reasoning_effort_identity.is_empty());
    assert!(!tab.auto_approve);
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
    let mut tool = message(
        "assistant",
        &(0..20)
            .map(|line| format!("line {line}\n"))
            .collect::<String>(),
    );
    tool.tool_result = Some(tool.content.clone());

    let selected = select_history(&[tool], 1000);

    assert!(selected.contains("line 0"));
    assert!(selected.contains("line 7"));
    assert!(selected.contains("additional output omitted"));
    assert!(!selected.contains("line 8"));
    assert_eq!(selected.lines().count(), 10);
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
