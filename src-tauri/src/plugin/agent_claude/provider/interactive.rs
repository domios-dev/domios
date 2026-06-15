//! Claude interactive-prompt extraction (AskUserQuestion).
//!
//! Structural fingerprint only: a `tool_use` block whose tool name is
//! `AskUserQuestion` carrying `input.questions[]`, each with an `options[]`
//! array. No human display copy is matched — vendors change wording freely.

use crate::agent::chat_types::{
    InteractiveElement, InteractiveOption, InteractiveQuestion, RichMessage,
};

const ASK_USER_QUESTION: &str = "AskUserQuestion";

pub(super) fn parse_interactive_events(msg: &RichMessage) -> Vec<InteractiveElement> {
    let mut out = Vec::new();
    for block in &msg.blocks {
        if block.kind != "tool_use" {
            continue;
        }
        let data = &block.data;
        if data.get("name").and_then(|v| v.as_str()) != Some(ASK_USER_QUESTION) {
            continue;
        }
        let tool_use_id = data
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let Some(questions) = data
            .get("input")
            .and_then(|i| i.get("questions"))
            .and_then(|q| q.as_array())
        else {
            continue;
        };
        for (i, q) in questions.iter().enumerate() {
            let Some(opts_raw) = q.get("options").and_then(|o| o.as_array()) else {
                continue;
            };
            let options: Vec<InteractiveOption> = opts_raw
                .iter()
                .filter_map(|o| {
                    let label = o.get("label").and_then(|v| v.as_str())?.to_string();
                    Some(InteractiveOption {
                        label,
                        description: o
                            .get("description")
                            .and_then(|v| v.as_str())
                            .map(String::from),
                    })
                })
                .collect();
            let prompt = q
                .get("question")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            out.push(InteractiveElement::Question(InteractiveQuestion {
                id: format!("{tool_use_id}-{i}"),
                tool_use_id: tool_use_id.clone(),
                prompt,
                header: q.get("header").and_then(|v| v.as_str()).map(String::from),
                options,
                multi_select: q
                    .get("multiSelect")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
            }));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::chat_types::MessageBlock;
    use serde_json::json;

    fn ask_user_question_message() -> RichMessage {
        RichMessage {
            role: "assistant".into(),
            blocks: vec![
                MessageBlock {
                    kind: "text".into(),
                    data: json!({"text": "Let me ask"}),
                },
                MessageBlock {
                    kind: "tool_use".into(),
                    data: json!({
                        "id": "toolu_42",
                        "name": "AskUserQuestion",
                        "input": {
                            "questions": [
                                {
                                    "question": "Which database?",
                                    "header": "Database",
                                    "multiSelect": false,
                                    "options": [
                                        {"label": "Postgres", "description": "Relational"},
                                        {"label": "SQLite"}
                                    ]
                                },
                                {
                                    "question": "Which features?",
                                    "multiSelect": true,
                                    "options": [
                                        {"label": "Auth"},
                                        {"label": "Billing"}
                                    ]
                                }
                            ]
                        }
                    }),
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn parses_ask_user_question_into_questions() {
        let events = parse_interactive_events(&ask_user_question_message());
        assert_eq!(events.len(), 2);
        let InteractiveElement::Question(q0) = &events[0];
        assert_eq!(q0.id, "toolu_42-0");
        assert_eq!(q0.tool_use_id, "toolu_42");
        assert_eq!(q0.prompt, "Which database?");
        assert_eq!(q0.header.as_deref(), Some("Database"));
        assert!(!q0.multi_select);
        assert_eq!(q0.options.len(), 2);
        assert_eq!(q0.options[0].label, "Postgres");
        assert_eq!(q0.options[0].description.as_deref(), Some("Relational"));
        assert_eq!(q0.options[1].description, None);

        let InteractiveElement::Question(q1) = &events[1];
        assert_eq!(q1.id, "toolu_42-1");
        assert!(q1.multi_select);
        assert_eq!(q1.header, None);
    }

    #[test]
    fn non_ask_user_question_tool_use_yields_empty() {
        let msg = RichMessage {
            role: "assistant".into(),
            blocks: vec![MessageBlock {
                kind: "tool_use".into(),
                data: json!({
                    "id": "toolu_1",
                    "name": "Bash",
                    "input": {"command": "ls"}
                }),
            }],
            ..Default::default()
        };
        assert!(parse_interactive_events(&msg).is_empty());
    }

    #[test]
    fn question_without_options_is_skipped() {
        let msg = RichMessage {
            role: "assistant".into(),
            blocks: vec![MessageBlock {
                kind: "tool_use".into(),
                data: json!({
                    "id": "toolu_9",
                    "name": "AskUserQuestion",
                    "input": {"questions": [{"question": "no options here"}]}
                }),
            }],
            ..Default::default()
        };
        assert!(parse_interactive_events(&msg).is_empty());
    }
}
