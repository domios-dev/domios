use super::protocol::Frame;
use crate::agent::chat_types::ChatMsg;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Default)]
pub struct Normalizer {
    items: HashMap<(String, String), ChatMsg>,
    sequence: i64,
}

impl Normalizer {
    pub fn apply(&mut self, frame: &Frame) -> Option<ChatMsg> {
        let Frame::Notification { method, params } = frame else {
            return None;
        };
        let thread = params.get("threadId")?.as_str()?;
        let item = params.get("item");
        let id = item
            .and_then(|item| item.get("id"))
            .or_else(|| params.get("itemId"))?
            .as_str()?;
        let key = (thread.to_owned(), id.to_owned());
        let previous = self.items.get(&key);
        let mut message = if matches!(method.as_str(), "item/started" | "item/completed") {
            from_item(item?, previous)?
        } else {
            from_delta(method, params, previous)?
        };
        message.id = id.into();
        if let Some(previous) = previous {
            message.seq = previous.seq;
            message.timestamp = previous.timestamp.clone();
        } else {
            self.sequence += 1;
            message.seq = self.sequence;
            message.timestamp = params
                .get("startedAtMs")
                .or_else(|| params.get("completedAtMs"))
                .and_then(Value::as_i64)
                .and_then(chrono::DateTime::from_timestamp_millis)
                .map(|timestamp| timestamp.to_rfc3339());
        }
        self.items.insert(key, message.clone());
        Some(message)
    }
}

fn strings(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .filter_map(|part| part.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

fn from_item(item: &Value, previous: Option<&ChatMsg>) -> Option<ChatMsg> {
    let mut message = previous.cloned().unwrap_or_default();
    match item.get("type")?.as_str()? {
        "agentMessage" => {
            message.kind = "assistant".into();
            message.content = item
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into();
        }
        "userMessage" => {
            message.kind = "user".into();
            message.content = item
                .get("content")
                .and_then(Value::as_array)?
                .iter()
                .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n");
        }
        "reasoning" => {
            message.kind = "thinking".into();
            let summary = strings(item.get("summary"));
            let content = strings(item.get("content"));
            if !summary.is_empty() || !content.is_empty() {
                message.content = if summary.is_empty() { content } else { summary };
            }
        }
        "commandExecution" => {
            message.kind = "tool_call".into();
            message.tool_kind = Some("shell".into());
            message.tool_name = Some("commandExecution".into());
            message.tool_id = item.get("id").and_then(Value::as_str).map(str::to_owned);
            message.tool_input = Some(item.clone());
            message.tool_summary = item
                .get("command")
                .and_then(Value::as_str)
                .map(str::to_owned);
            message.tool_args = message.tool_summary.clone();
            if let Some(output) = item.get("aggregatedOutput").and_then(Value::as_str) {
                message.tool_result = Some(output.into());
            }
            message.tool_exit_code = item.get("exitCode").and_then(Value::as_i64);
            message.tool_duration_ms = item.get("durationMs").and_then(Value::as_u64);
            message.tool_error = message.tool_exit_code.map(|code| code != 0);
            message.tool_interrupted = item
                .get("status")
                .and_then(Value::as_str)
                .filter(|status| *status == "interrupted")
                .map(|_| true);
            message.collapsed = true;
        }
        "fileChange" => {
            message.kind = "tool_call".into();
            message.tool_kind = Some("edit".into());
            message.tool_name = Some("fileChange".into());
            message.tool_id = item.get("id").and_then(Value::as_str).map(str::to_owned);
            message.tool_input = Some(item.clone());
            message.tool_edits = item.get("changes").cloned();
            message.file_diffs = item.get("changes").cloned();
            message.tool_error = item
                .get("status")
                .and_then(Value::as_str)
                .map(|status| matches!(status, "failed" | "declined"));
            message.collapsed = true;
        }
        _ => return None,
    }
    Some(message)
}

fn from_delta(method: &str, params: &Value, previous: Option<&ChatMsg>) -> Option<ChatMsg> {
    let delta = params.get("delta")?.as_str()?;
    let mut message = previous.cloned().unwrap_or_default();
    match method {
        "item/agentMessage/delta" => {
            message.kind = "assistant".into();
            message.content.push_str(delta);
        }
        "item/reasoning/textDelta" | "item/reasoning/summaryTextDelta" => {
            message.kind = "thinking".into();
            message.content.push_str(delta);
        }
        "item/commandExecution/outputDelta" | "item/fileChange/outputDelta" => {
            message.kind = "tool_call".into();
            message
                .tool_result
                .get_or_insert_with(String::new)
                .push_str(delta);
        }
        _ => return None,
    }
    Some(message)
}

#[cfg(test)]
mod tests;
