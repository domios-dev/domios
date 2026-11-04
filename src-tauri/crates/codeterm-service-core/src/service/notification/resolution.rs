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

    pub(in crate::service) fn reconcile_notification_questions(&self) {
        let Ok(now) = self.curator_now() else { return };
        let ids = match crate::service::curator::curator_bootstrap_ids(&self.plan_store) {
            Ok(ids) => ids,
            Err(error) => {
                log::error!("[curator] notification selection: {error}");
                return;
            }
        };
        for id in ids {
            if let Err(error) = self.reconcile_curator_id(&id, now) {
                log::error!("[curator] notification reconciliation {id}: {error}");
            }
        }
    }
}
