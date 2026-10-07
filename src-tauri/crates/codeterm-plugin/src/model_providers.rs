use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EndpointKind {
    OpenaiCompatible,
    Anthropic,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelProviderDefinition {
    pub id: String,
    pub kind: EndpointKind,
    pub base_url: String,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_env: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_name: Option<String>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct ConfigUpdate {
    pub config: Value,
    pub changed: bool,
    pub conflicts: Vec<String>,
}

pub trait ModelProviderConfig: Send + Sync {
    fn path(&self) -> Result<PathBuf, String>;
    fn read(&self, config: &Value) -> Result<Vec<ModelProviderDefinition>, String>;
    fn apply(
        &self,
        config: &Value,
        providers: &[ModelProviderDefinition],
    ) -> Result<ConfigUpdate, String>;
    fn key_env_name(&self, id: &str) -> Result<String, String>;
}
