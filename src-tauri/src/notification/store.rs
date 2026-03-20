use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct Notification {
    pub id: String,
    pub pane_id: Option<String>,
    pub workspace_id: Option<String>,
    pub title: String,
    pub body: Option<String>,
    pub level: NotificationLevel,
    pub timestamp: u64,
    pub read: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum NotificationLevel {
    Info,
    Warning,
    Error,
}

pub struct NotificationStore {
    notifications: Vec<Notification>,
    max_notifications: usize,
}

impl NotificationStore {
    pub fn new(max: usize) -> Self {
        Self {
            notifications: Vec::new(),
            max_notifications: max,
        }
    }

    /// Add a notification. Returns the notification ID.
    pub fn add(
        &mut self,
        title: String,
        body: Option<String>,
        level: NotificationLevel,
        pane_id: Option<String>,
        workspace_id: Option<String>,
    ) -> String {
        let id = Uuid::new_v4().to_string();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let notification = Notification {
            id: id.clone(),
            pane_id,
            workspace_id,
            title,
            body,
            level,
            timestamp,
            read: false,
        };

        self.notifications.push(notification);

        // Evict oldest if over capacity
        while self.notifications.len() > self.max_notifications {
            self.notifications.remove(0);
        }

        id
    }

    pub fn list(&self, unread_only: bool) -> Vec<&Notification> {
        if unread_only {
            self.notifications.iter().filter(|n| !n.read).collect()
        } else {
            self.notifications.iter().collect()
        }
    }

    pub fn mark_read(&mut self, id: &str) -> bool {
        if let Some(n) = self.notifications.iter_mut().find(|n| n.id == id) {
            n.read = true;
            true
        } else {
            false
        }
    }

    pub fn clear(&mut self, id: Option<&str>) {
        match id {
            Some(id) => {
                self.notifications.retain(|n| n.id != id);
            }
            None => {
                self.notifications.clear();
            }
        }
    }

    pub fn unread_count(&self) -> usize {
        self.notifications.iter().filter(|n| !n.read).count()
    }
}
