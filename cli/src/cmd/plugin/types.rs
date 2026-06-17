use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::args::plugin::{PluginAction, PluginChannelAction};
use crate::data_root;

pub(super) const SYSTEM_CHANNEL_NAME: &str = "codeterm-core";
pub(super) const DEFAULT_PLUGIN_CHANNEL_NAME: &str = "codeterm-plugins";
pub(super) const DEFAULT_SYSTEM_CHANNEL_URL: &str =
    "https://github.com/rollacode/codeterm.git#develop:plugins";
pub(super) const DEFAULT_PLUGIN_CHANNEL_URL: &str =
    "https://github.com/rollacode/codeterm-plugins.git";
pub(super) const SYSTEM_PLUGIN_IDS: [&str; 3] = ["claude", "codex", "opencode"];

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(super) struct ChannelRegistry {
    #[serde(default)]
    pub(super) channels: BTreeMap<String, ChannelRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ChannelRecord {
    pub(super) name: String,
    pub(super) url: String,
    pub(super) path: String,
    pub(super) kind: ChannelKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum ChannelKind {
    Local,
    Git,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct InstallRecord {
    pub(super) id: String,
    pub(super) version: Option<String>,
    pub(super) source: InstallSource,
    pub(super) installed_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) last_change: Option<LastChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum InstallSource {
    Dir { path: String },
    Channel { channel: String, path: String },
    SystemChannel { channel: String, path: String },
}

#[derive(Debug, Deserialize)]
pub(super) struct PluginManifest {
    pub(super) id: String,
    #[serde(default)]
    pub(super) version: Option<String>,
    #[serde(default, rename = "displayName")]
    pub(super) display_name: Option<String>,
    #[serde(default, rename = "minCodeterm")]
    pub(super) min_codeterm: Option<String>,
    /// §1.8: capability can't hot-activate; install/config tell the agent to
    /// reload CodeTerm to activate.
    #[serde(default, rename = "requiresReload")]
    pub(super) requires_reload: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ChannelManifest {
    pub(super) name: String,
    pub(super) plugins: Vec<ChannelPlugin>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ChannelPlugin {
    pub(super) id: String,
    #[serde(default)]
    pub(super) version: Option<String>,
    #[serde(default)]
    pub(super) path: Option<String>,
    #[serde(default)]
    pub(super) source: Option<serde_json::Value>,
    #[serde(default, rename = "minCodeterm")]
    pub(super) min_codeterm: Option<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct InstalledPluginRow {
    pub(super) id: String,
    pub(super) version: Option<String>,
    pub(super) display_name: Option<String>,
    pub(super) path: String,
    pub(super) install_path: String,
    pub(super) install_time: Option<String>,
    pub(super) manifest_hash: Option<String>,
    pub(super) verified: bool,
    pub(super) last_change: Option<LastChange>,
    pub(super) state: String,
    pub(super) source: Option<InstallSource>,
    pub(super) system: bool,
    pub(super) removable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct LastChange {
    pub(super) kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) from_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) to_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) channel: Option<String>,
    pub(super) time: String,
}

#[derive(Debug, Serialize)]
pub(super) struct ChannelDiffRow {
    pub(super) channel: String,
    pub(super) id: String,
    pub(super) local_version: Option<String>,
    pub(super) remote_version: Option<String>,
    pub(super) local_manifest_hash: Option<String>,
    pub(super) remote_manifest_hash: Option<String>,
    pub(super) drift: String,
    pub(super) compatible: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) blocked_reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct ChannelListRow {
    pub(super) name: String,
    pub(super) url: String,
    pub(super) path: String,
    pub(super) kind: ChannelKind,
    pub(super) system: bool,
    pub(super) removable: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct PluginEvent {
    pub(super) ts: String,
    pub(super) kind: String,
    pub(super) actor: String,
    pub(super) ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) plugin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) channel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) error: Option<String>,
}
pub(super) fn is_reserved_system_id(id: &str) -> bool {
    SYSTEM_PLUGIN_IDS.contains(&id)
}
