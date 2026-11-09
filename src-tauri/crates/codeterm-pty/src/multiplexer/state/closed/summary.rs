use crate::multiplexer::types::{ClosedTab, TabBacking};
use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, TS)]
pub struct ClosedTabSummary {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[ts(optional)]
    pub agent_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[ts(optional)]
    pub agent_type: Option<String>,
    #[serde(default)]
    pub backing: TabBacking,
    #[serde(default)]
    pub chat_view: bool,
    pub closed_at: u64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[ts(optional)]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[ts(optional)]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[ts(optional)]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[ts(optional)]
    pub parent_tab_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[ts(optional)]
    pub role_profile: Option<String>,
    pub tab_id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[ts(optional)]
    pub worker_id: Option<String>,
    #[serde(default)]
    pub workspace_id: String,
    #[serde(default)]
    pub workspace_name: String,
}

impl From<ClosedTab> for ClosedTabSummary {
    fn from(tab: ClosedTab) -> Self {
        Self {
            agent_session_id: tab.agent_session_id,
            agent_type: tab.agent_type,
            backing: tab.backing,
            chat_view: tab.chat_view,
            closed_at: tab.closed_at,
            cwd: tab.cwd,
            description: tab.description,
            model: tab.model,
            parent_tab_id: tab.parent_tab_id,
            role_profile: tab.role_profile,
            tab_id: tab.tab_id,
            title: tab.title,
            worker_id: tab.worker_id,
            workspace_id: tab.workspace_id,
            workspace_name: tab.workspace_name,
        }
    }
}
