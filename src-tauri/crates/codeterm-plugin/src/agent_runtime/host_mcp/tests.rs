use super::*;
use crate::agent_runtime::{validate_runtime_call, RpcCall, RuntimeOperation};
use serde_json::json;

fn capability() -> AgentRuntimeCapability {
    serde_json::from_value(json!({"runtime":"stdio-jsonrpc","command":{"binary":"fixture","args":[]},"requestMethods":{"openThread":["session/new"]},"notificationMethods":[],"hostMcp":{"param":"mcpServers","methods":["session/new"]}})).unwrap()
}
fn context() -> RuntimeContext {
    RuntimeContext {
        host_mcp: Some(RuntimeHostMcp {
            env: std::collections::BTreeMap::from([(
                "CODETERM_API_TOKEN".into(),
                "<credential-placeholder>".into(),
            )]),
            name: "domios".into(),
            command: "/bin/domios".into(),
            args: vec![
                "mcp".into(),
                "serve".into(),
                "--tab".into(),
                "own".into(),
                "--binding-nonce".into(),
                "nonce".into(),
            ],
        }),
        ..Default::default()
    }
}

fn validate_call(ctx: &RuntimeContext, params: Value) -> Result<(), RuntimeFailure> {
    validate_runtime_call(
        &capability(),
        ctx,
        &RuntimeOperation::OpenThread {
            instructions: String::new(),
            settings: Default::default(),
        },
        &RpcCall {
            method: "session/new".into(),
            params,
            target: None,
        },
    )
}

#[test]
fn host_mcp_refuses_descriptor_tampering_before_write() {
    let ctx = context();
    let exact = ctx.host_mcp.as_ref().unwrap().descriptor();
    let mut changed_command = exact.clone();
    changed_command["command"] = json!("/foreign");
    let mut changed_args = exact.clone();
    changed_args["args"] = json!(["mcp", "serve", "--tab", "foreign"]);
    let mut changed_env = exact.clone();
    changed_env["env"] = json!([{"name":"OVERRIDE","value":"foreign"}]);
    let mut extra_field = exact.clone();
    extra_field["url"] = json!("http://foreign");
    for servers in [
        json!([exact.clone(), exact.clone()]),
        json!([changed_command]),
        json!([changed_args]),
        json!([changed_env]),
        json!([extra_field]),
        json!(null),
    ] {
        assert!(validate_call(&ctx, json!({"mcpServers":servers})).is_err());
    }
    assert!(validate_call(&RuntimeContext::default(), json!({"mcpServers":[exact]})).is_err());
}

#[test]
fn host_mcp_accepts_only_empty_or_exact_context_descriptor() {
    let ctx = context();
    for servers in [
        json!([]),
        json!([ctx.host_mcp.as_ref().unwrap().descriptor()]),
    ] {
        let call = RpcCall {
            method: "session/new".into(),
            params: json!({"mcpServers":servers}),
            target: None,
        };
        assert!(super::super::validate_runtime_call(
            &capability(),
            &ctx,
            &super::super::RuntimeOperation::OpenThread {
                instructions: "".into(),
                settings: Default::default()
            },
            &call
        )
        .is_ok());
    }
    assert!(validate(&capability(), &ctx, "session/new", &json!({})).is_ok());
}

#[test]
fn host_mcp_manifest_refuses_undeclared_duplicate_and_invalid_methods() {
    let mut cap = capability();
    assert!(cap.validate_declaration().is_ok());
    for declaration in [
        json!({"param":"","methods":["session/new"]}),
        json!({"param":"mcpServers","methods":[]}),
        json!({"param":"mcpServers","methods":["foreign"]}),
        json!({"param":"mcpServers","methods":["session/new","session/new"]}),
    ] {
        cap.host_mcp = Some(serde_json::from_value(declaration).unwrap());
        assert!(cap.validate_declaration().is_err());
    }
}

#[test]
fn host_mcp_refuses_tampering_on_other_outbound_methods() {
    let mut cap = capability();
    cap.request_methods
        .insert("initialize".into(), vec!["handshake".into()]);
    let ctx = context();
    let mut tampered = ctx.host_mcp.as_ref().unwrap().descriptor();
    tampered["command"] = json!("/foreign");
    for params in [
        json!({"mcpServers":[tampered]}),
        json!({"mcpServers":[{"url":"http://foreign"}]}),
    ] {
        let call = RpcCall {
            method: "handshake".into(),
            params,
            target: None,
        };
        assert!(validate_runtime_call(&cap, &ctx, &RuntimeOperation::Initialize, &call).is_err());
    }
    let call = RpcCall {
        method: "handshake".into(),
        params: json!({}),
        target: None,
    };
    assert!(validate_runtime_call(&cap, &ctx, &RuntimeOperation::Initialize, &call).is_ok());
}
