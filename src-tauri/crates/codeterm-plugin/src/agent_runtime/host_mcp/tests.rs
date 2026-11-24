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
            http: None,
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
fn neighbor_reply_mcp_config_requires_exact_host_descriptor_on_fresh_and_resumed_threads() {
    let mut cap = capability();
    cap.host_mcp = Some(RuntimeHostMcpDeclaration {
        param: NEIGHBOR_REPLY_MCP_PARAM.into(),
        methods: vec!["thread/start".into(), "thread/resume".into()],
    });
    let mut ctx = context();
    let server = ctx.host_mcp.as_mut().unwrap();
    server.name = NEIGHBOR_REPLY_MCP_NAME.into();
    server.env =
        std::collections::BTreeMap::from([("CODETERM_SOCKET_PATH".into(), "opaque-socket".into())]);
    let config = json!({"config":{(NEIGHBOR_REPLY_CONFIG_KEY):{"command":server.command, "args":server.args,
        "env":server.env,"env_vars":["CODETERM_API_TOKEN"],"required":true,
        "enabled_tools":["reply"],"tools":{"reply":{"approval_mode":"approve"}}}}});
    for method in ["thread/start", "thread/resume"] {
        assert!(validate(&cap, &ctx, method, &config).is_ok());
        let mut shadowed = config.clone();
        shadowed["config"][format!("{NEIGHBOR_REPLY_CONFIG_KEY}.command")] = json!("other");
        assert!(validate(&cap, &ctx, method, &shadowed).is_err());
        let mut broadened = config.clone();
        broadened["config"][NEIGHBOR_REPLY_CONFIG_KEY]["env_vars"] =
            json!(["CODETERM_OWNER_TOKEN"]);
        assert!(validate(&cap, &ctx, method, &broadened).is_err());
        let mut server_wide = config.clone();
        server_wide["config"][NEIGHBOR_REPLY_CONFIG_KEY]["default_tools_approval_mode"] =
            json!("approve");
        assert!(validate(&cap, &ctx, method, &server_wide).is_err());
        let mut extra_tool = config.clone();
        extra_tool["config"][NEIGHBOR_REPLY_CONFIG_KEY]["tools"]["other"] =
            json!({"approval_mode":"approve"});
        assert!(validate(&cap, &ctx, method, &extra_tool).is_err());
        let mut missing_approval = config.clone();
        missing_approval["config"][NEIGHBOR_REPLY_CONFIG_KEY]
            .as_object_mut()
            .unwrap()
            .remove("tools");
        assert!(validate(&cap, &ctx, method, &missing_approval).is_err());
        assert!(validate(&cap, &ctx, method, &json!({})).is_err());
    }
    assert!(validate(&cap, &ctx, "turn/start", &config).is_err());
    assert!(validate(&cap, &RuntimeContext::default(), "thread/resume", &config).is_err());
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

#[test]
fn host_mcp_accepts_exact_http_descriptor_when_configured_and_refuses_tampering() {
    let mut ctx = context();
    ctx.host_mcp.as_mut().unwrap().http = Some(RuntimeHostMcpHttp {
        url: "http://127.0.0.1:7686/api/mcp".into(),
        headers: std::collections::BTreeMap::from([(
            "Authorization".into(),
            "Bearer <credential-placeholder>".into(),
        )]),
    });
    let http = ctx.host_mcp.as_ref().unwrap().http_descriptor().unwrap();
    assert!(validate_call(&ctx, json!({"mcpServers":[http.clone()]})).is_ok());
    let mut tampered = http;
    tampered["url"] = json!("http://127.0.0.1:9999/evil");
    assert!(validate_call(&ctx, json!({"mcpServers":[tampered]})).is_err());
}
