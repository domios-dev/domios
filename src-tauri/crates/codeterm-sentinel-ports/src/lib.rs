use std::sync::Arc;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BlueprintRecord {
    pub id: String,
    pub workspace_id: String,
    pub team_id: Option<String>,
    pub scenario_root: bool,
    pub definition: serde_json::Value,
}

pub trait BlueprintStore: Send + Sync {
    fn records(&self) -> Vec<BlueprintRecord>;
    fn scope_of(&self, id: &str) -> Option<(String, Option<String>)>;
    fn insert_restored(&self, record: &BlueprintRecord) -> Result<(), String>;
    fn ensure_external_commit_ready(&self) -> Result<(), String>;
}

pub trait SentinelWake: Send + Sync {
    fn notify(&self);
    fn notify_event_subscription_changed(&self, pipeline_id: &str);
    fn terminal_output(&self, tab_id: &str, generation: Option<u64>, bytes: &[u8]);
}

pub trait SentinelCleanup: Send + Sync {
    fn cancel_tab_owned_runs(&self, tab_id: &str) -> usize;
    fn remove_worker_watchers(&self, tab_id: &str) -> Result<(), String>;
    fn remove_team(&self, team_id: &str) -> Result<(), String>;
    fn remove_workspace(&self, workspace_id: &str) -> Result<(), String>;
}

#[derive(Clone)]
pub struct SentinelPorts {
    pub store: Arc<dyn BlueprintStore>,
    pub wake: Arc<dyn SentinelWake>,
    pub cleanup: Arc<dyn SentinelCleanup>,
}
