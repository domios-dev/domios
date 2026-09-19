//! OpenCode SSE event → structured chat and prompt mapping (pure).
//!
//! Maps the local opencode server's `/event` bus payloads onto normalized chat
//! parts and a provider-agnostic [`PendingPrompt`]. The `server.connected` frame
//! was observed live; chat frames follow the SDK shape.
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
    MessageUpdated { message: MessageMeta },
    PartUpdated { update: ChatPartUpdate },
    /// A new structured prompt is pending — surface it and remember its route.
    Prompt {
        prompt: PendingPrompt,
        route: ResponseRoute,
    },
    /// A prompt was answered (by us or elsewhere) — clear it.
    Replied { request_id: String },
    /// Anything else (server.connected, session lifecycle, …) — ignore.
    Ignored,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MessageMeta {
    pub session_id: String,
    pub message_id: String,
    pub role: String,
    pub timestamp: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChatPartUpdate {
    pub session_id: String,
    pub message_id: String,
    pub part_id: String,
    pub part: Value,
    pub delta: Option<String>,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    Question,
    Permission,
}

const PERMISSION_OPTIONS: [(&str, &str); 3] =
    [("once", "Once"), ("always", "Always"), ("reject", "Reject")];

/// Parse one SSE `data:` JSON object. Unknown events map to [`ParsedEvent::Ignored`].
pub fn parse_event(value: &Value) -> ParsedEvent {
    let event_type = value.get("type").and_then(|t| t.as_str()).unwrap_or("");
    let props = value.get("properties").unwrap_or(&Value::Null);
    match event_type {
        "message.updated" => parse_message_updated(props),
        "message.part.updated" => parse_part_updated(props),
        "question.asked" | "question.v2.asked" => parse_question(props),
        "permission.asked" | "permission.updated" => parse_permission(props),
        "question.replied" | "permission.replied" => props
            .get("requestID")
            .or_else(|| props.get("permissionID"))
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
            delta: props.get("delta").and_then(|v| v.as_str()).map(String::from),
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
            Some(ChatMsg {
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
            })
        }
        "tool" => normalize_tool_part(update, timestamp),
        _ => None,
    }
}

fn normalize_tool_part(update: &ChatPartUpdate, timestamp: Option<String>) -> Option<ChatMsg> {
    let tool_name = update.part.get("tool").and_then(|v| v.as_str()).unwrap_or("tool");
    let state = update.part.get("state").unwrap_or(&Value::Null);
    let input = state.get("input");
    let (tool_kind, tool_summary, tool_detail, tool_edits) = canonical_tool_fields(tool_name, input);
    Some(ChatMsg {
        id: update.part_id.clone(),
        kind: "tool_call".into(),
        content: String::new(),
        timestamp,
        tool_name: Some(tool_name.to_string()),
        tool_input: input.cloned(),
        tool_result: state.get("output").and_then(|v| v.as_str()).map(String::from),
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
        ("command", clean_tool_text(command, "Command"), command.to_string())
    } else if ["edit", "multiedit", "write"].contains(&lower.as_str()) {
        ("edit", nonempty_or(path, "File edit"), path.to_string())
    } else if lower == "read" {
        ("read", nonempty_or(path, "Read file"), path.to_string())
    } else if ["glob", "grep", "search"].contains(&lower.as_str()) {
        let detail = nonempty_or(pattern, path);
        ("search", clean_tool_text(detail, "Search"), detail.to_string())
    } else {
        ("generic", clean_tool_text(tool_name, "Tool"), tool_name.to_string())
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
            let path = first_string(item, &["file_path", "filePath", "path"])
                .unwrap_or(parent_path);
            Some(json!({
                "path": path,
                "old": old.unwrap_or_default(),
                "new": new.unwrap_or_default(),
            }))
        })
        .collect()
}

