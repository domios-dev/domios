use codeterm_types::agent::background_work::{
    BackgroundWorkKind, BackgroundWorkObservation, BackgroundWorkState,
};
use serde_json::Value;
fn state(value: &str) -> Option<BackgroundWorkState> {
    Some(match value {
        "running" | "async_launched" => BackgroundWorkState::Running,
        "completed" => BackgroundWorkState::Completed,
        "killed" => BackgroundWorkState::Killed,
        "failed" => BackgroundWorkState::Failed,
        _ => return None,
    })
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn bounded_label(value: Option<&str>) -> Option<String> {
    let trimmed = value?.trim();
    (!trimmed.is_empty()).then(|| trimmed.chars().take(256).collect())
}

fn single_tool_result_use_id(content: &Value) -> Option<String> {
    let results: Vec<&Value> = content
        .as_array()?
        .iter()
        .filter(|block| block["type"] == "tool_result")
        .collect();
    if results.len() != 1 || results[0]["is_error"] == true {
        return None;
    }
    let id = results[0]["tool_use_id"].as_str()?;
    valid_id(id).then(|| id.to_owned())
}

fn add(
    events: &mut Vec<BackgroundWorkObservation>,
    id: Option<&str>,
    status: Option<&str>,
    kind: Option<BackgroundWorkKind>,
    label: Option<String>,
    tool_use_id: Option<String>,
    output_path: Option<String>,
) {
    if let (Some(id), Some(state)) = (id, status.and_then(state)) {
        if valid_id(id) {
            events.push(BackgroundWorkObservation {
                id: id.into(),
                state,
                kind,
                label,
                tool_use_id,
                output_path,
            });
        }
    }
}

fn shell_output_path(row: &Value, id: &str) -> Option<String> {
    let session = row["sessionId"].as_str()?;
    let prefix =
        format!("Command running in background with ID: {id}. Output is being written to: ");
    let path = row["message"]["content"]
        .as_array()?
        .iter()
        .find_map(|block| {
            if block["type"] != "tool_result" || block["is_error"] == true {
                return None;
            }
            Some(
                block["content"]
                    .as_str()?
                    .strip_prefix(&prefix)?
                    .split_once(". You will be notified")?
                    .0,
            )
        })?;
    let parts: Vec<_> = path.rsplit(['/', '\\']).take(5).collect();
    let filename = format!("{id}.output");
    match parts.as_slice() {
        [name, "tasks", owner, key, root]
            if *name == filename
                && *owner == session
                && !key.is_empty()
                && (*root == "claude"
                    || root.strip_prefix("claude-").is_some_and(|uid| {
                        !uid.is_empty() && uid.bytes().all(|ch| ch.is_ascii_digit())
                    })) =>
        {
            Some(path.into())
        }
        _ => None,
    }
}
fn notification_end_state(status: Option<&str>) -> Option<&'static str> {
    match status? {
        "completed" => Some("completed"),
        "killed" | "stopped" => Some("killed"),
        "failed" => Some("failed"),
        _ => None,
    }
}
fn tag<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    Some(
        text.split_once(&format!("<{name}>"))?
            .1
            .split_once(&format!("</{name}>"))?
            .0
            .trim(),
    )
}
pub(super) fn parse(row: &Value) -> Vec<BackgroundWorkObservation> {
    if row["type"] == "summary"
        || row["isCompactSummary"] == true
        || (row["type"] == "system"
            && matches!(
                row["subtype"].as_str(),
                Some("compact_boundary" | "init" | "resume")
            ))
    {
        return vec![BackgroundWorkObservation {
            id: String::new(),
            state: BackgroundWorkState::SessionBoundary,
            ..Default::default()
        }];
    }
    let mut events = Vec::new();
    let content = &row["message"]["content"];
    if row["type"] == "assistant" {
        if let Some(blocks) = content.as_array() {
            for block in blocks {
                if block["type"] != "tool_use" {
                    continue;
                }
                let Some(call_id) = block["id"].as_str().filter(|id| valid_id(id)) else {
                    continue;
                };
                let name = block["name"].as_str().unwrap_or("").trim();
                let lower = name.to_ascii_lowercase();
                let input = &block["input"];
                if matches!(lower.as_str(), "bash" | "shell" | "exec") {
                    if let Some(command) = bounded_label(input["command"].as_str()) {
                        events.push(BackgroundWorkObservation {
                            id: call_id.into(),
                            state: BackgroundWorkState::ToolCall,
                            kind: Some(BackgroundWorkKind::Shell),
                            label: Some(command),
                            tool_use_id: None,
                            output_path: None,
                        });
                    }
                } else if matches!(name, "Agent" | "Task") && input["prompt"].is_string() {
                    if let Some(description) = bounded_label(input["description"].as_str()) {
                        events.push(BackgroundWorkObservation {
                            id: call_id.into(),
                            state: BackgroundWorkState::ToolCall,
                            kind: Some(BackgroundWorkKind::Subagent),
                            label: Some(description),
                            tool_use_id: None,
                            output_path: None,
                        });
                    }
                }
            }
        }
    }
    let result = &row["toolUseResult"];
    let result_tool_use_id = single_tool_result_use_id(content);
    if let Some(id) = result["backgroundTaskId"].as_str() {
        add(
            &mut events,
            Some(id),
            Some("running"),
            Some(BackgroundWorkKind::Shell),
            bounded_label(result["command"].as_str()),
            result_tool_use_id.clone(),
            shell_output_path(row, id),
        );
    }
    add(
        &mut events,
        result["agentId"].as_str(),
        result["status"].as_str(),
        Some(BackgroundWorkKind::Subagent),
        bounded_label(result["description"].as_str()),
        result_tool_use_id,
        None,
    );
    if row["type"] == "user"
        && result["task_type"] == "local_bash"
        && result["command"].is_string()
        && !content
            .as_array()
            .is_some_and(|blocks| blocks.iter().any(|block| block["is_error"] == true))
    {
        add(
            &mut events,
            result["task_id"].as_str(),
            Some("killed"),
            Some(BackgroundWorkKind::Shell),
            bounded_label(result["command"].as_str()),
            None,
            None,
        );
    }
    let attachment = &row["attachment"];
    let notification = if row["type"] == "attachment"
        && attachment["type"] == "queued_command"
        && (attachment["commandMode"] == "task-notification"
            || attachment["origin"]["kind"] == "task-notification")
    {
        attachment["prompt"].as_str()
    } else if row["type"] == "user"
        && (row["isMeta"] == true || row["origin"]["kind"] == "task-notification")
    {
        content.as_str()
    } else {
        None
    };
    if let Some(text) = notification {
        for block in text.split("<task-notification>").skip(1) {
            if let Some((body, _)) = block.split_once("</task-notification>") {
                let Some(status) = notification_end_state(tag(body, "status")) else {
                    continue;
                };
                for part in body.split("<task-id>").skip(1) {
                    if let Some((id, _)) = part.split_once("</task-id>") {
                        add(
                            &mut events,
                            Some(id.trim()),
                            Some(status),
                            None,
                            None,
                            None,
                            None,
                        );
                    }
                }
            }
        }
    }
    events
}

#[cfg(test)]
mod tests;
