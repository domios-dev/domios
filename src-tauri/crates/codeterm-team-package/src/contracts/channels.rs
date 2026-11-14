use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum TeamChannelKind {
    Local,
    Git,
    Archive,
}

#[derive(Debug, Deserialize, Serialize, TS)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum TeamChannelAction {
    Add { url: String, name: Option<String> },
    List,
    Refresh { name: Option<String> },
    Remove { name: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SkippedTeam {
    pub name: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TeamChannelRow {
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub url: String,
    pub path: String,
    pub kind: TeamChannelKind,
    pub official: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_refresh: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped_entries: Vec<SkippedTeam>,
}