fn parse_question(props: &Value) -> ParsedEvent {
    let Some(request_id) = props.get("id").and_then(|v| v.as_str()) else {
        return ParsedEvent::Ignored;
    };
    let session_id = props
        .get("sessionID")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let Some(questions) = props.get("questions").and_then(|q| q.as_array()) else {
        return ParsedEvent::Ignored;
    };
    let Some(first) = questions.first() else {
        return ParsedEvent::Ignored;
    };
    let question = first
        .get("question")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let option_labels: Vec<String> = first
        .get("options")
        .and_then(|o| o.as_array())
        .map(|opts| {
            opts.iter()
                .filter_map(|o| o.get("label").and_then(|v| v.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let multi_select = first
        .get("multiple")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let options = option_labels
        .iter()
        .enumerate()
        .map(|(idx, label)| PromptOption {
            label: label.clone(),
            value: (idx + 1).to_string(),
        })
        .collect();
    let prompt = PendingPrompt {
        question,
        options,
        sections: None,
        multi_select: Some(multi_select),
        submit_available: Some(!multi_select),
        selected_values: Some(Vec::new()),
    };
    ParsedEvent::Prompt {
        prompt,
        route: ResponseRoute {
            session_id: session_id.to_string(),
            request_id: request_id.to_string(),
            kind: PromptKind::Question,
            option_labels,
        },
    }
}

fn parse_permission(props: &Value) -> ParsedEvent {
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
        question,
        options,
        sections: None,
        multi_select: Some(false),
        submit_available: Some(true),
        selected_values: Some(Vec::new()),
    };
    ParsedEvent::Prompt {
        prompt,
        route: ResponseRoute {
            session_id: session_id.to_string(),
            request_id: request_id.to_string(),
            kind: PromptKind::Permission,
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
        PromptKind::Question => format!(
            "/api/session/{}/question/{}/reply",
            route.session_id, route.request_id
        ),
        PromptKind::Permission => format!(
            "/session/{}/permissions/{}",
            route.session_id, route.request_id
        ),
    }
}

/// Build the JSON reply body for a chosen option `value`.
/// For a question the answer is the option label; for a permission it is the
/// reply token (`once` / `always` / `reject`). Accepts either the option value
/// exposed in [`PendingPrompt`] or a legacy 1-based index.
pub fn reply_body(route: &ResponseRoute, value: &str) -> Option<Value> {
    let choice = if let Some(index) = value.parse::<usize>().ok().and_then(|v| v.checked_sub(1)) {
        route.option_labels.get(index)?.clone()
    } else {
        route
            .option_labels
            .iter()
            .find(|label| label.as_str() == value)
            .cloned()?
    };
    Some(match route.kind {
        PromptKind::Question => json!({ "answers": [[choice]] }),
        PromptKind::Permission => json!({ "response": choice }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_question_asked_into_prompt_and_route() {
        // Real shape from the W3 spike doc.
        let event = json!({
            "id": "evt_1",
            "type": "question.asked",
            "properties": {
                "id": "que_42",
                "sessionID": "ses_9",
                "questions": [{
                    "question": "Which option should CodeTerm choose?",
                    "header": "Spike",
                    "options": [
                        {"label": "Alpha", "description": ""},
                        {"label": "Beta", "description": ""},
                        {"label": "Gamma", "description": ""}
                    ]
                }]
            }
        });
        match parse_event(&event) {
            ParsedEvent::Prompt { prompt, route } => {
                assert_eq!(prompt.question, "Which option should CodeTerm choose?");
                assert_eq!(prompt.options.len(), 3);
                assert_eq!(prompt.options[0].label, "Alpha");
                assert_eq!(prompt.options[0].value, "1");
                assert_eq!(prompt.multi_select, Some(false));
                assert_eq!(route.kind, PromptKind::Question);
                assert_eq!(route.request_id, "que_42");
                assert_eq!(route.session_id, "ses_9");
                assert_eq!(
                    reply_endpoint(&route),
                    "/api/session/ses_9/question/que_42/reply"
                );
                assert_eq!(
                    reply_body(&route, "1").unwrap(),
                    json!({"answers": [["Alpha"]]})
                );
                assert_eq!(
                    reply_body(&route, "3").unwrap(),
                    json!({"answers": [["Gamma"]]})
                );
                assert!(reply_body(&route, "9").is_none());
            }
            other => panic!("expected Prompt, got {other:?}"),
        }
    }

    #[test]
    fn parses_permission_asked_with_three_replies() {
        let event = json!({
            "id": "evt_2",
            "type": "permission.asked",
            "properties": {
                "id": "per_7",
                "sessionID": "ses_5",
                "permission": "bash",
                "metadata": {
                    "command": "printf X",
                    "description": "Print X string"
                }
            }
        });
        match parse_event(&event) {
            ParsedEvent::Prompt { prompt, route } => {
                assert_eq!(prompt.question, "Print X string");
                let values: Vec<_> = prompt.options.iter().map(|o| o.value.as_str()).collect();
                assert_eq!(values, vec!["once", "always", "reject"]);
                assert_eq!(route.kind, PromptKind::Permission);
                assert_eq!(reply_endpoint(&route), "/session/ses_5/permissions/per_7");
                assert_eq!(
                    reply_body(&route, "1").unwrap(),
                    json!({"response": "once"})
                );
                assert_eq!(
                    reply_body(&route, "always").unwrap(),
                    json!({"response": "always"})
                );
                assert!(reply_body(&route, "4").is_none());
            }
            other => panic!("expected Prompt, got {other:?}"),
        }
    }

    #[test]
    fn permission_falls_back_to_command_then_permission_label() {
        let event = json!({
            "type": "permission.asked",
            "properties": {
                "id": "per_8", "sessionID": "ses_5",
                "permission": "edit",
                "metadata": {"command": "apply patch"}
            }
        });
        let ParsedEvent::Prompt { prompt, .. } = parse_event(&event) else {
            panic!("expected prompt");
        };
        assert_eq!(prompt.question, "apply patch");
    }

    #[test]
    fn replied_event_maps_to_clear() {
        let event = json!({
            "type": "question.replied",
            "properties": {"sessionID": "ses_9", "requestID": "que_42", "answers": [["Alpha"]]}
        });
        assert_eq!(
            parse_event(&event),
            ParsedEvent::Replied {
                request_id: "que_42".into()
            }
        );
    }

    #[test]
    fn server_connected_and_unknown_are_ignored() {
        let observed: Value =
            serde_json::from_str(include_str!("fixtures/observed_server_connected.json"))
                .expect("captured server.connected frame");
        assert_eq!(parse_event(&observed), ParsedEvent::Ignored);
        assert_eq!(
            parse_event(&json!({"type": "unrecognized.event"})),
            ParsedEvent::Ignored
        );
    }

    #[test]
    fn message_part_update_uses_full_text_and_stable_part_id() {
        let message_frame: Value = serde_json::from_str(include_str!(
            "fixtures/sdk_shape_message_updated.json"
        ))
        .unwrap();
        let message = parse_event(&message_frame);
        let ParsedEvent::MessageUpdated { message } = message else {
            panic!("expected message metadata");
        };
        assert_eq!(message.message_id, "msg_1");
        assert_eq!(message.session_id, "ses_1");
        assert_eq!(message.role, "assistant");

        let first_frame: Value = serde_json::from_str(include_str!(
            "fixtures/sdk_shape_message_part_text_early.json"
        ))
        .unwrap();
        let first = parse_event(&first_frame);
        let ParsedEvent::PartUpdated { update } = first else {
            panic!("expected part update");
        };
        assert_eq!(update.delta.as_deref(), Some("Hel"));
        let partial = normalize_chat_part(&update, &message, None).unwrap();
        assert_eq!(partial.id, "prt_text");
        assert_eq!(partial.kind, "assistant");
        assert_eq!(partial.content, "Hel");
        assert_eq!(partial.timestamp.as_deref(), Some("1710000000000"));

        let second_frame: Value = serde_json::from_str(include_str!(
            "fixtures/sdk_shape_message_part_text_delta.json"
        ))
        .unwrap();
        let second = parse_event(&second_frame);
        let ParsedEvent::PartUpdated { update } = second else {
            panic!("expected part update");
        };
        let grown = normalize_chat_part(&update, &message, Some("Hel")).unwrap();
        assert_eq!(grown.id, partial.id);
        assert_eq!(grown.content, "Hello");
    }

    #[test]
    fn message_part_update_normalizes_thinking_and_tool_parts() {
        let message = MessageMeta {
            session_id: "ses_2".into(),
            message_id: "msg_2".into(),
            role: "assistant".into(),
            timestamp: Some("1710000000000".into()),
        };
        let reasoning_frame: Value = serde_json::from_str(include_str!(
            "fixtures/sdk_shape_message_part_reasoning.json"
        ))
        .unwrap();
        let ParsedEvent::PartUpdated { update: reasoning } = parse_event(&reasoning_frame) else {
            panic!("expected reasoning part update");
        };
        let thinking = normalize_chat_part(&reasoning, &message, None).unwrap();
        assert_eq!(thinking.kind, "thinking");
        assert_eq!(thinking.content, "Need inspect");

        let tool_frame: Value = serde_json::from_str(include_str!(
            "fixtures/sdk_shape_message_part_tool.json"
        ))
        .unwrap();
        let ParsedEvent::PartUpdated { update: tool } = parse_event(&tool_frame) else {
            panic!("expected tool part update");
        };
        let call = normalize_chat_part(&tool, &message, None).unwrap();
        assert_eq!(call.id, "prt_tool");
        assert_eq!(call.kind, "tool_call");
        assert_eq!(call.tool_name.as_deref(), Some("bash"));
        assert_eq!(call.tool_kind.as_deref(), Some("command"));
        assert_eq!(call.tool_summary.as_deref(), Some("printf X"));
    }

    #[test]
    fn part_update_requires_matching_message_and_session() {
        let message = MessageMeta {
            session_id: "ses_owner".into(),
            message_id: "msg_owner".into(),
            role: "assistant".into(),
            timestamp: None,
        };
        let part = ChatPartUpdate {
            session_id: "ses_sibling".into(),
            message_id: "msg_owner".into(),
            part_id: "prt_sibling".into(),
            part: json!({"type":"text", "text":"sibling"}),
            delta: Some("sibling".into()),
        };
        assert!(normalize_chat_part(&part, &message, None).is_none());
    }

    #[test]
    fn parses_current_permission_update_and_reply_shapes() {
        let event: Value = serde_json::from_str(include_str!(
            "fixtures/sdk_shape_permission_updated.json"
        ))
        .unwrap();
        let ParsedEvent::Prompt { prompt, route } = parse_event(&event) else {
            panic!("expected prompt");
        };
        assert_eq!(prompt.question, "Run command");
        assert_eq!(route.request_id, "per_9");
        assert_eq!(
            parse_event(
                &serde_json::from_str(include_str!(
                    "fixtures/sdk_shape_permission_replied.json"
                ))
                .unwrap()
            ),
            ParsedEvent::Replied { request_id: "per_9".into() }
        );
    }

    #[test]
    fn multi_select_question_marks_submit_unavailable() {
        let event = json!({
            "type": "question.asked",
            "properties": {
                "id": "que_1", "sessionID": "ses_1",
                "questions": [{
                    "question": "Pick many", "multiple": true,
                    "options": [{"label": "A"}, {"label": "B"}]
                }]
            }
        });
        let ParsedEvent::Prompt { prompt, route } = parse_event(&event) else {
            panic!("expected prompt");
        };
        assert_eq!(prompt.multi_select, Some(true));
        assert_eq!(prompt.submit_available, Some(false));
        // The multi-select route still carries only ordered labels — see the GAP test.
        assert_eq!(route.option_labels, vec!["A".to_string(), "B".to_string()]);
    }

    #[test]
    fn question_reply_body_is_label_based_index_and_label_equivalent() {
        let route = ResponseRoute {
            session_id: "ses_x".into(),
            request_id: "que_x".into(),
            kind: PromptKind::Question,
            option_labels: vec!["Alpha".into(), "Beta".into()],
        };
        assert_eq!(
            reply_body(&route, "1").unwrap(),
            json!({"answers": [["Alpha"]]})
        );
        assert_eq!(
            reply_body(&route, "Alpha").unwrap(),
            json!({"answers": [["Alpha"]]}),
            "label resolves identically to its 1-based index"
        );
        assert_eq!(
            reply_body(&route, "2").unwrap(),
            json!({"answers": [["Beta"]]})
        );
        assert!(
            reply_body(&route, "3").is_none(),
            "out-of-range index -> None"
        );
        assert!(
            reply_body(&route, "Gamma").is_none(),
            "unknown label -> None"
        );
    }

    #[test]
    fn multi_select_question_reply_body_is_single_choice_today_gap() {
        let route = ResponseRoute {
            session_id: "ses_m".into(),
            request_id: "que_m".into(),
            kind: PromptKind::Question,
            option_labels: vec!["A".into(), "B".into(), "C".into()],
        };
        assert_eq!(
            reply_body(&route, "1").unwrap(),
            json!({"answers": [["A"]]})
        );
        assert_eq!(
            reply_body(&route, "3").unwrap(),
            json!({"answers": [["C"]]})
        );
    }
}
