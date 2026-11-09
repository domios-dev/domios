use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize, ts_rs::TS)]
#[ts(rename = "PluginSnapshotCapabilities")]
pub struct Capabilities {
    pub agent_provider: bool,
    pub secret_backend: bool,
    pub transcriber: bool,
    pub glance_view: bool,
    pub status_bubble: bool,
    pub view: bool,
    pub chat_backend: bool,
    pub chat_preprocessor: bool,
    pub graph_nodes: bool,
    pub local_recording_trigger: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, ts_rs::TS)]
pub struct SensitivePermission {
    pub key: String,
    pub label: String,
    pub allowed: bool,
    pub decided: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, ts_rs::TS)]
pub struct PluginLoadError {
    pub id: String,
    pub path: std::path::PathBuf,
    pub message: String,
    pub at_ms: u64,
    pub kept_previous: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, ts_rs::TS)]
pub struct PluginUpdate {
    pub available: bool,
    pub availability: crate::AvailabilityItem,
    pub reason: String,
    pub channel: Option<String>,
    pub remote_version: Option<String>,
    pub basis: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, ts_rs::TS)]
#[serde(default)]
#[ts(rename = "PluginSnapshotMetadata")]
pub struct Metadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub default_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub capabilities: Option<Capabilities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub sensitive_permissions: Option<Vec<SensitivePermission>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub granted_permissions: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub network_requested: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub declared_network_allow: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub network_allow: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub effective_network_allow: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub settings_schema: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub requires_reload: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub icon_svg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub icon_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub pitch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub config_help: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub homepage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub readme_markdown: Option<String>,
}

pub fn present_nullable<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

#[derive(Clone, Debug, Serialize, Deserialize, ts_rs::TS)]
#[ts(rename = "MarketplaceRow")]
pub struct MarketplaceRow {
    #[serde(flatten)]
    pub diff: crate::ChannelDiffRow,
    pub installed: bool,
    #[serde(flatten)]
    pub metadata: Metadata,
}
#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[ts(rename = "PluginSnapshot")]
pub struct Snapshot {
    pub plugins: Vec<crate::InstalledPluginRow>,
    pub channels: Vec<crate::ChannelListRow>,
    pub diff: Vec<crate::ChannelDiffRow>,
    pub marketplace: Vec<MarketplaceRow>,
    pub audit: Vec<Value>,
    #[serde(rename = "lastOutput")]
    pub last_output: Option<Value>,
}

#[cfg(test)]
mod tests;
