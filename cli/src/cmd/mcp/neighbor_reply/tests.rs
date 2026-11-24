use super::*;
use interprocess::local_socket::{prelude::*, GenericNamespaced, ListenerOptions};
use std::io::{BufRead, BufReader, Write};

fn call(id: u64, name: &str, arguments: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":arguments}})
}

#[test]
fn neighbor_reply_channel_lists_one_tool_and_rejects_other_destinations_and_operations() {
    let question = "a".repeat(32);
    let shape = json!({"kind":"answer","in_reply_to":question,"body":"opaque answer"});
    let mut foreign = shape.clone();
    foreign["friendship_id"] = json!("b".repeat(64));
    let requests = [
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
        call(2, "send", shape.clone()),
        call(3, NEIGHBOR_REPLY_MCP_TOOL, foreign),
        call(
            4,
            NEIGHBOR_REPLY_MCP_TOOL,
            json!({"kind":"question","in_reply_to":question,"body":"opaque"}),
        ),
        call(
            5,
            NEIGHBOR_REPLY_MCP_TOOL,
            json!({"kind":"refusal","in_reply_to":"foreign","body":"opaque"}),
        ),
        call(
            6,
            NEIGHBOR_REPLY_MCP_TOOL,
            json!({"kind":"refusal","in_reply_to":question,"body":"opaque token"}),
        ),
        call(7, NEIGHBOR_REPLY_MCP_TOOL, shape),
        call(
            8,
            NEIGHBOR_REPLY_MCP_TOOL,
            json!({"kind":"refusal","in_reply_to":question,"body":"opaque refusal"}),
        ),
    ];
    let input = requests
        .iter()
        .map(|request| format!("{request}\n"))
        .collect::<String>();
    let mut output = Vec::new();
    let mut sent = Vec::new();
    protocol(input.as_bytes(), &mut output, "opaque token", |reply| {
        sent.push(reply);
        Ok("opaque receipt".into())
    })
    .unwrap();
    let rows: Vec<Value> = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|row| serde_json::from_str(row).unwrap())
        .collect();
    let tools = rows[0]["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"], NEIGHBOR_REPLY_MCP_TOOL);
    for row in &rows[1..6] {
        assert_eq!(row["error"]["code"], -32602);
    }
    assert_eq!(sent.len(), 2);
    assert!(matches!(sent[0].kind, Kind::Answer));
    assert!(matches!(sent[1].kind, Kind::Refusal));
    assert!(sent.iter().all(|reply| reply.in_reply_to == question));
}

#[test]
fn neighbor_reply_channel_sends_once_to_the_pinned_local_endpoint_with_own_binding() {
    let name = format!(
        "domios-neighbor-reply-{}.sock",
        uuid::Uuid::new_v4().simple()
    );
    let listener = ListenerOptions::new()
        .name(name.as_str().to_ns_name::<GenericNamespaced>().unwrap())
        .create_sync()
        .unwrap();
    let server = std::thread::spawn(move || {
        let mut stream = BufReader::new(listener.accept().unwrap());
        let mut line = String::new();
        stream.read_line(&mut line).unwrap();
        let request: Value = serde_json::from_str(&line).unwrap();
        let body =
            json!({"message_id":"opaque-receipt","status":"pending","submission":"unconfirmed"});
        let response = json!({"jsonrpc":"2.0","id":request["id"],"result":{"status":200,"body":body.to_string()}});
        writeln!(stream.get_mut(), "{response}").unwrap();
        stream.get_mut().flush().unwrap();
        request
    });
    let friendship = "a".repeat(64);
    let question = "b".repeat(32);
    let launch = Launch {
        tab: "own-tab",
        nonce: "own-nonce",
        friendship: &friendship,
        socket: codeterm_paths::namespaced_socket_display_path(&name),
        token: "own-tab-credential".into(),
    };
    let receipt = launch
        .send(Reply {
            in_reply_to: question.clone(),
            kind: Kind::Answer,
            body: "opaque answer".into(),
        })
        .unwrap();
    assert!(receipt.contains("Do not replay"));
    let request = server.join().unwrap();
    assert_eq!(request["credential"], "own-tab-credential");
    assert_eq!(request["method"], "api.request");
    let params = &request["params"];
    assert_eq!(params["caller_tab_id"], "own-tab");
    assert_eq!(params["session_binding_nonce"], "own-nonce");
    assert_eq!(params["path"], RUNTIME_ROUTE);
    let body: Value = serde_json::from_str(params["body"].as_str().unwrap()).unwrap();
    assert_eq!(
        body,
        json!({"operation":"send","friendship_id":friendship,"in_reply_to":question,"kind":"answer","body":"opaque answer"})
    );
}

#[test]
fn neighbor_reply_channel_exposes_typed_refusals_without_daemon_details() {
    let name = format!(
        "domios-neighbor-refusals-{}.sock",
        uuid::Uuid::new_v4().simple()
    );
    let listener = ListenerOptions::new()
        .name(name.as_str().to_ns_name::<GenericNamespaced>().unwrap())
        .create_sync()
        .unwrap();
    let server = std::thread::spawn(move || {
        let responses = [
            json!({"error":{"code":-32003,"message":"opaque daemon detail","data":{"reason":"invalid_token"}}}),
            json!({"result":{"status":400,"body":json!({"code":"reply_target_unknown","message":"opaque daemon detail"}).to_string()}}),
            json!({"result":{"status":403,"body":json!({"error":{"code":"opaque_credential","message":"opaque daemon detail"}}).to_string()}}),
        ];
        for mut response in responses {
            let mut stream = BufReader::new(listener.accept().unwrap());
            let mut line = String::new();
            stream.read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            response["jsonrpc"] = "2.0".into();
            response["id"] = request["id"].clone();
            writeln!(stream.get_mut(), "{response}").unwrap();
            stream.get_mut().flush().unwrap();
        }
    });
    let friendship = "a".repeat(64);
    let launch = Launch {
        tab: "own-tab",
        nonce: "own-nonce",
        friendship: &friendship,
        socket: codeterm_paths::namespaced_socket_display_path(&name),
        token: "opaque_credential".into(),
    };
    let input = (1..=3)
        .map(|id| {
            format!(
                "{}\n",
                call(
                    id,
                    NEIGHBOR_REPLY_MCP_TOOL,
                    json!({"kind":"answer","in_reply_to":"b".repeat(32),"body":"opaque answer"})
                )
            )
        })
        .collect::<String>();
    let mut output = Vec::new();
    protocol(input.as_bytes(), &mut output, &launch.token, |reply| {
        launch.send(reply)
    })
    .unwrap();
    server.join().unwrap();
    let output = String::from_utf8(output).unwrap();
    assert!(!output.contains("opaque daemon detail"));
    assert!(!output.contains(&launch.token));
    let refusals: Vec<Value> = output
        .lines()
        .map(|line| {
            let response: Value = serde_json::from_str(line).unwrap();
            assert_eq!(response["result"]["isError"], true);
            serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap())
                .unwrap()
        })
        .collect();
    assert_eq!(
        refusals,
        [
            json!({"rpc_code":-32003,"code":"invalid_token"}),
            json!({"http_status":400,"code":"reply_target_unknown"}),
            json!({"http_status":403,"code":"http_refused"}),
        ]
    );
}
