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
fn add(
    events: &mut Vec<BackgroundWorkObservation>,
    id: Option<&str>,
    status: Option<&str>,
    kind: Option<BackgroundWorkKind>,
    output_path: Option<String>,
) {
    if let (Some(id), Some(state)) = (id, status.and_then(state)) {
        if !id.is_empty()
            && id.len() <= 256
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            events.push(BackgroundWorkObservation {
                id: id.into(),
                state,
                kind,
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
    let result = &row["toolUseResult"];
    if let Some(id) = result["backgroundTaskId"].as_str() {
        add(
            &mut events,
            Some(id),
            Some("running"),
            Some(BackgroundWorkKind::Shell),
            shell_output_path(row, id),
        );
    }
    add(
        &mut events,
        result["agentId"].as_str(),
        result["status"].as_str(),
        Some(BackgroundWorkKind::Subagent),
        None,
    );
    let content = &row["message"]["content"];
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
            None,
        );
    }
    let attachment = &row["attachment"];
    let notification = if row["type"] == "attachment"
        && attachment["type"] == "queued_command"
        && attachment["commandMode"] == "task-notification"
    {
        attachment["prompt"].as_str()
    } else if row["type"] == "user" && row["isMeta"] == true {
        content.as_str()
    } else {
        None
    };
    if let Some(text) = notification {
        for block in text.split("<task-notification>").skip(1) {
            if let Some((body, _)) = block.split_once("</task-notification>") {
                let status = tag(body, "status");
                if matches!(status, Some("completed" | "killed" | "failed")) {
                    add(&mut events, tag(body, "task-id"), status, None, None);
                }
            }
        }
    }
    events
}

#[cfg(test)]
mod tests;
