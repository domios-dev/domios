use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::RwLock;
use serde::Serialize;
use tauri::Emitter;

use crate::api::events::SystemEvent;
use crate::service::ServiceContext;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginActivity {
    pub id: String,
    pub plugin_id: String,
    pub kind: String,
    pub label: String,
    pub percent: Option<f32>,
    pub status: String,
    pub error: Option<String>,
    pub started_at: u64,
    pub updated_at: u64,
}

pub struct PluginActivityRegistry {
    inner: RwLock<HashMap<String, PluginActivity>>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl PluginActivityRegistry {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
        }
    }

    pub fn start(&self, plugin_id: &str, kind: &str, label: &str) -> PluginActivity {
        let now = now_ms();
        let activity = PluginActivity {
            id: uuid::Uuid::new_v4().to_string(),
            plugin_id: plugin_id.to_string(),
            kind: kind.to_string(),
            label: label.to_string(),
            percent: None,
            status: "running".to_string(),
            error: None,
            started_at: now,
            updated_at: now,
        };
        self.inner
            .write()
            .insert(activity.id.clone(), activity.clone());
        activity
    }

    pub fn step(&self, id: &str, label: &str, percent: Option<f32>) -> PluginActivity {
        let mut map = self.inner.write();
        let activity = map
            .get_mut(id)
            .unwrap_or_else(|| panic!("plugin activity not found: {}", id));
        activity.label = label.to_string();
        activity.percent = percent;
        activity.updated_at = now_ms();
        activity.clone()
    }

    pub fn finish(&self, id: &str, error: Option<String>) -> PluginActivity {
        let mut map = self.inner.write();
        let activity = map
            .get_mut(id)
            .unwrap_or_else(|| panic!("plugin activity not found: {}", id));
        activity.status = if error.is_some() {
            "error".to_string()
        } else {
            "done".to_string()
        };
        activity.error = error;
        activity.updated_at = now_ms();
        activity.clone()
    }

    pub fn get(&self, id: &str) -> Option<PluginActivity> {
        self.inner.read().get(id).cloned()
    }
}

impl Default for PluginActivityRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub fn emit_plugin_activity(svc: &ServiceContext, activity: &PluginActivity) {
    let _ = svc.app_handle.emit("plugin-activity", activity);
    svc.events
        .publish(SystemEvent::PluginActivity(activity.clone()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_tracks_start_step_and_finish_transitions() {
        let registry = PluginActivityRegistry::new();

        let started = registry.start("transcriber", "bootstrap", "Preparing");

        assert_eq!(started.plugin_id, "transcriber");
        assert_eq!(started.kind, "bootstrap");
        assert_eq!(started.label, "Preparing");
        assert_eq!(started.percent, None);
        assert_eq!(started.status, "running");
        assert_eq!(started.error, None);
        assert_eq!(started.started_at, started.updated_at);
        assert_eq!(registry.get(&started.id), Some(started.clone()));

        let stepped = registry.step(&started.id, "Downloading model", Some(42.5));

        assert_eq!(stepped.id, started.id);
        assert_eq!(stepped.label, "Downloading model");
        assert_eq!(stepped.percent, Some(42.5));
        assert_eq!(stepped.status, "running");
        assert_eq!(stepped.started_at, started.started_at);
        assert!(stepped.updated_at >= started.updated_at);
        assert_eq!(registry.get(&started.id), Some(stepped.clone()));

        let finished = registry.finish(&started.id, None);

        assert_eq!(finished.id, started.id);
        assert_eq!(finished.status, "done");
        assert_eq!(finished.error, None);
        assert_eq!(finished.label, "Downloading model");
        assert_eq!(finished.percent, Some(42.5));
        assert_eq!(finished.started_at, started.started_at);
        assert!(finished.updated_at >= stepped.updated_at);
        assert_eq!(registry.get(&started.id), Some(finished));
    }
}
