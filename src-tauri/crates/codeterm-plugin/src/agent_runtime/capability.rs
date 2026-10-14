use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentRuntimeProtocol {
    StdioJsonrpc,
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
