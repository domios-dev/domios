//! OpenCode SSE event → structured prompt mapping (pure).
//!
//! Maps the local opencode server's `/event` bus payloads onto a
//! provider-agnostic [`PendingPrompt`] plus the routing needed to answer over
//! HTTP. Event and endpoint shapes are proven live and documented in
//! `docs/plans/2026-06-20-opencode-structured-prompt-handle.md`.
//!
//! Pure + deterministic: no I/O, no locks. The subscriber thread feeds raw
//! `data:` JSON in and gets a [`ParsedEvent`] back; the HTTP layer turns a
//! reply into a request via [`reply_endpoint`] / [`reply_body`].

use serde_json::{json, Value};

use crate::multiplexer::types::{PendingPrompt, PromptOption};

/// What the host does in response to one SSE line.
#[derive(Debug, Clone, PartialEq)]
pub enum ParsedEvent {
    /// A new structured prompt is pending — surface it and remember its route.
    Prompt {
        prompt: PendingPrompt,
        route: ResponseRoute,
    },
    /// A prompt was answered (by us or elsewhere) — clear it.
    Replied { request_id: String },
    /// Anything else (server.connected, deltas, …) — ignore.
    Ignored,
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

/// Parse one SSE `data:` JSON object. Unknown / non-prompt events map to
/// [`ParsedEvent::Ignored`].
pub fn parse_event(value: &Value) -> ParsedEvent {
    let event_type = value.get("type").and_then(|t| t.as_str()).unwrap_or("");
    let props = value.get("properties").unwrap_or(&Value::Null);
    match event_type {
        "question.asked" | "question.v2.asked" => parse_question(props),
        "permission.asked" => parse_permission(props),
        "question.replied" | "permission.replied" => props
            .get("requestID")
            .and_then(|v| v.as_str())
            .map(|id| ParsedEvent::Replied {
                request_id: id.to_string(),
            })
            .unwrap_or(ParsedEvent::Ignored),
        _ => ParsedEvent::Ignored,
    }
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

/// Session-scoped v2 reply endpoint (preferred), relative to the server base.
pub fn reply_endpoint(route: &ResponseRoute) -> String {
    match route.kind {
        PromptKind::Question => format!(
            "/api/session/{}/question/{}/reply",
            route.session_id, route.request_id
        ),
        PromptKind::Permission => format!(
            "/api/session/{}/permission/{}/reply",
            route.session_id, route.request_id
        ),
    }
}

/// Build the JSON reply body for a chosen 1-based option `value`.
/// For a question the answer is the option label; for a permission it is the
/// reply token (`once` / `always` / `reject`). Returns `None` if the index is
/// out of range.
pub fn reply_body(route: &ResponseRoute, value: &str) -> Option<Value> {
    let index = value.parse::<usize>().ok()?.checked_sub(1)?;
    let choice = route.option_labels.get(index)?.clone();
    Some(match route.kind {
        PromptKind::Question => json!({ "answers": [[choice]] }),
        PromptKind::Permission => json!({ "reply": choice }),
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
                assert_eq!(
                    reply_endpoint(&route),
                    "/api/session/ses_5/permission/per_7/reply"
                );
                assert_eq!(reply_body(&route, "2").unwrap(), json!({"reply": "always"}));
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
        assert_eq!(
            parse_event(&json!({"type": "server.connected"})),
            ParsedEvent::Ignored
        );
        assert_eq!(
            parse_event(&json!({"type": "message.part.updated"})),
            ParsedEvent::Ignored
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
        let ParsedEvent::Prompt { prompt, .. } = parse_event(&event) else {
            panic!("expected prompt");
        };
        assert_eq!(prompt.multi_select, Some(true));
        assert_eq!(prompt.submit_available, Some(false));
    }
}
