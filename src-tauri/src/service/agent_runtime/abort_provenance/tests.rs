use super::*;

fn tool(id: &str, turn: &str, status: &str) -> ChatMsg {
    ChatMsg {
        id: id.into(),
        kind: "tool_call".into(),
        tool_flow_id: Some(turn.into()),
        tool_status: Some(status.into()),
        tool_result: Some("partial".into()),
        ..ChatMsg::default()
    }
}

#[test]
fn lost_process_mid_turn_interrupts_running_tools_and_names_the_cause() {
    let items = vec![
        tool("done", "turn-1", "completed"),
        tool("hung", "turn-1", "running"),
        tool("other-turn", "turn-0", "running"),
    ];
    let cause = runtime_ended_cause(&"Event mapping failed: runtime size limit");
    let rows = runtime_ended_rows(&items, Some("turn-1"), 7, &cause, None);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].id, "hung");
    assert_eq!(rows[0].tool_status.as_deref(), Some("interrupted"));
    assert_eq!(rows[0].tool_interrupted, Some(true));
    assert_eq!(rows[0].tool_result.as_deref(), Some("partial"));
    assert_eq!(rows[1].id, "runtime-ended:7");
    assert_eq!(rows[1].kind, "system");
    assert_eq!(
        rows[1].system_subtype.as_deref(),
        Some(RUNTIME_ENDED_SUBTYPE)
    );
    assert_eq!(rows[1].tool_flow_id.as_deref(), Some("turn-1"));
    assert!(rows[1].content.contains(&cause));
}

#[test]
fn unknown_turn_still_interrupts_every_running_tool() {
    let items = vec![
        tool("a", "turn-1", "running"),
        tool("b", "turn-2", "running"),
    ];
    let rows = runtime_ended_rows(&items, None, 1, "cause", None);
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[2].tool_flow_id, None);
}

#[test]
fn idle_process_exit_adds_no_rows() {
    let items = vec![tool("done", "turn-1", "completed")];
    assert!(runtime_ended_rows(&items, None, 1, "cause", None).is_empty());
}

#[test]
fn agent_notice_carries_the_supplied_cause_as_one_application_entry() {
    let value = abort_note_context("cause-token-42");
    assert!(value["instructions"].is_null());
    let entries = value["additionalContext"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["kind"], "application");
    assert!(entries[0]["value"]
        .as_str()
        .unwrap()
        .contains("cause-token-42"));
}

#[test]
fn active_turn_without_tools_still_reports_the_abort() {
    let rows = runtime_ended_rows(&[], Some("turn-1"), 3, "cause", None);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].system_subtype.as_deref(),
        Some(RUNTIME_ENDED_SUBTYPE)
    );
}
