use super::*;
use codeterm_service_core::agent_commands::{AgentCommandProvider, AgentCommandRegistry};
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
        _body: Option<&str>,
    ) -> Result<codeterm_service_core::agent_commands::AgentCommandResult, String> {
        if verb == "boom" {
            return Err("plugin said no".into());
        }
        Ok(codeterm_service_core::agent_commands::AgentCommandResult {
            result: format!("{session_id}|{verb}|{}", args.join(",")),
            catalogue_changed: verb == "refresh",
        })
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
    assert!(parse_method("agent.diagnostic").is_none());
    assert!(parse_method("plugin:noverb").is_none());
    assert!(parse_method("plugin:.verb").is_none());
}

#[test]
fn routes_to_granted_handler() {
    let reg = reg_with("npc", true);
    let params = Some(serde_json::json!({ "args": ["a", "b"], "sessionId": "s1" }));
    let out = route(&reg, "plugin:npc.say", &params, None).ok();
    assert_eq!(
        out,
        Some(serde_json::json!({
            "result": "s1|say|a,b",
            "catalogue_changed": false,
        }))
    );
}

#[test]
fn plugin_command_can_report_catalogue_mutation() {
    let reg = reg_with("npc", true);

    let out = route(&reg, "plugin:npc.refresh", &None, None).unwrap();

    assert_eq!(out["catalogue_changed"], true);
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
    assert_eq!(
        out,
        Some(serde_json::json!({ "result": "|say|", "catalogue_changed": false }))
    );
}

#[test]
fn empty_scope_does_not_restrict() {
    let reg = reg_with("npc", true);
    let allowed: Vec<String> = vec![];
    let out = route(&reg, "plugin:npc.anything", &None, Some(&allowed)).ok();
    assert_eq!(
        out,
        Some(serde_json::json!({ "result": "|anything|", "catalogue_changed": false }))
    );
}

#[test]
fn body_survives_json_routing_without_becoming_arguments() {
    use codeterm_plugin::js_runtime::{JsAgentCommandHandler, PluginVm};
    let vm = Arc::new(
        PluginVm::from_source(
            "npc",
            r#"module.exports.default = {
        onAgentCommand(ctx) { return { result: JSON.stringify(ctx) }; }
    }"#
            .into(),
        )
        .unwrap(),
    );
    let registry = AgentCommandRegistry::new();
    registry.register("npc", Arc::new(JsAgentCommandHandler::new(vm, "npc", true)));
    for body in [
        None,
        Some(""),
        Some("--message"),
        Some("--"),
        Some("quoted \"body\" \r\nλ😀\n"),
    ] {
        let mut params = serde_json::json!({"sessionId": "session", "args": ["--", "recipient"]});
        if let Some(body) = body {
            params["body"] = body.into();
        }
        let wire = serde_json::to_vec(&params).unwrap();
        let params: Value = serde_json::from_slice(&wire).unwrap();
        let result = route(
            &registry,
            "plugin:npc.echo-body",
            &Some(params.clone()),
            None,
        )
        .unwrap();
        let ctx: Value = serde_json::from_str(result["result"].as_str().unwrap()).unwrap();
        let mut expected = params;
        expected["verb"] = "echo-body".into();
        assert_eq!(ctx, expected);
    }
}

#[test]
fn plugin_body_bound_counts_utf8_bytes_and_refuses_before_handler() {
    let registry = reg_with("npc", true);
    for body in ["a".repeat(1024 * 1024), "λ".repeat(512 * 1024)] {
        let params = Some(serde_json::json!({"body": body}));
        assert!(route(&registry, "plugin:npc.say", &params, None).is_ok());
    }
    let params = Some(serde_json::json!({"body": "λ".repeat(512 * 1024 + 1)}));
    let error = route(&registry, "plugin:npc.boom", &params, None).unwrap_err();
    assert_eq!(error.code, -32602);
    assert_eq!(error.data.unwrap()["code"], "plugin_body_too_large");
}
