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
    let parsed = crate::Cli::try_parse_from([
        "domios",
        "mcp",
        "serve",
        "--tab",
        "own",
        "--binding-nonce",
        "nonce",
        "--mesh",
        "foreign",
    ])
    .unwrap();
    assert!(refuse_mesh_target(parsed.mesh.as_deref()).is_err());
    assert!(refuse_mesh_target(None).is_ok());
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
            "notify_send",
            "messages_check"
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

/// A renamed or removed CLI flag fails here instead of breaking an agent at runtime.
#[test]
fn every_tool_mapping_with_every_field_still_parses_as_a_cli_command() {
    fn sample(field: &str) -> &'static str {
        match field {
            "target_tab" => "own",
            "from_pad" => "payload.md",
            "status" => "in_review",
            "sha" => "abc1234",
            "query" | "area" | "id" | "title" => "opaque",
            "kind" => "fact",
            "level" => "verbose",
            "severity" => "info",
            other => panic!("add a valid sample for the MCP field {other}"),
        }
    }
    for tool in tools::TOOLS {
        let arguments: serde_json::Map<String, Value> = tool
            .required
            .iter()
            .chain(tool.optional)
            .map(|field| ((*field).to_owned(), json!(sample(field))))
            .collect();
        let args = tool_args(tool.name, &Value::Object(arguments))
            .unwrap_or_else(|reason| panic!("{}: {reason}", tool.name));
        let parsed =
            crate::Cli::try_parse_from(std::iter::once("domios".to_owned()).chain(args.clone()));
        if let Err(error) = parsed {
            panic!("{} no longer parses as {args:?}: {error}", tool.name);
        }
    }
    assert!(
        tool_args(tools::MESSAGES_TOOL, &json!({})).is_err()
            && is_messages_check(tools::MESSAGES_TOOL, &json!({})),
        "messages_check maps to no CLI command, so it has no argv to parse"
    );
}

#[test]
fn every_tool_and_field_has_one_description_and_every_description_has_its_owner() {
    use std::collections::BTreeSet;
    let source: Value =
        serde_json::from_str(include_str!("../../../../src-tauri/prompts/mcp_tools.json")).unwrap();
    let keys = |section: &str| -> BTreeSet<String> {
        source[section]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect()
    };
    let listed: BTreeSet<String> = tools()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(keys("tools"), listed);
    let fields: BTreeSet<String> = tools::TOOLS
        .iter()
        .flat_map(|tool| tool.required.iter().chain(tool.optional))
        .map(|field| (*field).to_owned())
        .collect();
    assert_eq!(keys("arguments"), fields);
}

#[test]
fn mcp_startup_ack_runs_the_task_ack_only_for_a_task_bound_tab() {
    let startup = tool_args("agent_startup_ack", &json!({})).unwrap();
    assert_eq!(startup, ["--json", "agent", "startup-ack"]);
    assert_eq!(launch_args(startup.clone(), false), startup);
    let task_ack = launch_args(startup, true);
    assert_eq!(task_ack, ["--json", "agent", "ack"]);
    assert!(
        crate::Cli::try_parse_from(std::iter::once("domios".to_owned()).chain(task_ack)).is_ok()
    );
    let report = tool_args("agent_report", &json!({"from_pad":"report.md"})).unwrap();
    assert_eq!(launch_args(report.clone(), true), report);
}

#[derive(Default)]
struct FakeInbound {
    waiting: Option<&'static str>,
    claims: usize,
    settled: Vec<(String, bool)>,
}

impl Inbound for FakeInbound {
    fn claim(&mut self) -> Option<inbound::Claim> {
        self.claims += 1;
        self.waiting.take().map(|text| inbound::Claim {
            id: format!("claim-{}", self.claims),
            text: text.into(),
        })
    }
    fn settle(&mut self, id: &str, written: bool) {
        self.settled.push((id.into(), written));
    }
}

fn tool_frame(id: u64, name: &str, arguments: Value) -> String {
    format!(
        "{}\n",
        json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":arguments}})
    )
}

