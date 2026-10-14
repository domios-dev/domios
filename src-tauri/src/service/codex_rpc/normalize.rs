use super::protocol::Frame;
use crate::agent::chat_types::ChatMsg;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

mod diffs;
mod items;
mod restored;

#[derive(Default)]
pub struct Normalizer {
    items: HashMap<(String, String), ChatMsg>,
    sequence: i64,
    restored: restored::RestoredItems,
}

impl Normalizer {
    pub fn apply_all(&mut self, frame: &Frame) -> Vec<ChatMsg> {
        let mut messages = Vec::new();
        if let Frame::Notification { method, params } = frame {
            let receivers = params["item"]["receiverThreadIds"]
                .as_array()
                .filter(|receivers| {
                    matches!(method.as_str(), "item/started" | "item/completed")
                        && params["item"]["type"] == "collabAgentToolCall"
                        && !receivers.is_empty()
                });
            if let Some(receivers) = receivers {
                for receiver in receivers {
                    let mut params = params.clone();
                    let id = format!(
                        "{}:{}",
                        params["item"]["id"].as_str().unwrap_or_default(),
                        receiver.as_str().unwrap_or_default()
                    );
                    params["item"]["id"] = serde_json::json!(id);
                    params["item"]["receiverThreadIds"] = serde_json::json!([receiver]);
                    messages.extend(self.apply(&Frame::Notification {
                        method: method.clone(),
                        params,
                    }));
                }
                return messages;
            }
            if method == "turn/completed"
                && params
                    .pointer("/turn/error")
                    .is_some_and(|error| !error.is_null())
            {
                let error = Frame::Notification {
                    method: "error".into(),
                    params: serde_json::json!({
                    "threadId":params["threadId"],"turnId":params["turn"]["id"],"error":params["turn"]["error"]}),
                };
                messages.extend(self.apply(&error));
            }
        }
        messages.extend(self.apply(frame));
        messages
    }

    pub fn from_history(thread: &str, messages: Vec<ChatMsg>) -> Self {
        let mut restored = restored::RestoredItems::default();
        for message in &messages {
            restored.insert(thread, &message.id);
        }
        Self {
            restored,
            sequence: messages
                .iter()
                .map(|message| message.seq)
                .max()
                .unwrap_or(0),
            items: messages
                .into_iter()
                .map(|message| ((thread.into(), message.id.clone()), message))
                .collect(),
        }
    }

    pub fn restore_open_items(&mut self, thread: &str, items: &HashMap<String, String>) {
        self.restored.open(thread, items);
    }

    pub fn finish_restored_turn(&mut self, thread: &str, turn: &str) -> HashSet<String> {
        self.restored.finish_turn(thread, turn)
    }

    pub fn replace_restored_items(
        &mut self,
        thread: &str,
        ids: &HashSet<String>,
        messages: Vec<ChatMsg>,
    ) -> Vec<ChatMsg> {
        messages
            .into_iter()
            .filter_map(|mut message| {
                if !ids.contains(&message.id) {
                    return None;
                }
                let key = (thread.to_owned(), message.id.clone());
                let previous = self.items.get(&key)?;
                message.seq = previous.seq;
                message.timestamp = previous.timestamp.clone();
                self.items.insert(key, message.clone());
                Some(message)
            })
            .collect()
    }

    pub fn snapshot(&self, thread: &str) -> Vec<ChatMsg> {
        let mut messages: Vec<_> = self
            .items
            .iter()
            .filter(|((owner, _), _)| owner == thread)
            .map(|(_, message)| message.clone())
            .collect();
        messages.sort_by_key(|message| message.seq);
        messages
    }

