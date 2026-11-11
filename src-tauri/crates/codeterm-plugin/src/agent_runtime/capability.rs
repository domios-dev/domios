use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "kebab-case")]
pub enum AgentRuntimeProtocol {
    StdioJsonrpc,
    #[serde(rename = "http-sse")]
    HttpSse,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "lowercase")]
pub enum AgentRuntimeEnvelope {
    Jsonrpc2,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
pub struct AgentRuntimeCommand {
    pub binary: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeLaunchProfile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub icon: Option<String>,
    #[serde(skip_deserializing, default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub icon_svg: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub registry_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub icon_url: Option<String>,
    pub binary: String,
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// The agent process starts in a host-owned directory under the profile's data;
    /// its sessions still open in the tab's working directory.
    #[serde(default, skip_serializing_if = "is_false")]
    pub host_working_directory: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub install: Option<RuntimeProfileInstall>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub secret_env: BTreeMap<String, String>,
    /// Optional credentials: each variable is injected only when its secret is stored,
    /// and the listed declared sign-in methods accept an API key into that secret.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub auth_env: BTreeMap<String, RuntimeProfileAuthEnv>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub owner_configured: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub detection: Option<RuntimeProfileDetection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub client_capabilities: Option<RuntimeProfileClientCapabilities>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub limits_source: Option<super::RuntimeProfileLimitsSource>,
    /// The agent's own session mode that approves every action, applied to full-access launches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub full_access_mode: Option<String>,
    /// How far the agent's own permission questions reach; absent makes no claim about them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub ask_scope: Option<RuntimeAskScope>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeAskScope {
    /// The agent confines its own work to the workspace and asks only to go beyond it.
    OutsideWorkspace,
}

impl RuntimeLaunchProfile {
    pub fn valid_session_mode(mode: &str) -> bool {
        (1..=128).contains(&mode.chars().count()) && !mode.chars().any(char::is_control)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeProfileClientCapabilities {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub fs: Option<RuntimeProfileFileCapabilities>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub terminal: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeProfileFileCapabilities {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub read_text_file: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub write_text_file: Option<bool>,
}

impl RuntimeProfileClientCapabilities {
    pub fn restrict(&self, bridges: &mut RuntimeClientBridges) {
        if let Some(fs) = &self.fs {
            bridges.fs_read &= fs.read_text_file.unwrap_or(true);
            bridges.fs_write &= fs.write_text_file.unwrap_or(true);
        }
        bridges.terminal &= self.terminal.unwrap_or(true);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeProfileAuthEnv {
    pub secret: String,
    pub methods: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeProfileInstall {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub distribution: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub executable: Option<String>,
    pub package: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub registry: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeProfileDetection {
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentRuntimeCapability {
    #[serde(default, skip_serializing_if = "is_false")]
    pub local_image_paths: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub host_mcp: Option<super::RuntimeHostMcpDeclaration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub envelope: Option<AgentRuntimeEnvelope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub interrupt: Option<RuntimeInterruptCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub client_bridges: Option<RuntimeClientBridges>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub launch_profiles: Option<BTreeMap<String, RuntimeLaunchProfile>>,
    /// Declarations a profile installed from this registry id inherits unless it declares its own.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub registry_defaults: BTreeMap<String, super::RuntimeRegistryDefaults>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub restricted_cli: Option<RestrictedCliCapability>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runtime_modes: Vec<RuntimeModeDescriptor>,
    /// Agent permission requests carry declared option kinds, so the host answers them from the
    /// launch policy and a full-access launch needs no command-line flag.
    #[serde(default, skip_serializing_if = "is_false")]
    pub host_approvals: bool,
    #[serde(default)]
    pub service_tiers: Vec<super::RuntimeServiceTier>,
    #[serde(default)]
    pub background_processes: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub goals: Option<RuntimeGoalCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub thread_history: Option<RuntimeThreadHistoryCapability>,
    #[serde(default)]
    pub stored_history: bool,
    #[serde(default)]
    pub usage: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reset_credits_min_version: Option<String>,
    pub runtime: AgentRuntimeProtocol,
    pub command: AgentRuntimeCommand,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inherited_environment_exclusions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub login: Option<RuntimeLoginCapability>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub turn_scoped_requests: Vec<String>,
    pub request_methods: BTreeMap<String, Vec<String>>,
    pub notification_methods: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum RestrictedCliCapability {
    ExecpolicyServiceOperations {
        #[serde(
            rename = "metadataDirectory",
            deserialize_with = "deserialize_metadata_directory"
        )]
        metadata_directory: String,
    },
}

impl RestrictedCliCapability {
    pub fn metadata_directory(&self) -> &str {
        match self {
            Self::ExecpolicyServiceOperations { metadata_directory } => metadata_directory,
        }
    }

    pub fn valid_metadata_directory(value: &str) -> bool {
        (2..=64).contains(&value.len())
            && value.starts_with('.')
            && !value.eq_ignore_ascii_case(".git")
            && value[1..]
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
    }
}

fn deserialize_metadata_directory<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<String, D::Error> {
    let value = String::deserialize(deserializer)?;
    if RestrictedCliCapability::valid_metadata_directory(&value) {
        Ok(value)
    } else {
        Err(serde::de::Error::custom(
            "restricted CLI metadata must be a direct protected directory component",
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeLoginCapability {
    pub methods: Vec<RuntimeLoginMethod>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub terminal_command: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeLoginMethod {
    Browser,
    Device,
    ApiKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeGoalCapability {
    pub actions: Vec<RuntimeGoalAction>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeGoalAction {
    Read,
    Cancel,
    Pause,
    Resume,
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeThreadHistoryCapability {
    pub actions: Vec<RuntimeThreadHistoryAction>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeThreadHistoryAction {
    Fork,
    Revert,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeModeDescriptor {
    pub id: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeClientBridges {
    pub fs_read: bool,
    pub fs_write: bool,
    pub terminal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeInterruptCapability {
    pub before_first_turn: bool,
}

fn is_false(value: &bool) -> bool {
    !value
}
