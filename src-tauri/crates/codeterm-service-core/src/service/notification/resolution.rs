use super::*;

impl ServiceContext {
    pub fn clear_notifications(&self, id: Option<&str>) {
        let spent = self.notif.lock().clear_resolved(id);
        for (id, question) in spent {
            if let Some(question) = question {
                self.spend_on_channels(&id, &question);
            }
            self.notify_event("notification-cleared", serde_json::json!({"id": id}));
        }
        self.reconcile_notification_questions();
    }

    pub fn resolve_notification(&self, id: &str, caller: Option<&str>) -> Result<(), String> {
        self.notif.lock().resolve_question(id, caller)?;
        self.announce_resolved(&[id.to_owned()]);
        self.reconcile_notification_questions();
        Ok(())
    }

    pub(crate) fn answer_notification_questions_in_chat(&self, tab: &str, sent_at: u64) {
        let ids = self.notif.lock().answer_questions_in_chat(tab, sent_at);
        self.announce_resolved(&ids);
        if !ids.is_empty() {
            self.reconcile_notification_questions();
        }
    }

    pub(in crate::service) fn reconcile_notification_questions(&self) {
        let Ok(now) = self.curator_now() else { return };
        for plan in self.plan_store.list_full() {
            self.reconcile_curator_id(&plan.id, now);
        }
    }
}
