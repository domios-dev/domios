use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, TS)]
pub struct AtomicSpawn<S> {
    pub snapshot: S,
    pub tab_id: String,
    pub workspace_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub launch_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub warning: Option<String>,
}
