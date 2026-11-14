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

/// Why a channel left a listed team out; clients word it from this code, never from `reason`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    MissingPackage,
    InvalidPackage,
    RequiresNewerDomios,
    ReservedName,
    IndexMismatch,
    InvalidPath,
    InvalidIndexEntry,
    DuplicateName,
    HostPolicy,
    /// Registries written before codes existed read as this until their next refresh.
    #[default]
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SkippedTeam {
    pub name: String,
    #[serde(default)]
    pub code: SkipReason,
    /// The host's raw detail, possibly OS-localized; shown only on demand.
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_domios: Option<String>,
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
