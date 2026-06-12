use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResizeOwner {
    pub pane_id: String,
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
        pane_id: String,
        pty_id: String,
        client_id: String,
        cols: u16,
        rows: u16,
    ) -> ResizeOwner {
        let owner = ResizeOwner {
            pane_id,
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
