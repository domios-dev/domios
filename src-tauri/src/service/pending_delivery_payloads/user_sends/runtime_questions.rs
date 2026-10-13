use super::*;
use crate::service::agent_runtime::questions::{QuestionReceipt, QuestionRecord};

pub(crate) struct QuestionResolution {
    pub thread_id: String,
    pub item_id: String,
    pub receipt: QuestionReceipt,
}

impl UserSendStore {
    pub(crate) fn runtime_questions(
        &self,
        tab_id: &str,
        thread: &str,
    ) -> Result<Vec<QuestionRecord>, String> {
        let tab = self.tab(tab_id);
        let mut data = tab.data.lock();
        Ok(self
            .load(&tab, &mut data)?
            .index
            .runtime_questions
            .list(thread))
    }

    pub(crate) fn persist_runtime_question(
        &self,
        tab_id: &str,
        question: QuestionRecord,
    ) -> Result<Vec<QuestionRecord>, String> {
        let tab = self.tab(tab_id);
        let mut data = tab.data.lock();
        let loaded = self.load(&tab, &mut data)?;
        let mut next = loaded.index.clone();
        let thread = question.thread_id.clone();
        next.runtime_questions.reconcile(question);
        if next.runtime_questions.list(&thread).len() > 1024 {
            return Err("Runtime question retention capacity exhausted".into());
        }
        self.persist(&tab, &next)?;
        loaded.index = next;
        Ok(loaded.index.runtime_questions.list(&thread))
    }

    pub(crate) fn persist_runtime_history_questions(
        &self,
        tab_id: &str,
        thread: &str,
        questions: Vec<codeterm_plugin::agent_runtime::RuntimeHistoryQuestion>,
    ) -> Result<Vec<QuestionRecord>, String> {
        let tab = self.tab(tab_id);
        let mut data = tab.data.lock();
        let loaded = self.load(&tab, &mut data)?;
        let mut next = loaded.index.clone();
        for question in questions {
            if question.thread_id != thread {
                return Err("History question thread changed".into());
            }
            let record = QuestionRecord {
                thread_id: question.thread_id,
                item_id: question.item_id,
                questions: question.questions,
                response: question.response,
                created_at: chrono::Utc::now().timestamp_millis().max(0) as u64,
                resolved: None,
            };
            next.runtime_questions
                .reconcile_history(record, question.resolved.unwrap_or(false));
        }
        self.persist(&tab, &next)?;
        loaded.index = next;
        Ok(loaded.index.runtime_questions.list(thread))
    }

    pub(crate) fn persist_runtime_status(
        &self,
        status: &crate::service::chat_send_transaction::ChatSendStatus,
    ) -> Result<(), String> {
        let tab = self.tab(&status.tab_id);
        let mut data = tab.data.lock();
        let loaded = self.load(&tab, &mut data)?;
        let mut next = loaded.index.clone();
        let mut changed = next.runtime_questions.update_status(status)?;
        if status.state == crate::service::chat_send_transaction::ChatSendState::Sent {
            if let Some(record) = next
                .records
                .iter()
                .find(|record| record.send_id == status.send_id)
                .cloned()
            {
                continuity::retain_receipt(&mut next, &record, continuity::ReceiptState::Delivered);
                changed = true;
            }
        }
        if changed {
            self.persist(&tab, &next)?;
            loaded.index = next;
        }
        Ok(())
    }

    pub(crate) fn admit_question_answer(
        &self,
        reservation: &UserReservation,
        text: &str,
        question: QuestionResolution,
        held_before_readiness: bool,
    ) -> Result<UserSendRecord, String> {
        self.admit_with_question(
            reservation,
            text,
            text,
            None,
            None,
            held_before_readiness,
            Some(question),
            false,
        )
    }
}

#[cfg(test)]
mod tests;
