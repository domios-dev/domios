use super::*;

pub struct DurableBellNotice {
    pub id: String,
    pub title: String,
    pub body: String,
    pub severity: NotificationSeverity,
    pub level: Option<NotifyLevel>,
    pub actions: Vec<NotificationAction>,
}

impl NotificationStore {
    pub fn add_durable_bell(
        &mut self,
        notice: DurableBellNotice,
    ) -> std::io::Result<Option<Notification>> {
        if self.notification(&notice.id).is_some() {
            return Ok(None);
        }
        if self.max_notifications == 0 {
            return Err(std::io::Error::other("bell has no notification capacity"));
        }
        let notification = Notification {
            id: notice.id,
            title: notice.title,
            body: Some(notice.body),
            severity: notice.severity,
            level: notice.level,
            timestamp: now_ms(),
            read: false,
            actions: Some(notice.actions),
            sticky: Some(true),
            tab_id: None,
            workspace_id: None,
            tab_title: None,
            peer_name: None,
            target_tab_id: None,
            chat_message_id: None,
            answer: None,
            resolved: false,
            surface_offer: None,
            push_failed: None,
            placements: Vec::new(),
            condition: None,
        };
        self.notifications.push(notification.clone());
        let evict = self
            .notifications
            .len()
            .saturating_sub(self.max_notifications);
        if let Some(persistence) = &mut self.persistence {
            if let Err(error) = persistence.sync_checked(&self.notifications[evict..]) {
                self.notifications.pop();
                return Err(error);
            }
        }
        for _ in 0..evict {
            let evicted = self.notifications.remove(0);
            self.forget_record_indexes(&evicted.id);
        }
        Ok(Some(notification))
    }

    pub fn admit_durable_bell_push(&mut self, scope: &str, limit: u32) -> bool {
        self.admit_push_budget(scope.to_owned(), limit, now_ms())
    }
}
