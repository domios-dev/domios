use codeterm_types::agent::background_work::{BackgroundWorkObservation, BackgroundWorkState};
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
fn add(events: &mut Vec<BackgroundWorkObservation>, id: Option<&str>, status: Option<&str>) {
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
            });
        }
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
        }];
    }
    let mut events = Vec::new();
    let result = &row["toolUseResult"];
    add(
        &mut events,
        result["backgroundTaskId"].as_str(),
        Some("running"),
    );
    add(
        &mut events,
        result["agentId"].as_str(),
        result["status"].as_str(),
    );
    let mut texts = Vec::new();
    let content = &row["message"]["content"];
    if row["type"] == "user"
        && result["task_type"] == "local_bash"
        && result["command"].is_string()
        && !content
            .as_array()
            .is_some_and(|blocks| blocks.iter().any(|block| block["is_error"] == true))
    {
        add(&mut events, result["task_id"].as_str(), Some("killed"));
    }
    if let Some(text) = content.as_str() {
        texts.push(text);
    }
    if let Some(blocks) = content.as_array() {
        texts.extend(blocks.iter().filter_map(|block| block["text"].as_str()));
    }
    if let Some(text) = row["attachment"]["prompt"].as_str() {
        texts.push(text);
    }
    for text in texts {
        for block in text.split("<task-notification>").skip(1) {
            if let Some((body, _)) = block.split_once("</task-notification>") {
                if tag(body, "status") != Some("async_launched") {
                    add(&mut events, tag(body, "task-id"), tag(body, "status"));
                }
            }
        }
    }
    events
}

#[cfg(test)]
mod tests;
