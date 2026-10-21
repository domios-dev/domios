use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteOwnershipTeam {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// Authenticated control ownership (controller, origin peer, origin Team),
/// separate from transport identity — set on target Worker and origin alike.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteOwnership {
    pub controller_title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub controller_tab_id: Option<String>,
    #[serde(default)]
    pub peer_id: String,
    #[serde(default)]
    pub peer_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<RemoteOwnershipTeam>,
}
