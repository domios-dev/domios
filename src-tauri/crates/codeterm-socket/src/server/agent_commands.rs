//! `plugin:<id>.<verb>` JSON-RPC routing — the inverse seam (plan §1.5 F5 /
//! §1.10). A spawned agent runs `domios plugin <id> <verb> [args…]`; the CLI
//! sends method `plugin:<id>.<verb>` with `{ args, sessionId, body? }`. We resolve the
//! owning plugin in [`AgentCommandRegistry`](codeterm_service_core::agent_commands), enforce its
//! `agent_commands` grant, optionally scope the verb to what the spawning plugin
//! declared at `host.agent.spawn` (C2 MINOR-2), then call its `onAgentCommand`.

use serde_json::Value;

use codeterm_service_core::agent_commands::AgentCommandRegistry;
use codeterm_service_core::service::ServiceState;

use super::JsonRpcError;

/// Reserved prefix that dodges collision with built-in domains (`agent`,
/// `workspace`, …) — F5.
const PLUGIN_PREFIX: &str = "plugin:";

/// Parse `plugin:<id>.<verb>` → `(id, verb)`. The id may not contain `.`; the
/// verb is everything after the first `.`.
fn parse_method(method: &str) -> Option<(String, String)> {
    let rest = method.strip_prefix(PLUGIN_PREFIX)?;
    let (id, verb) = rest.split_once('.')?;
    if id.is_empty() || verb.is_empty() {
        return None;
    }
    Some((id.to_string(), verb.to_string()))
}

fn arg_strings(params: &Option<Value>) -> Vec<String> {
    params
        .as_ref()
        .and_then(|p| p.get("args"))
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

fn session_id(params: &Option<Value>) -> String {
    params
        .as_ref()
        .and_then(|p| p.get("sessionId"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

/// Pure routing core — testable without a live `ServiceContext`. `allowed_verbs`
/// is the spawning worker's declared command set (C2 MINOR-2): `None` ⇒ no
/// scoping (plugin-level grant suffices), `Some(non-empty)` ⇒ the verb must be a
/// member or the call is refused.
pub(crate) fn route(
    registry: &AgentCommandRegistry,
    method: &str,
    params: &Option<Value>,
    allowed_verbs: Option<&[String]>,
) -> Result<Value, JsonRpcError> {
    let (id, verb) = parse_method(method).ok_or_else(|| JsonRpcError {
        code: -32600,
        message: format!("malformed plugin command method: {method} (expected plugin:<id>.<verb>)"),
        data: None,
    })?;

    let handler = registry.get(&id).ok_or_else(|| JsonRpcError {
        code: -32601,
        message: format!("plugin '{id}' has no agent_commands handler"),
        data: None,
    })?;
    if !handler.granted() {
        return Err(JsonRpcError {
            code: -32603,
            message: format!("plugin '{id}' is not granted agent_commands"),
            data: None,
        });
    }
    if let Some(verbs) = allowed_verbs {
        if !verbs.is_empty() && !verbs.iter().any(|v| v == &verb) {
            return Err(JsonRpcError {
                code: -32603,
                message: format!("verb '{verb}' is not in the spawned worker's granted commands"),
                data: None,
            });
        }
    }

    let args = arg_strings(params);
    let sid = session_id(params);
    let body = params
        .as_ref()
        .and_then(|p| p.get("body"))
        .and_then(Value::as_str);
    const MAX_BODY_BYTES: usize = 1024 * 1024;
    if body.is_some_and(|body| body.len() > MAX_BODY_BYTES) {
        return Err(JsonRpcError {
            code: -32602,
            message: "plugin command body exceeds 1048576 bytes".into(),
            data: Some(
                serde_json::json!({"code": "plugin_body_too_large", "max_bytes": MAX_BODY_BYTES}),
            ),
        });
    }
    match handler.on_agent_command(&sid, &verb, &args, body) {
        Ok(result) => Ok(serde_json::json!({
            "result": result.result,
            "catalogue_changed": result.catalogue_changed,
        })),
        Err(e) => Ok(serde_json::json!({ "error": e })),
    }
}

/// Socket-dispatch entry. Resolves the calling worker's command scope, then
/// delegates to [`route`].
pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    svc: &ServiceState,
) -> Result<Value, JsonRpcError> {
    let svc = &**svc;
    let (id, _) = parse_method(method).ok_or_else(|| JsonRpcError {
        code: -32600,
        message: format!("malformed plugin command method: {method} (expected plugin:<id>.<verb>)"),
        data: None,
    })?;

    // confined to the verbs the plugin declared via `host.agent.spawn(commands)`.
    let sid = session_id(params);
    let scope = if sid.is_empty() {
        None
    } else {
        svc.workers.get(&sid).and_then(|w| {
            if w.spawned_by == format!("plugin:{id}") && !w.commands.is_empty() {
                Some(w.commands)
            } else {
                None
            }
        })
    };

    let mut result = route(
        svc.ext::<codeterm_service_core::agent_commands::AgentCommandRegistryHandle>(),
        method,
        params,
        scope.as_deref(),
    )?;
    if result
        .get("catalogue_changed")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        log::debug!(
            "[agent_command] refresh trigger source=command provider={} method={}",
            id,
            method
        );
        codeterm_service_core::agent::model_catalog::invalidate(&id);
        if let Some(provider) = svc.agent_registry.provider_arc(&id) {
            codeterm_service_core::async_runtime::spawn(async move {
                let _ = codeterm_service_core::agent::model_catalog::refresh_models(provider, true)
                    .await;
            });
        } else if svc.chatbackend_registry.get(&id).is_some() {
            let _ = svc.chatbackend_registry.refresh_provider(&id);
        }
    }
    if let Some(object) = result.as_object_mut() {
        object.remove("catalogue_changed");
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