fn replies(output: Vec<u8>) -> Vec<Value> {
    String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn a_waiting_message_rides_the_next_tool_result_once_and_settles_after_it_was_written() {
    let input = [
        tool_frame(1, "mem_get", json!({"id":"opaque"})),
        tool_frame(2, "mem_get", json!({"id":"opaque"})),
    ]
    .concat();
    let mut output = Vec::new();
    let mut inbound = FakeInbound {
        waiting: Some("opaque inbound"),
        ..Default::default()
    };
    protocol_with(input.as_bytes(), &mut output, "", &mut inbound, |_| {
        Ok("opaque-result".into())
    })
    .unwrap();
    let replies = replies(output);
    assert_eq!(
        replies[0]["result"]["content"],
        json!([{"type":"text","text":"opaque-result"},{"type":"text","text":"opaque inbound"}])
    );
    assert_eq!(
        replies[1]["result"]["content"],
        json!([{"type":"text","text":"opaque-result"}]),
        "the next result does not repeat it"
    );
    assert_eq!(inbound.settled, [("claim-1".to_owned(), true)]);
}

#[test]
fn a_failed_write_leaves_the_claim_unsettled_because_bytes_may_have_gone_out() {
    struct BrokenPipe;
    impl std::io::Write for BrokenPipe {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let input = tool_frame(1, "mem_get", json!({"id":"opaque"}));
    let mut inbound = FakeInbound {
        waiting: Some("opaque inbound"),
        ..Default::default()
    };
    assert!(
        protocol_with(input.as_bytes(), &mut BrokenPipe, "", &mut inbound, |_| Ok(
            "opaque-result".into()
        ))
        .is_err()
    );
    assert_eq!(inbound.claims, 1);
    assert!(inbound.settled.is_empty(), "no delivery was proven");
}

#[test]
fn only_a_successful_tool_result_can_carry_a_message() {
    let input = [
        tool_frame(1, "agent_report", json!({"from_pad":"report.md"})),
        tool_frame(2, "mem_get", json!({"id":"opaque","extra":"refused"})),
        format!(
            "{}\n",
            json!({"jsonrpc":"2.0","id":3,"method":"tools/list"})
        ),
    ]
    .concat();
    let mut output = Vec::new();
    let mut inbound = FakeInbound {
        waiting: Some("opaque inbound"),
        ..Default::default()
    };
    protocol_with(input.as_bytes(), &mut output, "", &mut inbound, |_| {
        Err("opaque failure")
    })
    .unwrap();
    assert_eq!(inbound.claims, 0, "an agent may discard a failed call");
    assert!(!String::from_utf8(output)
        .unwrap()
        .contains("opaque inbound"));
}

#[test]
fn checking_messages_runs_no_command_and_returns_what_is_waiting() {
    let input = tool_frame(1, "messages_check", json!({}));
    let mut output = Vec::new();
    let mut inbound = FakeInbound {
        waiting: Some("opaque inbound"),
        ..Default::default()
    };
    protocol_with(input.as_bytes(), &mut output, "", &mut inbound, |_| {
        panic!("this tool has no command")
    })
    .unwrap();
    assert_eq!(
        replies(output)[0]["result"]["content"][1]["text"],
        "opaque inbound"
    );
    assert_eq!(inbound.settled, [("claim-1".to_owned(), true)]);

    let refused = tool_frame(1, "messages_check", json!({"tab":"foreign"}));
    let mut output = Vec::new();
    protocol(refused.as_bytes(), &mut output, "", |_| {
        panic!("this tool has no command")
    })
    .unwrap();
    assert_eq!(replies(output)[0]["error"]["code"], -32602);
}

#[test]
fn a_queued_send_reaches_the_sending_agent_as_its_receipt_not_as_a_failure() {
    let held =
        json!({"ok":false,"error":{"code":"held"},"result":{"send_id":"opaque","status":"held"}});
    assert!(is_held_receipt(held.to_string().as_bytes()));
    for other in [
        json!({"ok":false,"error":{"code":"internal"},"result":{"send_id":"opaque"}}),
        json!({"ok":false,"error":{"code":"held"},"result":null}),
    ] {
        assert!(!is_held_receipt(other.to_string().as_bytes()));
    }
    assert!(!is_held_receipt(b"not json"));
}
