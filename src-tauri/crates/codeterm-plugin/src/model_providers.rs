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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPreset {
    pub name: String,
    pub model: String,
    pub prompt: String,
    #[serde(default)]
    pub params: PresetParams,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresetParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steps: Option<u32>,
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
    fn read_presets(&self, _config: &Value) -> Result<Vec<ModelPreset>, String> {
        Err("presets unsupported".into())
    }
    fn apply_presets(
        &self,
        _config: &Value,
        _presets: &[ModelPreset],
    ) -> Result<ConfigUpdate, String> {
        Err("presets unsupported".into())
    }
    fn preset_launch_args(&self, _name: &str) -> Result<Vec<String>, String> {
        Err("presets unsupported".into())
    }
}
