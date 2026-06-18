//! `plugin:<id>.<verb>` JSON-RPC routing — the inverse seam (plan §1.5 F5 /
//! §1.10). A spawned agent runs `codeterm plugin <id> <verb> [args…]`; the CLI
//! sends method `plugin:<id>.<verb>` with `{ args, sessionId }`. We resolve the
//! owning plugin in [`AgentCommandRegistry`](crate::agent_commands), enforce its
//! `agent_commands` grant, optionally scope the verb to what the spawning plugin
//! declared at `host.agent.spawn` (C2 MINOR-2), then call its `onAgentCommand`.

use serde_json::Value;

use crate::agent_commands::AgentCommandRegistry;
use crate::service::ServiceState;

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
    })?;

    let handler = registry.get(&id).ok_or_else(|| JsonRpcError {
        code: -32601,
        message: format!("plugin '{id}' has no agent_commands handler"),
    })?;
    if !handler.granted() {
        return Err(JsonRpcError {
            code: -32603,
            message: format!("plugin '{id}' is not granted agent_commands"),
        });
    }
    if let Some(verbs) = allowed_verbs {
        if !verbs.is_empty() && !verbs.iter().any(|v| v == &verb) {
            return Err(JsonRpcError {
                code: -32603,
                message: format!("verb '{verb}' is not in the spawned worker's granted commands"),
            });
        }
    }

    let args = arg_strings(params);
    let sid = session_id(params);
    match handler.on_agent_command(&sid, &verb, &args) {
        Ok(result) => Ok(serde_json::json!({ "result": result })),
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
    })?;

    // C2 MINOR-2: a worker the plugin spawned (`spawned_by == plugin:<id>`) is
    // confined to the verbs the plugin declared via `host.agent.spawn(commands)`.
    // A non-plugin caller (orchestrator agent, etc.) is gated by the plugin-level
    // grant only.
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

    route(
        &svc.agent_command_registry,
        method,
        params,
        scope.as_deref(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_commands::{AgentCommandProvider, AgentCommandRegistry};
    use std::sync::Arc;

    struct Fake {
        id: String,
        granted: bool,
    }
    impl AgentCommandProvider for Fake {
        fn id(&self) -> &str {
            &self.id
        }
        fn granted(&self) -> bool {
            self.granted
        }
        fn on_agent_command(
            &self,
            session_id: &str,
            verb: &str,
            args: &[String],
        ) -> Result<String, String> {
            if verb == "boom" {
                return Err("plugin said no".into());
            }
            Ok(format!("{session_id}|{verb}|{}", args.join(",")))
        }
    }

    fn reg_with(id: &str, granted: bool) -> AgentCommandRegistry {
        let r = AgentCommandRegistry::new();
        r.register(
            id,
            Arc::new(Fake {
                id: id.into(),
                granted,
            }),
        );
        r
    }

    #[test]
    fn parses_id_and_verb() {
        assert_eq!(
            parse_method("plugin:local-shell.say"),
            Some(("local-shell".into(), "say".into()))
        );
        assert_eq!(
            parse_method("plugin:p.do.it"),
            Some(("p".into(), "do.it".into()))
        );
        assert!(parse_method("agent.report").is_none());
        assert!(parse_method("plugin:noverb").is_none());
        assert!(parse_method("plugin:.verb").is_none());
    }

    #[test]
    fn routes_to_granted_handler() {
        let reg = reg_with("npc", true);
        let params = Some(serde_json::json!({ "args": ["a", "b"], "sessionId": "s1" }));
        let out = route(&reg, "plugin:npc.say", &params, None).ok();
        assert_eq!(out, Some(serde_json::json!({ "result": "s1|say|a,b" })));
    }

    #[test]
    fn plugin_error_becomes_error_envelope_not_rpc_error() {
        let reg = reg_with("npc", true);
        let out = route(&reg, "plugin:npc.boom", &None, None).ok();
        assert_eq!(out, Some(serde_json::json!({ "error": "plugin said no" })));
    }

    #[test]
    fn ungranted_plugin_is_refused() {
        let reg = reg_with("npc", false);
        let err = route(&reg, "plugin:npc.say", &None, None).unwrap_err();
        assert_eq!(err.code, -32603);
        assert!(err.message.contains("not granted"));
    }

    #[test]
    fn unknown_plugin_is_method_not_found() {
        let reg = reg_with("npc", true);
        let err = route(&reg, "plugin:other.say", &None, None).unwrap_err();
        assert_eq!(err.code, -32601);
    }

    #[test]
    fn malformed_method_is_invalid_request() {
        let reg = reg_with("npc", true);
        let err = route(&reg, "plugin:npc", &None, None).unwrap_err();
        assert_eq!(err.code, -32600);
    }

    #[test]
    fn scope_denies_verb_outside_declared_commands() {
        let reg = reg_with("npc", true);
        let allowed = vec!["say".to_string(), "look".to_string()];
        let err = route(&reg, "plugin:npc.move", &None, Some(&allowed)).unwrap_err();
        assert_eq!(err.code, -32603);
        assert!(err.message.contains("granted commands"));
    }

    #[test]
    fn scope_allows_declared_verb() {
        let reg = reg_with("npc", true);
        let allowed = vec!["say".to_string()];
        let out = route(&reg, "plugin:npc.say", &None, Some(&allowed)).ok();
        assert_eq!(out, Some(serde_json::json!({ "result": "|say|" })));
    }

    #[test]
    fn empty_scope_does_not_restrict() {
        let reg = reg_with("npc", true);
        let allowed: Vec<String> = vec![];
        let out = route(&reg, "plugin:npc.anything", &None, Some(&allowed)).ok();
        assert_eq!(out, Some(serde_json::json!({ "result": "|anything|" })));
    }
}
