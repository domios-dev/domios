use super::ExecutionIntent;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionSandboxPolicy {
    pub intent: ExecutionIntent,
    pub cwd: String,
    pub launch_args: Vec<String>,
    pub runtime_mode: Option<String>,
    pub writable_roots: Vec<String>,
    pub environment: std::collections::BTreeMap<String, String>,
}
