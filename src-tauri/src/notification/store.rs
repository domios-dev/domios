use serde::Serialize;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct Notification {
    pub id: String,
    pub tab_id: Option<String>,
    pub workspace_id: Option<String>,
    pub title: String,
    pub body: Option<String>,
    pub level: NotificationLevel,
    pub timestamp: u64,
    pub read: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actions: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sticky: Option<bool>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum NotificationLevel {
    Info,
    Warning,
    Error,
}

#[derive(Debug, PartialEq, Eq)]
pub enum DedupAdmission {
    Added(String),
    Duplicate(String),
}

pub struct NotificationStore {
    notifications: Vec<Notification>,
    provider_updates: HashMap<String, String>,
    max_notifications: usize,
}

impl NotificationStore {
    pub fn new(max: usize) -> Self {
        Self {
            notifications: Vec::new(),
            provider_updates: HashMap::new(),
            max_notifications: max,
        }
    }

    /// Add a notification. Returns the notification ID.
    pub fn add(
        &mut self,
        title: String,
        body: Option<String>,
        level: NotificationLevel,
        tab_id: Option<String>,
        workspace_id: Option<String>,
    ) -> String {
        self.add_ext(title, body, level, tab_id, workspace_id, None, None)
    }

    pub fn add_ext(
        &mut self,
        title: String,
        body: Option<String>,
        level: NotificationLevel,
        tab_id: Option<String>,
        workspace_id: Option<String>,
        actions: Option<serde_json::Value>,
        sticky: Option<bool>,
    ) -> String {
        let id = Uuid::new_v4().to_string();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let notification = Notification {
            id: id.clone(),
            tab_id,
            workspace_id,
            title,
            body,
            level,
            timestamp,
            read: false,
            actions,
            sticky,
        };

        self.notifications.push(notification);

        while self.notifications.len() > self.max_notifications {
            self.notifications.remove(0);
        }

        id
    }

    pub fn add_provider_update(
        &mut self,
        prompt_id: String,
        title: String,
        body: String,
        actions: serde_json::Value,
        tab_id: String,
        workspace_id: Option<String>,
    ) -> DedupAdmission {
        if let Some(id) = self.provider_updates.get(&prompt_id) {
            return DedupAdmission::Duplicate(id.clone());
        }
        let id = self.add_ext(
            title,
            Some(body),
            NotificationLevel::Info,
            Some(tab_id),
            workspace_id,
            Some(actions),
            Some(true),
        );
        self.provider_updates.insert(prompt_id, id.clone());
        DedupAdmission::Added(id)
    }

    pub fn resolve_provider_update(&mut self, prompt_id: &str) -> Option<String> {
        let id = self.provider_updates.remove(prompt_id)?;
        self.clear(Some(&id));
        Some(id)
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
                self.provider_updates.retain(|_, value| value != id);
            }
            None => {
                self.notifications.clear();
                self.provider_updates.clear();
            }
        }
    }

    pub fn unread_count(&self) -> usize {
        self.notifications.iter().filter(|n| !n.read).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_notification() {
        let mut store = NotificationStore::new(100);
        let id = store.add("Test".into(), None, NotificationLevel::Info, None, None);
        assert!(!id.is_empty());
        assert_eq!(store.list(false).len(), 1);
    }

    #[test]
    fn test_notification_defaults_unread() {
        let mut store = NotificationStore::new(100);
        store.add("Test".into(), None, NotificationLevel::Info, None, None);
        assert_eq!(store.unread_count(), 1);
        assert_eq!(store.list(true).len(), 1);
    }

    #[test]
    fn test_mark_read() {
        let mut store = NotificationStore::new(100);
        let id = store.add("Test".into(), None, NotificationLevel::Info, None, None);
        assert!(store.mark_read(&id));
        assert_eq!(store.unread_count(), 0);
        assert_eq!(store.list(true).len(), 0);
        assert_eq!(store.list(false).len(), 1);
    }

    #[test]
    fn test_mark_read_nonexistent() {
        let mut store = NotificationStore::new(100);
        assert!(!store.mark_read("nonexistent"));
    }

    #[test]
    fn test_clear_specific() {
        let mut store = NotificationStore::new(100);
        let id1 = store.add("A".into(), None, NotificationLevel::Info, None, None);
        let _id2 = store.add("B".into(), None, NotificationLevel::Info, None, None);
        store.clear(Some(&id1));
        assert_eq!(store.list(false).len(), 1);
        assert_eq!(store.list(false)[0].title, "B");
    }

    #[test]
    fn test_clear_all() {
        let mut store = NotificationStore::new(100);
        store.add("A".into(), None, NotificationLevel::Info, None, None);
        store.add("B".into(), None, NotificationLevel::Info, None, None);
        store.clear(None);
        assert_eq!(store.list(false).len(), 0);
    }

    #[test]
    fn test_eviction() {
        let mut store = NotificationStore::new(3);
        store.add("A".into(), None, NotificationLevel::Info, None, None);
        store.add("B".into(), None, NotificationLevel::Info, None, None);
        store.add("C".into(), None, NotificationLevel::Info, None, None);
        store.add("D".into(), None, NotificationLevel::Info, None, None);
        assert_eq!(store.list(false).len(), 3);
        let titles: Vec<&str> = store.list(false).iter().map(|n| n.title.as_str()).collect();
        assert_eq!(titles, vec!["B", "C", "D"]);
    }

    #[test]
    fn test_notification_levels() {
        let mut store = NotificationStore::new(100);
        store.add("Info".into(), None, NotificationLevel::Info, None, None);
        store.add("Warn".into(), None, NotificationLevel::Warning, None, None);
        store.add("Err".into(), None, NotificationLevel::Error, None, None);
        let levels: Vec<&NotificationLevel> = store.list(false).iter().map(|n| &n.level).collect();
        assert_eq!(
            levels,
            vec![
                &NotificationLevel::Info,
                &NotificationLevel::Warning,
                &NotificationLevel::Error
            ]
        );
    }

    #[test]
    fn test_notification_with_body_and_tab() {
        let mut store = NotificationStore::new(100);
        store.add(
            "Title".into(),
            Some("Body text".into()),
            NotificationLevel::Info,
            Some("tab-1".into()),
            Some("ws-1".into()),
        );
        let n = &store.list(false)[0];
        assert_eq!(n.title, "Title");
        assert_eq!(n.body.as_deref(), Some("Body text"));
        assert_eq!(n.tab_id.as_deref(), Some("tab-1"));
        assert_eq!(n.workspace_id.as_deref(), Some("ws-1"));
    }

    #[test]
    fn provider_update_notification_is_actionable_deduplicated_and_resolvable() {
        let mut store = NotificationStore::new(100);
        let actions = serde_json::json!([{"label":"Update","type":"invoke"}]);
        let first = store.add_provider_update(
            "prompt-1".into(),
            "Update available".into(),
            "Install separately".into(),
            actions.clone(),
            "tab-1".into(),
            Some("workspace-1".into()),
        );
        let second = store.add_provider_update(
            "prompt-1".into(),
            "Duplicate".into(),
            "Duplicate".into(),
            actions.clone(),
            "tab-1".into(),
            Some("workspace-1".into()),
        );
        let id = match first {
            DedupAdmission::Added(id) => id,
            _ => panic!("first provider update must be added"),
        };
        assert_eq!(second, DedupAdmission::Duplicate(id.clone()));
        assert_eq!(store.list(false).len(), 1);
        assert_eq!(store.list(false)[0].actions, Some(actions));
        assert_eq!(store.resolve_provider_update("prompt-1"), Some(id));
        assert!(store.list(false).is_empty());
    }
}
