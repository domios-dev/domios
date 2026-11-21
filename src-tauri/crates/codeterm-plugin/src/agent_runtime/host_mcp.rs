use super::{AgentRuntimeCapability, RuntimeContext, RuntimeFailure, RuntimeFailureCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeHostMcpDeclaration {
    pub param: String,
    pub methods: Vec<String>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeHostMcpHttp {
    pub url: String,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub headers: std::collections::BTreeMap<String, String>,
}

impl std::fmt::Debug for RuntimeHostMcpHttp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeHostMcpHttp")
            .field("url", &self.url)
            .field("headers", &"[redacted]")
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeHostMcp {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub env: std::collections::BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub http: Option<RuntimeHostMcpHttp>,
}

impl std::fmt::Debug for RuntimeHostMcp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeHostMcp")
            .field("name", &self.name)
            .field("command", &self.command)
            .field("args", &self.args)
            .field("env", &"[redacted]")
            .field("http", &self.http)
            .finish()
    }
}

impl RuntimeHostMcp {
    pub fn descriptor(&self) -> Value {
        let env: Vec<Value> = self
            .env
            .iter()
            .map(|(name, value)| json!({"name":name,"value":value}))
            .collect();
        json!({"name":self.name,"command":self.command,"args":self.args,"env":env})
    }

    pub fn http_descriptor(&self) -> Option<Value> {
        let http = self.http.as_ref()?;
        let headers: Vec<Value> = http
            .headers
            .iter()
            .map(|(name, value)| json!({"name":name,"value":value}))
            .collect();
        Some(json!({"type":"http","name":self.name,"url":http.url,"headers":headers}))
    }

    pub fn allowed_descriptors(&self) -> Vec<Value> {
        let mut descriptors = vec![self.descriptor()];
        if let Some(http) = self.http_descriptor() {
            descriptors.push(http);
        }
        descriptors
    }
}

pub(super) fn validate(
    cap: &AgentRuntimeCapability,
    ctx: &RuntimeContext,
    _method: &str,
    params: &Value,
) -> Result<(), RuntimeFailure> {
    let Some(declaration) = &cap.host_mcp else {
        return Ok(());
    };
    let servers = params.get(&declaration.param);
    if servers.is_none() {
        return Ok(());
    }
    if servers == Some(&json!([]))
        || ctx.host_mcp.as_ref().is_some_and(|descriptor| {
            descriptor
                .allowed_descriptors()
                .into_iter()
                .any(|allowed| servers == Some(&json!([allowed])))
        })
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
