use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionModelTranscriptBoundary {
    #[serde(default)]
    pub last_entry_uuid: Option<String>,
    #[serde(default)]
    pub last_entry_timestamp: Option<String>,
    #[serde(default)]
    pub last_entry_type: Option<String>,
    #[serde(default)]
    pub boundary_found: bool,
}
