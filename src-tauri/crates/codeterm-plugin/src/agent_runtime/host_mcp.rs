use super::{AgentRuntimeCapability, RuntimeContext, RuntimeFailure, RuntimeFailureCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeHostMcpDeclaration {
    pub param: String,
    pub methods: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeHostMcp {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
}

impl RuntimeHostMcp {
    pub fn descriptor(&self) -> Value {
        json!({"name":self.name,"command":self.command,"args":self.args,"env":[]})
    }
}

pub(super) fn validate(
    cap: &AgentRuntimeCapability,
    ctx: &RuntimeContext,
    method: &str,
    params: &Value,
) -> Result<(), RuntimeFailure> {
    let Some(declaration) = &cap.host_mcp else {
        return Ok(());
    };
    if !declaration
        .methods
        .iter()
        .any(|declared| declared == method)
    {
        return Ok(());
    }
    let servers = params.get(&declaration.param);
    if servers == Some(&json!([]))
        || ctx
            .host_mcp
            .as_ref()
            .is_some_and(|descriptor| servers == Some(&json!([descriptor.descriptor()])))
    {
        return Ok(());
    }
    Err(RuntimeFailure::new(
        RuntimeFailureCode::BindingMismatch,
        "host MCP servers differ from tab authority",
    ))
}

#[cfg(test)]
mod tests;
