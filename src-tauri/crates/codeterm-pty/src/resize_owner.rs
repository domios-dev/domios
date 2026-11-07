use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResizeOwner {
    pub tab_id: String,
    pub pty_id: String,
    pub client_id: String,
    pub cols: u16,
    pub rows: u16,
    pub updated_at: u64,
}

#[derive(Default)]
pub struct ResizeOwnerRegistry {
    owners_by_pty: HashMap<String, ResizeOwner>,
}

impl ResizeOwnerRegistry {
    pub fn claim(
        &mut self,
        tab_id: String,
        pty_id: String,
        client_id: String,
        cols: u16,
        rows: u16,
        force: bool,
    ) -> ResizeOwner {
        if let Some(owner) = self.owners_by_pty.get(&pty_id) {
            if !force && owner.client_id != client_id {
                return owner.clone();
            }
        }
        let owner = ResizeOwner {
            tab_id,
            pty_id: pty_id.clone(),
            client_id,
            cols,
            rows,
            updated_at: now_millis(),
        };
        self.owners_by_pty.insert(pty_id, owner.clone());
        owner
    }

    pub fn get(&self, pty_id: &str) -> Option<ResizeOwner> {
        self.owners_by_pty.get(pty_id).cloned()
    }

    /// Records a size the owner applied so followers can render it; a
    /// non-owner's or an unowned PTY's resize leaves the registry untouched.
    pub fn record_size(
        &mut self,
        pty_id: &str,
        client_id: &str,
        cols: u16,
        rows: u16,
    ) -> Option<ResizeOwner> {
        let owner = self.owners_by_pty.get_mut(pty_id)?;
        if owner.client_id != client_id {
            return None;
        }
        owner.cols = cols;
        owner.rows = rows;
        owner.updated_at = now_millis();
        Some(owner.clone())
    }

    pub fn is_owner(&self, pty_id: &str, client_id: &str) -> bool {
        self.owners_by_pty
            .get(pty_id)
            .map(|owner| owner.client_id == client_id)
            .unwrap_or(true)
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

pub type ResizeOwnerState = Arc<Mutex<ResizeOwnerRegistry>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_cannot_steal_existing_resize_owner_without_force() {
        let mut registry = ResizeOwnerRegistry::default();
        let first = registry.claim(
            "tab".into(),
            "pty".into(),
            "tauri:desktop".into(),
            160,
            48,
            false,
        );
        let second = registry.claim(
            "tab".into(),
            "pty".into(),
            "web:phone".into(),
            80,
            24,
            false,
        );

        assert_eq!(second.client_id, first.client_id);
        assert_eq!(second.cols, 160);
        assert!(registry.is_owner("pty", "tauri:desktop"));
        assert!(!registry.is_owner("pty", "web:phone"));
    }

    #[test]
    fn same_client_can_refresh_existing_resize_owner_without_force() {
        let mut registry = ResizeOwnerRegistry::default();
        registry.claim(
            "tab".into(),
            "pty".into(),
            "web:phone".into(),
            80,
            24,
            false,
        );
        let owner = registry.claim(
            "tab".into(),
            "pty".into(),
            "web:phone".into(),
            160,
            48,
            false,
        );

        assert_eq!(owner.client_id, "web:phone");
        assert_eq!(owner.cols, 160);
        assert!(registry.is_owner("pty", "web:phone"));
    }

    #[test]
    fn owner_resize_updates_the_recorded_size_for_followers() {
        let mut registry = ResizeOwnerRegistry::default();
        registry.claim(
            "tab".into(),
            "pty".into(),
            "tauri:desktop".into(),
            160,
            48,
            false,
        );

        let owner = registry
            .record_size("pty", "tauri:desktop", 138, 46)
            .expect("owner resize is recorded");

        assert_eq!(owner.client_id, "tauri:desktop");
        assert_eq!((owner.cols, owner.rows), (138, 46));
        let stored = registry.get("pty").expect("owner stays registered");
        assert_eq!((stored.cols, stored.rows), (138, 46));
    }

    #[test]
    fn non_owner_and_unowned_resizes_leave_the_registry_untouched() {
        let mut registry = ResizeOwnerRegistry::default();
        assert!(registry.record_size("pty", "web:phone", 80, 24).is_none());
        assert!(registry.get("pty").is_none());

        registry.claim(
            "tab".into(),
            "pty".into(),
            "tauri:desktop".into(),
            160,
            48,
            false,
        );
        assert!(registry.record_size("pty", "web:phone", 80, 24).is_none());
        let stored = registry.get("pty").expect("owner stays registered");
        assert_eq!(stored.client_id, "tauri:desktop");
        assert_eq!((stored.cols, stored.rows), (160, 48));
    }

    #[test]
    fn explicit_web_takeover_can_reclaim_from_tauri_resize_owner() {
        let mut registry = ResizeOwnerRegistry::default();
        registry.claim(
            "tab".into(),
            "pty".into(),
            "tauri:desktop".into(),
            160,
            48,
            false,
        );
        let owner = registry.claim("tab".into(), "pty".into(), "web:phone".into(), 80, 24, true);

        assert_eq!(owner.client_id, "web:phone");
        assert_eq!(owner.cols, 80);
        assert!(registry.is_owner("pty", "web:phone"));
    }
}
