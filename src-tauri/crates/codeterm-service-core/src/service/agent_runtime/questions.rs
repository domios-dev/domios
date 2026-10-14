use super::model::RuntimeFailure;
use codeterm_plugin::agent_runtime::{
    RuntimeQuestion, RuntimeQuestionAnswer, RuntimeQuestionResponse,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionReceipt {
    pub answers: Vec<RuntimeQuestionAnswer>,
    pub send_id: String,
    pub status: serde_json::Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionRecord {
    pub thread_id: String,
    pub item_id: String,
    pub questions: Vec<RuntimeQuestion>,
    pub response: RuntimeQuestionResponse,
    pub created_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved: Option<QuestionReceipt>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct QuestionBook {
    records: Vec<QuestionRecord>,
    #[serde(default)]
    history_resolved: std::collections::HashSet<(String, String)>,
}

impl QuestionBook {
    pub fn reconcile(&mut self, record: QuestionRecord) {
        if let Some(existing) = self.records.iter_mut().find(|existing| {
            existing.thread_id == record.thread_id && existing.item_id == record.item_id
        }) {
            if existing.resolved.is_none() {
                existing.questions = record.questions;
            }
        } else {
            self.records.push(record);
        }
    }
    pub fn reconcile_history(&mut self, record: QuestionRecord, resolved: bool) {
        if resolved {
            self.history_resolved
                .insert((record.thread_id.clone(), record.item_id.clone()));
        }
        self.reconcile(record);
    }
    pub fn list(&self, thread: &str) -> Vec<QuestionRecord> {
        self.records
            .iter()
            .filter(|record| {
                record.thread_id == thread
                    && (record.resolved.is_some()
                        || !self
                            .history_resolved
                            .contains(&(record.thread_id.clone(), record.item_id.clone())))
            })
            .cloned()
            .collect()
    }
    pub fn get(&self, thread: &str, item: &str) -> Option<&QuestionRecord> {
        self.records
            .iter()
            .find(|record| record.thread_id == thread && record.item_id == item)
    }
    pub fn update_status(
        &mut self,
        status: &crate::service::chat_send_transaction::ChatSendStatus,
    ) -> Result<bool, String> {
        let mut changed = false;
        for record in &mut self.records {
            if let Some(receipt) = record
                .resolved
                .as_mut()
                .filter(|receipt| receipt.send_id == status.send_id)
            {
                receipt.status =
                    serde_json::to_value(status).map_err(|reason| reason.to_string())?;
                changed = true;
            }
        }
        Ok(changed)
    }
    pub fn resolve(
        &mut self,
        thread: &str,
        item: &str,
        receipt: QuestionReceipt,
    ) -> Result<QuestionReceipt, RuntimeFailure> {
        let record = self
            .records
            .iter_mut()
            .find(|record| record.thread_id == thread && record.item_id == item)
            .ok_or(RuntimeFailure::Superseded)?;
        if let Some(existing) = &record.resolved {
            return Ok(existing.clone());
        }
        validate_answers(&record.questions, &receipt.answers)?;
        record.resolved = Some(receipt.clone());
        Ok(receipt)
    }
}

pub fn validate_answers(
    questions: &[RuntimeQuestion],
    answers: &[RuntimeQuestionAnswer],
) -> Result<(), RuntimeFailure> {
    let ids = answers
        .iter()
        .map(|answer| &answer.question_id)
        .collect::<std::collections::HashSet<_>>();
    if answers.len() != questions.len() || ids.len() != answers.len() {
        return Err(RuntimeFailure::Refused(
            "Question answer set is incomplete or duplicated".into(),
        ));
    }
    for question in questions {
        let answer = answers
            .iter()
            .find(|answer| answer.question_id == question.id)
            .ok_or_else(|| RuntimeFailure::Refused("Question answer retargeted".into()))?;
        let values = answer
            .values
            .iter()
            .collect::<std::collections::HashSet<_>>();
        let text = answer
            .text
            .as_deref()
            .filter(|text| !text.trim().is_empty());
        if values.len() != answer.values.len()
            || (!question.multi_select.unwrap_or(false) && values.len() > 1)
            || answer
                .values
                .iter()
                .any(|value| !question.options.iter().any(|option| &option.value == value))
            || (text.is_some() && !question.allow_free_text.unwrap_or(false))
            || (values.is_empty() && text.is_none())
        {
            return Err(RuntimeFailure::Refused(
                "Answer does not match the persisted native question options".into(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
