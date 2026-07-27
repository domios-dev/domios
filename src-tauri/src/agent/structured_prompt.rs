//! Structured pending-prompt construction — provider-agnostic core.
//!
//! When a provider exposes a STRUCTURED prompt source (claude's AskUserQuestion
//! in the parsed JSONL transcript, opencode's SSE event bus), core builds
//! `tab.pending_prompt` from that handle instead of scraping the ephemeral PTY
//! picker. This module owns the pure transforms shared across providers:
//!
//! - [`active_question_prompt`] — pick the live (unanswered) AskUserQuestion out
//!   of a window of parsed transcript messages and shape it into a
//!   [`PendingPrompt`]. Used by the claude pull path.
//!
//! Pure + deterministic — no I/O, no locks. The provider-specific I/O (reading
//! the JSONL tail, subscribing to SSE) lives at the boundary and feeds this.

use std::collections::HashSet;

use crate::agent::chat_types::{InteractiveElement, InteractiveQuestion, RichMessage};
use crate::multiplexer::types::{PendingPrompt, PromptOption, PromptSection};

/// Build a [`PendingPrompt`] from the most recent UNANSWERED interactive
/// question in `messages` (oldest→newest order). A question is answered once a
/// later message carries a `tool_result` block for its originating tool_use id.
/// Returns `None` when there is no interactive question, or every one already
/// has a matching answer.
///
/// `messages` must already be enriched (each carries `interactive`, populated by
/// the provider's `parse_interactive_events`).
pub fn active_question_prompt(messages: &[RichMessage]) -> Option<PendingPrompt> {
    let answered = answered_tool_use_ids(messages);
    for msg in messages.iter().rev() {
        let questions = questions_of(msg);
        let Some(first) = questions.first() else {
            continue;
        };
        // are answered together by a single tool_result.
        if answered.contains(first.tool_use_id.as_str()) {
            continue;
        }
        return Some(prompt_from_questions(&questions));
    }
    None
}

/// tool_use ids that have a corresponding `tool_result` block anywhere in the
/// window — i.e. the user has already answered them.
fn answered_tool_use_ids(messages: &[RichMessage]) -> HashSet<&str> {
    let mut answered = HashSet::new();
    for msg in messages {
        for block in &msg.blocks {
            if block.kind != "tool_result" {
                continue;
            }
            if let Some(id) = block.data.get("tool_use_id").and_then(|v| v.as_str()) {
                answered.insert(id);
            }
        }
    }
    answered
}

fn questions_of(msg: &RichMessage) -> Vec<&InteractiveQuestion> {
    msg.interactive
        .iter()
        .map(|e| match e {
            InteractiveElement::Question(q) => q,
        })
        .collect()
}

/// Shape one AskUserQuestion (one or more sub-questions, all pending together)
/// into the flat `PendingPrompt` the UI + send-back path consume. The first
/// sub-question's options drive the picker; when there are several, each
/// question becomes a `PromptSection` (the first active) so the multi-step
/// wizard renders. Option `value`s are 1-based strings — same contract the
/// native detector uses, so `build_prompt_response(option_index, …)` navigates
/// identically for both paths.
fn prompt_from_questions(questions: &[&InteractiveQuestion]) -> PendingPrompt {
    let first = questions[0];
    let options = first
        .options
        .iter()
        .enumerate()
        .map(|(idx, opt)| PromptOption {
            label: opt.label.clone(),
            value: (idx + 1).to_string(),
        })
        .collect();
    let sections = (questions.len() > 1).then(|| {
        questions
            .iter()
            .enumerate()
            .map(|(idx, q)| PromptSection {
                label: q
                    .header
                    .clone()
                    .filter(|h| !h.is_empty())
                    .unwrap_or_else(|| q.prompt.clone()),
                active: idx == 0,
                completed: false,
            })
            .collect()
    });
    PendingPrompt {
        question: first.prompt.clone(),
        options,
        sections,
        multi_select: Some(first.multi_select),
        submit_available: Some(!first.multi_select),
        selected_values: Some(Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::chat_types::{InteractiveOption, MessageBlock};
    use serde_json::json;

    fn question(tool_use_id: &str, idx: usize, multi: bool) -> InteractiveQuestion {
        InteractiveQuestion {
            id: format!("{tool_use_id}-{idx}"),
            tool_use_id: tool_use_id.to_string(),
            prompt: format!("Pick {idx}"),
            header: Some(format!("H{idx}")),
            options: vec![
                InteractiveOption {
                    label: "Alpha".into(),
                    description: None,
                },
                InteractiveOption {
                    label: "Beta".into(),
                    description: Some("the second".into()),
                },
            ],
            multi_select: multi,
        }
    }

    fn msg_with_question(tool_use_id: &str, multi: bool) -> RichMessage {
        RichMessage {
            role: "assistant".into(),
            blocks: vec![MessageBlock {
                kind: "tool_use".into(),
                data: json!({"id": tool_use_id, "name": "AskUserQuestion"}),
            }],
            interactive: vec![InteractiveElement::Question(question(
                tool_use_id,
                0,
                multi,
            ))],
            ..Default::default()
        }
    }

    fn tool_result(tool_use_id: &str) -> RichMessage {
        RichMessage {
            role: "user".into(),
            blocks: vec![MessageBlock {
                kind: "tool_result".into(),
                data: json!({"tool_use_id": tool_use_id, "content": "Alpha"}),
            }],
            ..Default::default()
        }
    }

    #[test]
    fn unanswered_question_becomes_prompt() {
        let msgs = vec![msg_with_question("toolu_1", false)];
        let prompt = active_question_prompt(&msgs).expect("active prompt");
        assert_eq!(prompt.question, "Pick 0");
        assert_eq!(prompt.options.len(), 2);
        assert_eq!(prompt.options[0].label, "Alpha");
        assert_eq!(prompt.options[0].value, "1");
        assert_eq!(prompt.options[1].value, "2");
        assert_eq!(prompt.multi_select, Some(false));
        assert_eq!(prompt.submit_available, Some(true));
        assert!(prompt.sections.is_none());
    }

    #[test]
    fn answered_question_yields_none() {
        let msgs = vec![msg_with_question("toolu_1", false), tool_result("toolu_1")];
        assert!(active_question_prompt(&msgs).is_none());
    }

    #[test]
    fn newest_unanswered_wins_over_older_answered() {
        let msgs = vec![
            msg_with_question("toolu_1", false),
            tool_result("toolu_1"),
            msg_with_question("toolu_2", true),
        ];
        let prompt = active_question_prompt(&msgs).expect("second question is live");
        assert_eq!(prompt.multi_select, Some(true));
        assert_eq!(prompt.submit_available, Some(false));
    }

    #[test]
    fn multi_question_ask_user_question_emits_sections() {
        let mut msg = msg_with_question("toolu_3", false);
        msg.interactive
            .push(InteractiveElement::Question(question("toolu_3", 1, false)));
        let prompt = active_question_prompt(&[msg]).expect("active");
        let sections = prompt.sections.expect("multi-question → sections");
        assert_eq!(sections.len(), 2);
        assert!(sections[0].active);
        assert!(!sections[1].active);
        assert_eq!(sections[0].label, "H0");
    }

    #[test]
    fn no_interactive_yields_none() {
        let msg = RichMessage {
            role: "assistant".into(),
            blocks: vec![MessageBlock {
                kind: "text".into(),
                data: json!({"text": "hi"}),
            }],
            ..Default::default()
        };
        assert!(active_question_prompt(&[msg]).is_none());
    }
}
