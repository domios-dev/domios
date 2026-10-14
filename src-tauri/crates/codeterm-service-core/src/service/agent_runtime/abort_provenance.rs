use crate::agent::chat_types::ChatMsg;

// Not "turn_aborted": Chat renders that subtype as a fixed chip and would hide the cause.
pub(crate) const RUNTIME_ENDED_SUBTYPE: &str = "runtime_ended";

pub(crate) fn runtime_ended_cause(reason: &impl std::fmt::Display) -> String {
    format!(
        "the agent runtime process ended ({reason}); tool calls in flight were cut off and the agent sees them as \"aborted\""
    )
}

/// Rows that make a turn cut short by a lost runtime process visible in Chat, with its cause.
/// An idle process exit produces no rows.
pub(crate) fn runtime_ended_rows(
    items: &[ChatMsg],
    turn: Option<&str>,
    epoch: u64,
    cause: &str,
    timestamp: Option<String>,
) -> Vec<ChatMsg> {
    let mut rows: Vec<ChatMsg> = items
        .iter()
        .filter(|row| row.tool_status.as_deref() == Some("running"))
        .filter(|row| turn.is_none() || row.tool_flow_id.as_deref() == turn)
        .map(|row| ChatMsg {
            tool_status: Some("interrupted".into()),
            tool_interrupted: Some(true),
            ..row.clone()
        })
        .collect();
    if rows.is_empty() && turn.is_none() {
        return rows;
    }
    rows.push(ChatMsg {
        id: format!("runtime-ended:{epoch}"),
        kind: "system".into(),
        content: format!("Turn aborted: {cause}"),
        timestamp,
        tool_flow_id: turn.map(str::to_owned),
        system_subtype: Some(RUNTIME_ENDED_SUBTYPE.into()),
        ..ChatMsg::default()
    });
    rows
}

/// restoreContext value that tells the agent, on its next loaded thread, why its turn ended.
pub(crate) fn abort_note_context(cause: &str) -> serde_json::Value {
    serde_json::json!({
        "instructions": null,
        "additionalContext": [{
            "key": "domios-turn-aborted",
            "kind": "application",
            "value": format!("Domios notice: your previous turn was aborted, not stopped by the user: {cause}."),
        }],
    })
}

#[cfg(test)]
mod tests;
