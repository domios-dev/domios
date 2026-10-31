use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentRuntimeProtocol {
    StdioJsonrpc,
    #[serde(rename = "http-sse")]
    HttpSse,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentRuntimeCommand {
    pub binary: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentRuntimeCapability {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restricted_cli: Option<RestrictedCliCapability>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runtime_modes: Vec<RuntimeModeDescriptor>,
    #[serde(default)]
    pub service_tiers: Vec<super::RuntimeServiceTier>,
    #[serde(default)]
    pub background_processes: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goals: Option<RuntimeGoalCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_history: Option<RuntimeThreadHistoryCapability>,
    #[serde(default)]
    pub stored_history: bool,
    #[serde(default)]
    pub usage: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset_credits_min_version: Option<String>,
    pub runtime: AgentRuntimeProtocol,
    pub command: AgentRuntimeCommand,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inherited_environment_exclusions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub login: Option<RuntimeLoginCapability>,
    pub request_methods: BTreeMap<String, Vec<String>>,
    pub notification_methods: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeLoginCapability {
    pub methods: Vec<RuntimeLoginMethod>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_command: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeLoginMethod {
    Browser,
    Device,
    ApiKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeGoalCapability {
    pub actions: Vec<RuntimeGoalAction>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeGoalAction {
    Read,
    Cancel,
    Pause,
    Resume,
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeThreadHistoryCapability {
    pub actions: Vec<RuntimeThreadHistoryAction>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeThreadHistoryAction {
    Fork,
    Revert,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeModeDescriptor {
    pub id: String,
    pub display_name: String,
}