    pub fn apply(&mut self, frame: &Frame) -> Option<ChatMsg> {
        let Frame::Notification { method, params } = frame else {
            return None;
        };
        let thread = params.get("threadId")?.as_str()?;
        if let Some(item) = items::notification_item(method, params) {
            let mut params = params.clone();
            params["item"] = item;
            return self.apply(&Frame::Notification {
                method: "item/completed".into(),
                params,
            });
        }
        let item = params.get("item");
        let id = item
            .and_then(|item| item.get("id"))
            .or_else(|| params.get("itemId"))?
            .as_str()?;
        let key = (thread.to_owned(), id.to_owned());
        // Unpositioned replay cannot safely extend an authoritative snapshot.
        if self.restored.suppresses(&key) && method != "item/completed" {
            return None;
        }
        let previous = self.items.get(&key);
        let mut message = if matches!(method.as_str(), "item/started" | "item/completed") {
            from_item(item?, previous)?
        } else {
            from_delta(method, params, previous)?
        };
        message.id = id.into();
        message.tool_flow_id = params
            .get("turnId")
            .or_else(|| params.pointer("/turn/id"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or(message.tool_flow_id);
        if let (Some("command"), Some(input), Some(turn)) = (
            message.tool_kind.as_deref(),
            message.tool_input.as_mut(),
            message.tool_flow_id.as_ref(),
        ) {
            input["commandBatchId"] = serde_json::json!(turn);
        }
        if method == "item/completed"
            && message.kind == "tool_call"
            && message.tool_error.is_none()
            && item?.get("status").is_none()
        {
            message.tool_error = Some(false);
        }
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
        if method == "item/completed" {
            self.restored.completed(&key);
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
            message.attachments = items::attachments(item);
            if let Some(subtype) = items::system_subtype(&message.content) {
                message.kind = "system".into();
                message.system_subtype = Some(subtype.into());
            }
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
            message.tool_kind = Some("command".into());
            message.tool_name = Some("commandExecution".into());
            message.tool_id = item.get("id").and_then(Value::as_str).map(str::to_owned);
            message.tool_input = Some(item.clone());
            message.tool_summary = item
                .get("command")
                .and_then(Value::as_str)
                .map(str::to_owned);
            message.tool_args = message.tool_summary.clone();
            message.tool_detail = message.tool_summary.clone();
            if let Some(output) = item.get("aggregatedOutput").and_then(Value::as_str) {
                message.tool_result = Some(output.into());
            }
            message.tool_exit_code = item.get("exitCode").and_then(Value::as_i64);
            message.tool_duration_ms = item.get("durationMs").and_then(Value::as_u64);
            message.tool_error = if message.tool_exit_code.is_some_and(|code| code != 0) {
                Some(true)
            } else {
                items::outcome(item).or_else(|| message.tool_exit_code.map(|code| code != 0))
            };
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
            message.file_diffs = Some(diffs::file_diffs(item.get("changes")));
            message.tool_edits = Some(diffs::edits(message.file_diffs.as_ref()));
            message.tool_summary = Some(diffs::paths(item.get("changes")));
            message.tool_detail = message.tool_summary.clone();
            message.tool_error = item
                .get("status")
                .and_then(Value::as_str)
                .map(|status| matches!(status, "failed" | "declined"));
            message.collapsed = true;
        }
        _ => return items::from_item(item, previous),
    }
    Some(message)
}

fn from_delta(method: &str, params: &Value, previous: Option<&ChatMsg>) -> Option<ChatMsg> {
    let delta = params
        .get("delta")
        .or_else(|| params.get("message"))?
        .as_str()?;
    let mut message = previous.cloned().unwrap_or_default();
    match method {
        "item/agentMessage/delta" | "item/plan/delta" => {
            message.kind = "assistant".into();
            message.content.push_str(delta);
        }
        "item/reasoning/textDelta" | "item/reasoning/summaryTextDelta" => {
            message.kind = "thinking".into();
            message.content.push_str(delta);
        }
        "item/commandExecution/outputDelta" | "item/fileChange/outputDelta" => {
            message.kind = "tool_call".into();
            message.tool_id = params
                .get("itemId")
                .and_then(Value::as_str)
                .map(str::to_owned);
            message.tool_kind = Some(
                if method == "item/commandExecution/outputDelta" {
                    "command"
                } else {
                    "edit"
                }
                .into(),
            );
            message
                .tool_result
                .get_or_insert_with(String::new)
                .push_str(delta);
        }
        "item/mcpToolCall/progress" => {
            message.kind = "tool_call".into();
            message.tool_kind.get_or_insert_with(|| "generic".into());
            message.tool_id = params
                .get("itemId")
                .and_then(Value::as_str)
                .map(str::to_owned);
            message.tool_detail = Some(delta.into());
        }
        _ => return None,
    }
    Some(message)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod recovery_tests;
