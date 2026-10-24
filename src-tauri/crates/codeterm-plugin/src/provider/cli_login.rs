use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CliLoginCapability {
    pub methods: Vec<CliLoginMethod>,
    pub url_pattern: String,
    pub timeout_ms: u64,
    #[serde(default)]
    pub code_input: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CliLoginMethod {
    pub id: String,
    pub label: String,
    pub args: Vec<String>,
}
