use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AgentSpawnSpec {
    pub provider: String,
    #[serde(default)]
    pub account_id: Option<String>,
    #[serde(skip)]
    pub remote_task_bound: bool,
    #[serde(default)]
    pub planned_model: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default, skip_deserializing)]
    pub(crate) planned_reasoning_effort: Option<String>,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
    #[serde(default)]
    pub task: Option<String>,
    #[serde(default)]
    pub agent_name: Option<String>,
    #[serde(default)]
    pub extra_args: Vec<String>,
    #[serde(default, skip_deserializing)]
    pub provisioned_cwd: Option<String>,
    #[serde(default, skip_deserializing)]
    pub existing_tab_id: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub role_profile: Option<String>,
    #[serde(default)]
    pub can_spawn: Option<bool>,
    #[serde(default)]
    pub report_target_tab_id: Option<String>,
    #[serde(default)]
    pub worker_id: Option<String>,
    #[serde(default)]
    pub spawned_by: Option<String>,
    #[serde(default)]
    pub extra_env: HashMap<String, String>,
    #[serde(default)]
    pub task_preamble: Option<String>,
    #[serde(default)]
    pub spawned_by_tab_id: Option<String>,
    #[serde(default)]
    pub resume_session_id: Option<String>,
    #[serde(default, skip_deserializing)]
    pub(crate) fork_session: bool,
    /// Highlight the new tab in every client; it never moves a client or the last selection.
    #[serde(default)]
    pub activate: Option<bool>,
    #[serde(skip)]
    pub(crate) launch_purpose: crate::service::unattended_launch::LaunchPurpose,
}
