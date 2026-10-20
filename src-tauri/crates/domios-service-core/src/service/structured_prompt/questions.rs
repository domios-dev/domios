use serde_json::{json, Value};

use super::opencode_sse::{PromptKind, ResponseRoute};
use crate::multiplexer::types::{PendingPrompt, PromptOption};
use codeterm_types::pty::PromptSection;

#[derive(Debug, Clone, PartialEq)]
pub struct Question {
    pub header: String,
    pub text: String,
    pub labels: Vec<String>,
    pub multiple: bool,
    pub custom: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Answer {
    Next {
        route: ResponseRoute,
        prompt: PendingPrompt,
    },
    Complete {
        body: Value,
        option_index: usize,
    },
}

pub(super) fn parse(value: &Value) -> Option<Vec<Question>> {
    let questions = value
        .as_array()?
        .iter()
        .map(|question| {
            Some(Question {
                header: question
                    .get("header")
                    .and_then(Value::as_str)
                    .unwrap_or("Question")
                    .into(),
                text: question.get("question")?.as_str()?.into(),
                labels: question
                    .get("options")?
                    .as_array()?
                    .iter()
                    .map(|option| option.get("label")?.as_str().map(String::from))
                    .collect::<Option<_>>()?,
                multiple: question
                    .get("multiple")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                custom: question
                    .get("custom")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    (!questions.is_empty()).then_some(questions)
}

pub(super) fn prompt(route: &ResponseRoute) -> Option<PendingPrompt> {
    let index = route.answers.len();
    let question = route.questions.get(index)?;
    let mut options: Vec<_> = question
        .labels
        .iter()
        .enumerate()
        .map(|(index, label)| PromptOption {
            label: label.clone(),
            value: (index + 1).to_string(),
        })
        .collect();
    if question.custom && !question.multiple {
        options.push(PromptOption {
            label: "Other".into(),
            value: "custom".into(),
        });
    }
    Some(PendingPrompt {
        request_id: None,
        question: question.text.clone(),
        options,
        sections: (route.questions.len() > 1).then(|| {
            route
                .questions
                .iter()
                .enumerate()
                .map(|(step, question)| PromptSection {
                    label: question.header.clone(),
                    active: step == index,
                    completed: step < index,
                })
                .collect()
        }),
        multi_select: Some(question.multiple),
        submit_available: Some(false),
        selected_values: Some(Vec::new()),
    })
}

fn choices(question: &Question, value: &str) -> Option<Vec<String>> {
    if question.multiple {
        if let Ok(values) = serde_json::from_str::<Vec<String>>(value) {
            let mut labels = Vec::new();
            for value in values {
                let choice = single_choice(question, &value)?;
                if !labels.contains(&choice) {
                    labels.push(choice);
                }
            }
            return Some(labels);
        }
        if let Some(mask) = value
            .parse::<u64>()
            .ok()
            .and_then(|value| value.checked_sub(10000))
        {
            if question.labels.len() > 31 || mask >> question.labels.len() != 0 {
                return None;
            }
            return Some(
                question
                    .labels
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| mask & (1 << index) != 0)
                    .map(|(_, label)| label.clone())
                    .collect(),
            );
        }
    }
    single_choice(question, value).map(|choice| vec![choice])
}

fn single_choice(question: &Question, value: &str) -> Option<String> {
    if let Ok(index) = value.parse::<usize>() {
        return question.labels.get(index.checked_sub(1)?).cloned();
    }
    if question.labels.iter().any(|label| label == value)
        || (question.custom && !value.trim().is_empty() && value != "custom")
    {
        Some(value.to_string())
    } else {
        None
    }
}

pub(super) fn answer(route: &ResponseRoute, value: &str) -> Option<Answer> {
    if matches!(
        route.kind,
        PromptKind::Permission | PromptKind::PermissionV2
    ) {
        let index = value
            .parse::<usize>()
            .ok()
            .and_then(|value| value.checked_sub(1))
            .or_else(|| route.option_labels.iter().position(|label| label == value))?;
        return Some(Answer::Complete {
            body: json!({"reply":route.option_labels.get(index)?}),
            option_index: index,
        });
    }
    let question = route.questions.get(route.answers.len())?;
    let selected = choices(question, value)?;
    let option_index = selected
        .first()
        .and_then(|label| question.labels.iter().position(|option| option == label))
        .unwrap_or(question.labels.len());
    let mut next = route.clone();
    next.answers.push(selected);
    if next.answers.len() == next.questions.len() {
        return Some(Answer::Complete {
            body: json!({"answers":next.answers}),
            option_index,
        });
    }
    next.option_labels = next.questions.get(next.answers.len())?.labels.clone();
    let prompt = prompt(&next)?;
    Some(Answer::Next {
        route: next,
        prompt,
    })
}

#[cfg(test)]
mod tests;
