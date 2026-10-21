//! OpenCode SSE event → structured chat and prompt mapping (pure).
//!
//! Maps live OpenCode `/event` frames onto normalized chat parts and a
//! provider-agnostic [`PendingPrompt`]. Frames were captured on OpenCode 1.18.31.
//!
//! Pure + deterministic: no I/O, no locks. The subscriber thread feeds raw
//! `data:` JSON in and gets a [`ParsedEvent`] back; the HTTP layer turns a
//! reply into a request via [`reply_endpoint`] / [`reply_body`].

use serde_json::{json, Value};

use crate::agent::chat_types::ChatMsg;
use crate::multiplexer::types::{PendingPrompt, PromptOption};

/// What the host does in response to one SSE line.
#[derive(Debug, Clone, PartialEq)]
pub enum ParsedEvent {
    SessionUpdate {
        session_id: String,
        kind: SessionUpdate,
    },
    MessageUpdated {
        message: MessageMeta,
    },
    PartUpdated {
        update: ChatPartUpdate,
    },
    /// A new structured prompt is pending — surface it and remember its route.
    Prompt {
        prompt: PendingPrompt,
        route: ResponseRoute,
    },
    /// A prompt was answered (by us or elsewhere) — clear it.
    Replied {
        request_id: String,
    },
    /// Anything else (server.connected, session lifecycle, …) — ignore.
    Ignored,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SessionUpdate {
    Compacted,
    Error { error: Value },
    Todos { todos: Vec<Value> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct MessageMeta {
    pub session_id: String,
    pub message_id: String,
    pub role: String,
    pub timestamp: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChatPartUpdate {
    pub session_id: String,
    pub message_id: String,
    pub part_id: String,
    pub part: Value,
    pub delta: Option<String>,
    pub event: ChatPartEvent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatPartEvent {
    Delta,
    Updated,
}

/// Everything needed to answer an opencode structured prompt over HTTP.
#[derive(Debug, Clone, PartialEq)]
pub struct ResponseRoute {
    pub session_id: String,
    pub request_id: String,
    pub kind: PromptKind,
    /// Option labels in order — maps a chosen 1-based index back to its label
    /// for the `question` reply body.
    pub option_labels: Vec<String>,
    pub questions: Vec<super::questions::Question>,
    pub answers: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    Question,
    QuestionV2,
    Permission,
    PermissionV2,
}

const PERMISSION_OPTIONS: [(&str, &str); 3] =
    [("once", "Once"), ("always", "Always"), ("reject", "Reject")];

/// Parse one SSE `data:` JSON object. Unknown events map to [`ParsedEvent::Ignored`].
pub fn parse_event(value: &Value) -> ParsedEvent {
    let event_type = value.get("type").and_then(|t| t.as_str()).unwrap_or("");
    let props = value.get("properties").unwrap_or(&Value::Null);
    match event_type {
        "session.compacted" | "session.error" | "todo.updated" => {
            let Some(session_id) = props.get("sessionID").and_then(Value::as_str) else {
                return ParsedEvent::Ignored;
            };
            let kind = match event_type {
                "session.compacted" => SessionUpdate::Compacted,
                "session.error" => SessionUpdate::Error {
                    error: props.get("error").cloned().unwrap_or(Value::Null),
                },
                _ => {
                    let Some(todos) = props.get("todos").and_then(Value::as_array) else {
                        return ParsedEvent::Ignored;
                    };
                    SessionUpdate::Todos {
                        todos: todos.clone(),
                    }
                }
            };
            ParsedEvent::SessionUpdate {
                session_id: session_id.into(),
                kind,
            }
        }
        "message.updated" => parse_message_updated(props),
        "message.part.delta" => parse_part_delta(props),
        "message.part.updated" => parse_part_updated(props),
        "question.asked" => parse_question(props, PromptKind::Question),
        "question.v2.asked" => parse_question(props, PromptKind::QuestionV2),
        "permission.asked" => parse_permission(props, PromptKind::Permission),
        "permission.v2.asked" => parse_permission(props, PromptKind::PermissionV2),
        "question.replied"
        | "question.rejected"
        | "question.v2.replied"
        | "question.v2.rejected"
        | "permission.replied"
        | "permission.v2.replied" => props
            .get("requestID")
            .and_then(|v| v.as_str())
            .map(|id| ParsedEvent::Replied {
                request_id: id.to_string(),
            })
            .unwrap_or(ParsedEvent::Ignored),
        _ => ParsedEvent::Ignored,
    }
}

fn parse_message_updated(props: &Value) -> ParsedEvent {
    let Some(info) = props.get("info") else {
        return ParsedEvent::Ignored;
    };
    let (Some(message_id), Some(session_id), Some(role)) = (
        info.get("id").and_then(|v| v.as_str()),
        info.get("sessionID").and_then(|v| v.as_str()),
        info.get("role").and_then(|v| v.as_str()),
    ) else {
        return ParsedEvent::Ignored;
    };
    let timestamp = info
        .get("time")
        .and_then(|t| t.get("created"))
        .and_then(timestamp_string);
    ParsedEvent::MessageUpdated {
        message: MessageMeta {
            session_id: session_id.to_string(),
            message_id: message_id.to_string(),
            role: role.to_string(),
            timestamp,
            model: assistant_model(info),
        },
    }
}

pub(super) fn assistant_model(info: &Value) -> Option<String> {
    if info.get("role").and_then(Value::as_str) != Some("assistant") {
        return None;
    }
    let provider = info.get("providerID")?.as_str()?.trim();
    let model = info.get("modelID")?.as_str()?.trim();
    (!provider.is_empty() && !model.is_empty()).then(|| format!("{provider}/{model}"))
}

fn parse_part_delta(props: &Value) -> ParsedEvent {
    let (Some(session_id), Some(message_id), Some(part_id), Some(delta)) = (
        props.get("sessionID").and_then(|v| v.as_str()),
        props.get("messageID").and_then(|v| v.as_str()),
        props.get("partID").and_then(|v| v.as_str()),
        props.get("delta").and_then(|v| v.as_str()),
    ) else {
        return ParsedEvent::Ignored;
    };
    if props.get("field").and_then(|v| v.as_str()) != Some("text") {
        return ParsedEvent::Ignored;
    }
    ParsedEvent::PartUpdated {
        update: ChatPartUpdate {
            session_id: session_id.to_string(),
            message_id: message_id.to_string(),
            part_id: part_id.to_string(),
            part: json!({
                "id": part_id,
                "sessionID": session_id,
                "messageID": message_id
            }),
            delta: Some(delta.to_string()),
            event: ChatPartEvent::Delta,
        },
    }
}

fn parse_part_updated(props: &Value) -> ParsedEvent {
    let Some(part) = props.get("part") else {
        return ParsedEvent::Ignored;
    };
    let (Some(part_id), Some(session_id), Some(message_id)) = (
        part.get("id").and_then(|v| v.as_str()),
        part.get("sessionID").and_then(|v| v.as_str()),
        part.get("messageID").and_then(|v| v.as_str()),
    ) else {
        return ParsedEvent::Ignored;
    };
    ParsedEvent::PartUpdated {
        update: ChatPartUpdate {
            session_id: session_id.to_string(),
            message_id: message_id.to_string(),
            part_id: part_id.to_string(),
            part: part.clone(),
            delta: props
                .get("delta")
                .and_then(|v| v.as_str())
                .map(String::from),
            event: ChatPartEvent::Updated,
        },
    }
}

fn timestamp_string(value: &Value) -> Option<String> {
    match value {
        Value::Number(_) => Some(value.to_string()),
        Value::String(value) => Some(value.clone()),
        _ => None,
    }
}

/// Produce the same stable-id `ChatMsg` shape as the sqlite reader. The part's
/// full text wins over `delta`; the latter is only a fallback for delta-only frames.
pub fn normalize_chat_part(
    update: &ChatPartUpdate,
    message: &MessageMeta,
    previous_text: Option<&str>,
) -> Option<ChatMsg> {
    if update.session_id != message.session_id || update.message_id != message.message_id {
        return None;
    }
    let part_type = update.part.get("type").and_then(|v| v.as_str())?;
    let timestamp = update
        .part
        .get("time")
        .and_then(|t| t.get("created"))
        .and_then(timestamp_string)
        .or_else(|| message.timestamp.clone());
    match part_type {
        "text" | "reasoning" => {
            let text = update
                .part
                .get("text")
                .and_then(|v| v.as_str())
                .map(String::from)
                .or_else(|| {
                    let delta = update.delta.as_deref()?;
                    Some(format!("{}{}", previous_text.unwrap_or_default(), delta))
                })?;
            if text.trim().is_empty() {
                return None;
            }
            Some(
                ChatMsg {
                    id: update.part_id.clone(),
                    kind: if part_type == "reasoning" {
                        "thinking".into()
                    } else if message.role == "user" {
                        "user".into()
                    } else {
                        "assistant".into()
                    },
                    content: text,
                    timestamp,
                    ..Default::default()
                }
                .with_segments(),
            )
        }
        "tool" => normalize_tool_part(update, timestamp),
        _ => None,
    }
}

fn normalize_tool_part(update: &ChatPartUpdate, timestamp: Option<String>) -> Option<ChatMsg> {
    let tool_name = update
        .part
        .get("tool")
        .and_then(|v| v.as_str())
        .unwrap_or("tool");
    let state = update.part.get("state").unwrap_or(&Value::Null);
    let input = state.get("input");
    let metadata = state.get("metadata").unwrap_or(&Value::Null);
    let time = state.get("time").unwrap_or(&Value::Null);
    let start = time.get("start").and_then(Value::as_i64);
    let end = time.get("end").and_then(Value::as_i64);
    let exit_code = metadata.get("exit").and_then(Value::as_i64);
    let (tool_kind, tool_summary, tool_detail, tool_edits) =
        canonical_tool_fields(tool_name, input);
    Some(ChatMsg {
        id: update.part_id.clone(),
        kind: "tool_call".into(),
        content: String::new(),
        timestamp: time
            .get("start")
            .and_then(timestamp_string)
            .or_else(|| time.get("end").and_then(timestamp_string))
            .or(timestamp),
        tool_name: Some(tool_name.to_string()),
        tool_input: input.cloned(),
        tool_result: state
            .get("output")
            .and_then(|v| v.as_str())
            .map(String::from),
        tool_id: update
            .part
            .get("callID")
            .and_then(|v| v.as_str())
            .map(String::from),
        tool_title: state
            .get("title")
            .and_then(|v| v.as_str())
            .map(String::from),
        tool_exit_code: exit_code,
        tool_duration_ms: start
            .zip(end)
            .and_then(|(start, end)| u64::try_from(end.saturating_sub(start)).ok()),
        tool_error: match state["status"].as_str() {
            Some("error") => Some(true),
            Some("completed") => Some(false),
            _ => exit_code.map(|code| code != 0),
        },
        tool_kind: Some(tool_kind),
        tool_summary: Some(tool_summary),
        tool_detail: Some(tool_detail),
        tool_edits: Some(Value::Array(tool_edits)),
        ..Default::default()
    })
}

fn canonical_tool_fields(
    tool_name: &str,
    input: Option<&Value>,
) -> (String, String, String, Vec<Value>) {
    let null = Value::Null;
    let value = input.filter(|v| v.is_object()).unwrap_or(&null);
    let command = first_string(value, &["command", "cmd"]).unwrap_or_default();
    let path = first_string(value, &["file_path", "filePath", "path"]).unwrap_or_default();
    let pattern = first_string(value, &["pattern"]).unwrap_or_default();
    let lower = tool_name.trim().to_lowercase();
    let edits = canonical_edits(value, &path);
    let fields = if ["bash", "shell", "exec", "run"].contains(&lower.as_str()) {
        (
            "command",
            clean_tool_text(command, "Command"),
            command.to_string(),
        )
    } else if ["edit", "multiedit", "write"].contains(&lower.as_str()) {
        (
            "edit",
            nonempty_or(path, "File edit").to_string(),
            path.to_string(),
        )
    } else if lower == "read" {
        (
            "read",
            nonempty_or(path, "Read file").to_string(),
            path.to_string(),
        )
    } else if ["glob", "grep", "search"].contains(&lower.as_str()) {
        let detail = nonempty_or(pattern, path);
        (
            "search",
            clean_tool_text(detail, "Search"),
            detail.to_string(),
        )
    } else {
        (
            "generic",
            clean_tool_text(tool_name, "Tool"),
            tool_name.to_string(),
        )
    };
    (fields.0.into(), fields.1, fields.2, edits)
}

fn first_string<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(|v| v.as_str()))
}

fn nonempty_or<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if value.is_empty() {
        fallback
    } else {
        value
    }
}

fn clean_tool_text(value: &str, fallback: &str) -> String {
    let cleaned = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if cleaned.is_empty() {
        return fallback.to_string();
    }
    if cleaned.chars().count() > 120 {
        return format!("{}...", cleaned.chars().take(117).collect::<String>());
    }
    cleaned
}

fn canonical_edits(input: &Value, parent_path: &str) -> Vec<Value> {
    let mut source = Vec::new();
    if let Some(edits) = input.get("edits").and_then(|v| v.as_array()) {
        source.extend(edits.iter());
    } else {
        source.push(input);
    }
    source
        .into_iter()
        .filter_map(|item| {
            if !item.is_object() {
                return None;
            }
            let old = first_string(item, &["old_string", "oldString"]);
            let new = first_string(item, &["new_string", "newString"]);
            if old.is_none() && new.is_none() {
                return None;
            }
            let path =
                first_string(item, &["file_path", "filePath", "path"]).unwrap_or(parent_path);
            Some(json!({
                "path": path,
                "old": old.unwrap_or_default(),
                "new": new.unwrap_or_default(),
            }))
        })
        .collect()
}

fn parse_question(props: &Value, kind: PromptKind) -> ParsedEvent {
    let (Some(request_id), Some(session_id)) = (
        props.get("id").and_then(Value::as_str),
        props.get("sessionID").and_then(Value::as_str),
    ) else {
        return ParsedEvent::Ignored;
    };
    let Some(questions) = props.get("questions").and_then(super::questions::parse) else {
        return ParsedEvent::Ignored;
    };
    let route = ResponseRoute {
        session_id: session_id.into(),
        request_id: request_id.into(),
        kind,
        option_labels: questions[0].labels.clone(),
        questions,
        answers: Vec::new(),
    };
    let Some(prompt) = super::questions::prompt(&route) else {
        return ParsedEvent::Ignored;
    };
    ParsedEvent::Prompt { prompt, route }
}

fn parse_permission(props: &Value, kind: PromptKind) -> ParsedEvent {
    let Some(request_id) = props.get("id").and_then(|v| v.as_str()) else {
        return ParsedEvent::Ignored;
    };
    let session_id = props
        .get("sessionID")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let meta = props.get("metadata").unwrap_or(&Value::Null);
    let question = meta
        .get("description")
        .and_then(|v| v.as_str())
        .or_else(|| meta.get("command").and_then(|v| v.as_str()))
        .or_else(|| props.get("title").and_then(|v| v.as_str()))
        .or_else(|| props.get("permission").and_then(|v| v.as_str()))
        .or_else(|| props.get("action").and_then(Value::as_str))
        .unwrap_or("Permission requested")
        .to_string();
    let options = PERMISSION_OPTIONS
        .iter()
        .map(|(value, label)| PromptOption {
            label: (*label).to_string(),
            value: (*value).to_string(),
        })
        .collect();
    let prompt = PendingPrompt {
        request_id: None,
        question,
        options,
        sections: None,
        multi_select: Some(false),
        submit_available: Some(false),
        selected_values: Some(Vec::new()),
    };
    ParsedEvent::Prompt {
        prompt,
        route: ResponseRoute {
            session_id: session_id.to_string(),
            request_id: request_id.to_string(),
            kind,
            questions: Vec::new(),
            answers: Vec::new(),
            option_labels: PERMISSION_OPTIONS
                .iter()
                .map(|(v, _)| v.to_string())
                .collect(),
        },
    }
}

/// Reply endpoint relative to the server base.
pub fn reply_endpoint(route: &ResponseRoute) -> String {
    match route.kind {
        PromptKind::Question => format!("/question/{}/reply", route.request_id),
        PromptKind::QuestionV2 => format!(
            "/api/session/{}/question/{}/reply",
            route.session_id, route.request_id
        ),
        PromptKind::Permission => format!("/permission/{}/reply", route.request_id),
        PromptKind::PermissionV2 => format!(
            "/api/session/{}/permission/{}/reply",
            route.session_id, route.request_id
        ),
    }
}

/// Build the JSON reply body for a chosen option `value`.
/// For a question the answer is the option label; for a permission it is the
/// reply token (`once` / `always` / `reject`). Accepts either the option value
/// exposed in [`PendingPrompt`] or a legacy 1-based index.
pub fn reply_body(route: &ResponseRoute, value: &str) -> Option<Value> {
    match super::questions::answer(route, value)? {
        super::questions::Answer::Complete { body, .. } => Some(body),
        super::questions::Answer::Next { .. } => None,
    }
}

#[cfg(test)]
mod tests;
