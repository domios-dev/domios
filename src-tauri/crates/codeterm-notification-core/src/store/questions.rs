use super::*;

impl NotificationStore {
    pub fn clear_resolved(&mut self, id: Option<&str>) -> Vec<(String, Option<SpentQuestion>)> {
        let ids: Vec<_> = self.list(false).into_iter()
            .filter(|record| id.is_none_or(|id| record.id == id))
            .map(|record| record.id.clone()).collect();
        let records = ids.into_iter().map(|id| {
            self.resolve_id(&id);
            let spent = self.spent_question(&id);
            (id, spent)
        }).collect();
        self.clear(id);
        records
    }

    pub fn resolve_question(&mut self, id: &str, caller: Option<&str>) -> Result<(), String> {
        let record = self.notification(id).ok_or("Notification not found")?;
        let slot = record.answer.as_ref().ok_or("Notification is not a question")?;
        if slot.daemon_request.is_some() || record.surface_offer.is_some() || record.condition.is_some() {
            return Err("Host decisions require their offered action".into());
        }
        if caller.is_some() && slot.asked_from_tab.as_deref() != caller {
            return Err("Only the asking tab may resolve this question".into());
        }
        self.resolve_id(id);
        Ok(())
    }

    pub fn answer_questions_in_chat(&mut self, tab_id: &str, sent_at: u64) -> Vec<String> {
        let mut ids = Vec::new();
        for record in &mut self.notifications {
            if record.resolved || record.timestamp > sent_at || record.surface_offer.is_some()
                || record.condition.is_some() {
                continue;
            }
            let Some(slot) = record.answer.as_mut() else { continue };
            if slot.answered.is_some() || slot.daemon_request.is_some()
                || slot.asked_from_tab.as_deref() != Some(tab_id) {
                continue;
            }
            slot.answered = Some("Answered in chat".into());
            slot.answered_by = Some("chat".into());
            slot.answered_via = Some("chat".into());
            slot.delivered = true;
            ids.push(record.id.clone());
        }
        for id in &ids { self.resolve_id(id); }
        ids
    }
}

#[cfg(test)]
mod tests;
