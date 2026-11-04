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
    protocol(&input[..], &mut output, "opaque-test-credential", |_| {
        Ok("opaque-test-credential".into())
    })
    .unwrap();
    assert!(!String::from_utf8(output)
        .unwrap()
        .contains("opaque-test-credential"));
}

#[test]
fn mcp_serve_child_admission_refusal_becomes_tool_error() {
    let input = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"agent_report\",\"arguments\":{\"from_pad\":\"report.md\"}}}\n";
    let mut output = Vec::new();
    let mut executed = false;
    protocol(&input[..], &mut output, "opaque-test-credential", |args| {
        assert_eq!(args, ["--json", "agent", "report", "--from-pad=report.md"]);
        executed = true;
        Err("Child CLI admission refused")
    })
    .unwrap();
    let reply: Value = serde_json::from_slice(&output).unwrap();
    assert!(executed);
    assert_eq!(reply["result"]["isError"], true);
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
    protocol(&input[..], &mut output, "opaque-test-credential", |_| {
        panic!("credential must never become an argument")
    })
    .unwrap();
    let reply: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(reply["id"], 1);
    assert_eq!(reply["error"]["code"], -32602);
    assert!(!String::from_utf8(output)
        .unwrap()
        .contains("opaque-test-credential"));
}

#[test]
fn mcp_serve_initialize_list_and_call_delegate_through_cli_port() {
    let input = [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"_meta":{"correlation":"initialize"}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{"_meta":{"correlation":"ready"}}}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{"_meta":{"correlation":"list"}}}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"mem_get","arguments":{"id":"opaque"}}}),
    ].iter().map(|frame|format!("{frame}\n")).collect::<String>();
    let mut output = Vec::new();
    let mut calls = Vec::new();
    protocol(
        input.as_bytes(),
        &mut output,
        "opaque-test-credential",
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

#[test]
fn mcp_serve_tool_meta_is_ignored_but_other_unknown_params_refuse() {
    let input = [
        json!({"jsonrpc":"2.0","id":"progress-call","method":"tools/call","params":{"name":"mem_get","arguments":{"id":"opaque"},"_meta":{"progressToken":"opaque-progress","correlation":"opaque"}}}),
        json!({"jsonrpc":"2.0","id":"unknown-param","method":"tools/call","params":{"name":"mem_get","arguments":{"id":"opaque"},"extra":true}}),
    ].iter().map(|frame| format!("{frame}\n")).collect::<String>();
    let mut output = Vec::new();
    let mut executions = 0;
    protocol(
        input.as_bytes(),
        &mut output,
        "opaque-test-credential",
        |_| {
            executions += 1;
            Ok("opaque-result".into())
        },
    )
    .unwrap();
    let replies = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(executions, 1);
    assert_eq!(replies[0]["id"], "progress-call");
    assert_eq!(
        replies[0]["result"]["content"],
        json!([{"type":"text","text":"opaque-result"}])
    );
    assert_eq!(replies[1]["error"]["code"], -32602);
}

#[test]
fn mcp_serve_negotiates_supported_client_version_or_newest() {
    for offered in ["2025-06-18", "2025-03-26", "2024-11-05", "unknown"] {
        let input = format!(
            "{}\n",
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":offered}})
        );
        let mut output = Vec::new();
        protocol(std::io::Cursor::new(input), &mut output, "", |_| {
            panic!("initialize must not execute a tool")
        })
        .unwrap();
        let response: Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(
            response["result"]["protocolVersion"],
            if offered == "unknown" {
                "2025-06-18"
            } else {
                offered
            }
        );
    }
    assert_eq!(negotiated_version(None), "2025-06-18");
}

#[test]
fn mcp_tool_and_argument_descriptions_come_from_prompt_source() {
    let source: Value =
        serde_json::from_str(include_str!("../../../../src-tauri/prompts/mcp_tools.json")).unwrap();
    for tool in tools() {
        let name = tool["name"].as_str().unwrap();
        assert_eq!(tool["description"], source["tools"][name]);
        assert!(!tool["description"].as_str().unwrap().trim().is_empty());
        for (key, property) in tool["inputSchema"]["properties"].as_object().unwrap() {
            assert_eq!(property["description"], source["arguments"][key]);
            assert!(!property["description"].as_str().unwrap().trim().is_empty());
        }
    }
}
