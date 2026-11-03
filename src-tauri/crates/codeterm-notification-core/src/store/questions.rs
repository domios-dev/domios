use super::*;

impl NotificationStore {
    pub fn clear_resolved(&mut self, id: Option<&str>) -> Vec<(String, Option<SpentQuestion>)> {
        let ids: Vec<_> = self
            .list(false)
            .into_iter()
            .filter(|record| id.is_none_or(|id| record.id == id))
            .map(|record| record.id.clone())
            .collect();
        let records = ids
            .into_iter()
            .map(|id| {
                self.resolve_id(&id);
                let spent = self.spent_question(&id);
                (id, spent)
            })
            .collect();
        self.clear(id);
        records
    }

    pub fn resolve_question(&mut self, id: &str, caller: Option<&str>) -> Result<(), String> {
        let record = self.notification(id).ok_or("Notification not found")?;
        if record
            .answer
            .as_ref()
            .is_some_and(|slot| slot.daemon_request.is_some())
            || record.surface_offer.is_some()
            || record.condition.is_some()
        {
            return Err("Host decisions require their offered action".into());
        }
        let asking_tab = match record.answer.as_ref() {
            Some(slot) => slot.asked_from_tab.as_deref(),
            None if record.level == Some(NotifyLevel::ActionNeeded) => record.tab_id.as_deref(),
            None => return Err("Notification is not a question".into()),
        };
        if caller.is_some() && asking_tab != caller {
            return Err("Only the asking tab may resolve this question".into());
        }
        self.resolve_id(id);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
