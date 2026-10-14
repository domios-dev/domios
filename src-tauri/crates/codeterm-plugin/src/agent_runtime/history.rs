use super::*;
use codeterm_types::agent::chat_types::ChatMsg;
use serde::Deserialize;

#[derive(Default)]
pub struct RuntimeHistory {
    items: Vec<ChatMsg>,
    open_items: Vec<OpenItem>,
    questions: Vec<RuntimeHistoryQuestion>,
}

impl RuntimeHistory {
    pub fn append(&mut self, page: RuntimeResult) {
        let RuntimeResult::HistoryPage {
            items,
            open_items,
            questions,
            ..
        } = page
        else {
            return;
        };
        self.reconcile_questions(&items, questions);
        self.items.extend(items);
        self.open_items.extend(open_items);
    }

    pub fn reconcile_questions(
        &mut self,
        items: &[ChatMsg],
        questions: Option<Vec<RuntimeHistoryQuestion>>,
    ) {
        for question in questions.into_iter().flatten() {
            if let Some(old) = self
                .questions
                .iter_mut()
                .find(|old| old.item_id == question.item_id)
            {
                let resolved = old.resolved == Some(true) || question.resolved == Some(true);
                *old = question;
                old.resolved = resolved.then_some(true);
            } else {
                self.questions.push(question);
            }
        }
        for item in items {
            if item.kind != "user" {
                continue;
            }
            let Ok(answer) = serde_json::from_str::<HistoryAnswer>(&item.content) else {
                continue;
            };
            for question in &mut self.questions {
                if question.item_id == answer.item_id
                    && history_answers_match(&question.questions, &answer.answers)
                {
                    question.resolved = Some(true);
                }
            }
        }
    }

    pub fn questions(&self) -> Vec<RuntimeHistoryQuestion> {
        self.questions.clone()
    }

    pub fn finish(self, thread_id: String, next_cursor: Option<String>) -> RuntimeResult {
        RuntimeResult::HistoryPage {
            thread_id,
            items: self.items,
            open_items: self.open_items,
            next_cursor,
            questions: (!self.questions.is_empty()).then_some(self.questions),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryAnswer {
    item_id: String,
    answers: Vec<RuntimeQuestionAnswer>,
}

fn history_answers_match(questions: &[RuntimeQuestion], answers: &[RuntimeQuestionAnswer]) -> bool {
    if answers.len() != questions.len() {
        return false;
    }
    let ids: std::collections::HashSet<_> =
        answers.iter().map(|answer| &answer.question_id).collect();
    if ids.len() != answers.len() {
        return false;
    }
    questions.iter().all(|question| {
        let Some(answer) = answers
            .iter()
            .find(|answer| answer.question_id == question.id)
        else {
            return false;
        };
        let values: std::collections::HashSet<_> = answer.values.iter().collect();
        let text = answer.text.as_deref().filter(|text| !text.is_empty());
        values.len() == answer.values.len()
            && (question.multi_select.unwrap_or(false) || values.len() <= 1)
            && answer
                .values
                .iter()
                .all(|value| question.options.iter().any(|option| &option.value == value))
            && (answer.text.is_none()
                || (text.is_some() && question.allow_free_text != Some(false)))
            && (!values.is_empty() || text.is_some())
    })
}

#[cfg(test)]
mod tests;
