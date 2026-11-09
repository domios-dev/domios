use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A workspace contains multiple tabs and tracks which tab is active.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub tabs: Vec<String>,
    pub active_tab_id: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub cwd_missing: bool,
    /// Directory the daemon created for this workspace; host provenance, never wire input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub provisioned_cwd: Option<String>,
    /// Protected workspaces cannot be closed via API or UI.
    #[serde(default)]
    pub protected: bool,
    /// Creation order for plan-limit locks; 0 until restore backfills a legacy item.
    #[serde(default)]
    pub created_at_ms: u64,
    #[serde(
        default,
        skip_deserializing,
        skip_serializing_if = "std::ops::Not::not"
    )]
    pub locked: bool,
    /// Sibling ordinal among workspaces. Only relative order within the group
    /// matters; the frontend sorts by it. Legacy snapshots default to 0.
    #[serde(default)]
    pub sort_order: i32,
    #[serde(
        default,
        skip_serializing_if = "codeterm_types::plan::task_key::WorkspaceTaskKeys::is_unissued"
    )]
    pub task_keys: codeterm_types::plan::task_key::WorkspaceTaskKeys,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, TS)]
#[serde(rename_all = "snake_case")]
pub enum ContextEpochTransport {
    #[default]
    AgentProvider,
    ChatBackend,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, TS)]
#[serde(rename_all = "snake_case")]
pub enum ContextEpochBoundary {
    #[default]
    ContextReset,
    ProviderHandoff,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ContextEpochArchive {
    pub provider: String,
    pub cwd: String,
    pub session_id: String,
    #[serde(default)]
    pub transport: ContextEpochTransport,
    #[serde(default)]
    pub boundary: ContextEpochBoundary,
}
