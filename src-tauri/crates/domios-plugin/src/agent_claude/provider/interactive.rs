//! Claude interactive-prompt extraction (AskUserQuestion).
//!
//! Structural fingerprint only: a `tool_use` block whose tool name is
//! `AskUserQuestion` carrying `input.questions[]`, each with an `options[]`
//! array. No human display copy is matched — vendors change wording freely.

use codeterm_types::agent::chat_types::{
    InteractiveElement, InteractiveOption, InteractiveQuestion, RichMessage,
};
use codeterm_types::pty::{PendingPrompt, PromptOption, PromptSection};

const ASK_USER_QUESTION: &str = "AskUserQuestion";

pub(super) fn parse_screen_prompt(screen: &str) -> Option<PendingPrompt> {
    let lines = screen.lines().collect::<Vec<_>>();
    let tab_line = lines.iter().rposition(|line| {
        line.contains("Submit") && line.chars().any(|character| is_checkbox(character))
    })?;
    let body = &lines[tab_line + 1..];
    let mut checkbox_index = 0;
    let rows = body
        .iter()
        .enumerate()
        .filter_map(|(index, line)| {
            let mut option = parse_screen_option(line, index)?;
            if option.checkbox {
                checkbox_index += 1;
                option.value = checkbox_index.to_string();
            }
            Some(option)
        })
        .collect::<Vec<_>>();
    if rows.len() < 2 {
        return None;
    }
    let first = rows.first()?.line;
    let question = body[..first]
        .iter()
        .rev()
        .map(|line| line.trim())
        .find(|line| line.ends_with('?'))?
        .to_string();
    let multi_select = rows.iter().any(|option| option.checkbox);
    let sections = parse_sections(lines[tab_line]);
    let selected_values = rows
        .iter()
        .filter(|option| option.selected)
        .map(|option| option.value.clone())
        .collect();

    Some(PendingPrompt {
        request_id: None,
        question,
        options: rows
            .into_iter()
            .map(|option| PromptOption {
                label: option.label,
                value: option.value,
            })
            .collect(),
        sections: (!sections.is_empty()).then_some(sections),
        multi_select: Some(multi_select),
        submit_available: Some(lines[tab_line].contains("Submit")),
        selected_values: Some(selected_values),
    })
}

struct ScreenOption {
    label: String,
    value: String,
    line: usize,
    checkbox: bool,
    selected: bool,
}

fn parse_screen_option(line: &str, row: usize) -> Option<ScreenOption> {
    let trimmed = line
        .trim()
        .trim_start_matches(|character| matches!(character, '❯' | '>' | '›' | '▶' | '➤'))
        .trim();
    let marker = trimmed.chars().next()?;
    if is_checkbox(marker) {
        let (label, selected) = if marker == '[' {
            let mark = trimmed.get(0..3)?;
            if !matches!(mark, "[ ]" | "[x]" | "[X]") {
                return None;
            }
            (trimmed.get(3..)?.trim(), mark != "[ ]")
        } else {
            (
                trimmed[marker.len_utf8()..].trim(),
                matches!(marker, '☑' | '☒' | '✔'),
            )
        };
        let label = normalize_option_label(label);
        if label.is_empty() {
            return None;
        }
        return Some(ScreenOption {
            label,
            value: (row + 1).to_string(),
            line: row,
            checkbox: true,
            selected,
        });
    }
    let (number, label) = trimmed.split_once(". ")?;
    if number.is_empty() || !number.chars().all(|character| character.is_ascii_digit()) {
        return None;
    }
    let label = normalize_option_label(label);
    (!label.is_empty()).then(|| ScreenOption {
        label,
        value: number.to_string(),
        line: row,
        checkbox: false,
        selected: false,
    })
}

fn normalize_option_label(label: &str) -> String {
    label.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn parse_sections(line: &str) -> Vec<PromptSection> {
    let mut sections = Vec::new();
    let characters: Vec<(usize, char)> = line.char_indices().collect();
    let mut position = 0;
    while position < characters.len() {
        let marker = characters[position].1;
        position += 1;
        if !is_checkbox(marker) {
            continue;
        }
        let start = characters
            .get(position)
            .map_or(line.len(), |(index, _)| *index);
        let stop = characters[position..]
            .iter()
            .position(|(_, character)| is_checkbox(*character) || *character == '→')
            .map(|offset| position + offset);
        let end = stop.map_or(line.len(), |at| characters[at].0);
        let label = line[start..end].trim();
        position = stop.unwrap_or(characters.len());
        if label.is_empty() || label.eq_ignore_ascii_case("submit") {
            continue;
        }
        sections.push(PromptSection {
            label: label.to_string(),
            active: false,
            completed: marker == '✔' || marker == '☑' || marker == '☒',
        });
    }
    if let Some(active) = sections.iter_mut().find(|section| !section.completed) {
        active.active = true;
    }
    sections
}

fn is_checkbox(character: char) -> bool {
    matches!(character, '[' | '☐' | '☑' | '☒' | '✔')
}

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
                        value: None,
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
    use codeterm_types::agent::chat_types::MessageBlock;
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

    #[test]
    fn screen_option_labels_collapse_internal_whitespace() {
        let screen = [
            "← ☐ Environment ☐ Scope ✔ Submit →",
            "Which token should be set?",
            "❯ 1. opaque-alpha      opaque-beta",
            "  2. opaque-gamma",
            "Enter to select · Esc to cancel",
        ]
        .join("\n");
        let prompt = parse_screen_prompt(&screen).expect("interactive screen prompt");
        assert_eq!(prompt.options[0].label, "opaque-alpha opaque-beta");
    }
}
