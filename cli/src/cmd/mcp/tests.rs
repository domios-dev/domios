use super::*;
use clap::Parser;
use serde_json::json;

fn environment() -> Vec<(&'static str, &'static str)> {
    vec![
        ("CODETERM_API_TOKEN", "opaque-test-credential"),
        ("CODETERM_API_URL", "http://127.0.0.1:43210"),
        ("CODETERM_TAB_ID", "own"),
        ("CODETERM_SESSION_BINDING_NONCE", "nonce"),
    ]
}

#[test]
fn mcp_serve_missing_credential_and_binding_mismatch_fail_closed() {
    assert!(validate_launch("own", "nonce", |key| environment()
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v.to_string()))
    .is_ok());
    for missing in 0..4 {
        let mut env = environment();
        env.remove(missing);
        assert!(validate_launch("own", "nonce", |key| env
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.to_string()))
        .is_err());
    }
    for (tab, nonce) in [("foreign", "nonce"), ("own", "foreign")] {
        assert!(validate_launch(tab, nonce, |key| environment()
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.to_string()))
        .is_err());
    }
}

#[test]
fn mcp_serve_refuses_url_mesh_instance_and_identity_overrides() {
    for flag in [
        "--api-url",
        "--port",
        "--token",
        "--instance",
        "--mesh",
        "--caller-tab",
    ] {
        assert!(crate::Cli::try_parse_from([
            "domios",
            "mcp",
            "serve",
            "--tab",
            "own",
            "--binding-nonce",
            "nonce",
            flag,
            "foreign"
        ])
        .is_err());
    }
    for key in ["url", "mesh", "instance", "tab", "caller_tab_id", "token"] {
        assert!(tool_args("mem_search", &json!({"query":"opaque",key:"foreign"})).is_err());
    }
}

#[test]
fn mcp_serve_fixed_allowlist_and_unknown_fields() {
    assert_eq!(
        tools()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec![
            "send",
            "agent_report",
            "agent_startup_ack",
            "mem_search",
            "mem_get",
            "task_show",
            "notify_send"
        ]
    );
    for name in ["spawn", "mem_secret", "mesh", "config"] {
        assert!(tool_args(name, &json!({})).is_err());
    }
    assert!(tool_args(
        "agent_report",
        &json!({"from_pad":"report.md", "tab":"foreign"})
    )
    .is_err());
    assert!(tool_args("agent_startup_ack", &json!({"unexpected":true})).is_err());
}

#[test]
fn mcp_serve_values_cannot_inject_cli_flags_or_caller_identity() {
    let args = tool_args("mem_search", &json!({"query":"--mesh=foreign"})).unwrap();
    assert!(
        args.contains(&"--query=--mesh=foreign".to_string())
            || args.ends_with(&["--".into(), "--mesh=foreign".into()])
    );
    let report = tool_args("agent_report", &json!({"from_pad":"report.md"})).unwrap();
    assert!(!report
        .iter()
        .any(|arg| arg.starts_with("--tab") || arg.starts_with("--mesh")));
}

#[test]
fn mcp_serve_token_never_reaches_protocol_output() {
    let input = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"mem_get\",\"arguments\":{\"id\":\"opaque\"}}}\n";
    let mut output = Vec::new();
    protocol(
        &input[..],
        &mut output,
        "opaque-test-credential",
        false,
        |_| Ok("opaque-test-credential".into()),
    )
    .unwrap();
    assert!(!String::from_utf8(output)
        .unwrap()
        .contains("opaque-test-credential"));
}

#[test]
fn mcp_serve_restricted_scope_refuses_out_of_scope_tools() {
    let input = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"agent_report\",\"arguments\":{\"from_pad\":\"report.md\"}}}\n";
    let mut output = Vec::new();
    protocol(
        &input[..],
        &mut output,
        "opaque-test-credential",
        true,
        |_| panic!("must refuse before execution"),
    )
    .unwrap();
    assert!(String::from_utf8(output).unwrap().contains("scope"));
}

#[test]
fn mcp_serve_refuses_mesh_target_and_token_in_arguments() {
    for target in ["mesh:peer/foreign", "own,foreign", "--mesh=foreign"] {
        assert!(tool_args(
            "send",
            &json!({"target_tab":target,"from_pad":"message.md"})
        )
        .is_err());
    }
    let input = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"mem_get\",\"arguments\":{\"id\":\"opaque-test-credential\"}}}\n";
    let mut output = Vec::new();
    protocol(
        &input[..],
        &mut output,
        "opaque-test-credential",
        false,
        |_| panic!("credential must never become an argument"),
    )
    .unwrap();
    assert!(!String::from_utf8(output)
        .unwrap()
        .contains("opaque-test-credential"));
}

#[test]
fn mcp_serve_initialize_list_and_call_delegate_through_cli_port() {
    let input = [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"mem_get","arguments":{"id":"opaque"}}}),
    ].iter().map(|frame|format!("{frame}\n")).collect::<String>();
    let mut output = Vec::new();
    let mut calls = Vec::new();
    protocol(
        input.as_bytes(),
        &mut output,
        "opaque-test-credential",
        false,
        |args| {
            calls.push(args.to_vec());
            Ok("opaque-result".into())
        },
    )
    .unwrap();
    let replies = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(replies.len(), 3);
    assert_eq!(replies[0]["result"]["capabilities"], json!({"tools":{}}));
    assert_eq!(replies[1]["result"]["tools"], json!(tools()));
    assert_eq!(
        replies[2]["result"]["content"],
        json!([{"type":"text","text":"opaque-result"}])
    );
    assert_eq!(calls, vec![vec!["--json", "mem", "get", "--", "opaque"]]);
}
