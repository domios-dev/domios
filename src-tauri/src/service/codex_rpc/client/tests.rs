use super::*;

#[test]
fn codex_rpc_client_connection_failure_distinguishes_transport_from_refusal() {
    use tokio_tungstenite::tungstenite::{http::Response, Error};
    assert_eq!(
        connection_error(Error::Io(std::io::ErrorKind::ConnectionRefused.into())),
        "RPC WebSocket connection failed"
    );
    for kind in [
        std::io::ErrorKind::PermissionDenied,
        std::io::ErrorKind::InvalidInput,
    ] {
        assert!(connection_error(Error::Io(kind.into()))
            .starts_with("RPC WebSocket handshake refused:"));
    }
    let error = connection_error(Error::Http(Box::new(
        Response::builder().status(403).body(None).unwrap(),
    )));
    assert!(error.starts_with("RPC WebSocket handshake refused:"));
}

fn client() -> (Client, mpsc::Receiver<Message>) {
    let (outgoing, messages) = mpsc::channel(64);
    let (events, _) = broadcast::channel(32);
    let (observations, _) = broadcast::channel(32);
    let (death, _) = watch::channel(None);
    (
        Client(Arc::new(Inner {
            state: Mutex::new(State {
                closed: None,
                pending: HashMap::new(),
                buffer: NotificationBuffer::new(2, 2),
                additional_context: HashMap::new(),
            }),
            next_id: AtomicI64::new(1),
            initialized: AtomicBool::new(false),
            outgoing: Mutex::new(outgoing),
            epoch: AtomicU64::new(0),
            resync: watch::channel(0).0,
            events,
            observations,
            death,
        })),
        messages,
    )
}

#[tokio::test]
async fn codex_rpc_client_keeps_server_ids_separate() {
    let (client, mut outgoing) = client();
    let mut events = client.subscribe();
    let request = client.request("account/read", json!({}));
    tokio::pin!(request);
    tokio::select! { _ = &mut request => panic!("premature response"), message = outgoing.recv() => assert!(message.is_some()) }
    client
        .ingest(Frame::ServerRequest {
            id: RequestId::Number(1),
            method: "approval".into(),
            params: json!({}),
        })
        .unwrap();
    assert!(matches!(
        events.recv().await.unwrap(),
        Frame::ServerRequest { .. }
    ));
    assert_eq!(client.0.state.lock().pending.len(), 1);
    client
        .ingest(Frame::Response {
            id: RequestId::Number(1),
            result: Ok(json!({"account":null})),
        })
        .unwrap();
    assert_eq!(request.await.unwrap(), json!({"account":null}));
}

#[tokio::test]
async fn codex_rpc_client_initialize_once_then_notification() {
    let (client, mut outgoing) = client();
    let init = client.initialize();
    tokio::pin!(init);
    tokio::select! { _ = &mut init => panic!(), message = outgoing.recv() => assert!(message.is_some()) }
    client
        .ingest(Frame::Response {
            id: RequestId::Number(1),
            result: Ok(json!({"userAgent":"codex/0.160.0"})),
        })
        .unwrap();
    init.await.unwrap();
    let Message::Text(text) = outgoing.recv().await.unwrap() else {
        panic!()
    };
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap()["method"],
        "initialized"
    );
    assert!(client.initialize().await.is_err());
    assert!(outgoing.try_recv().is_err());
}

#[tokio::test]
async fn codex_rpc_client_death_fails_pending_and_rejects_new_requests() {
    let (client, mut outgoing) = client();
    let request = client.request("account/read", json!({}));
    tokio::pin!(request);
    tokio::select! { _ = &mut request => panic!(), _ = outgoing.recv() => {} }
    client.close("process died");
    assert_eq!(
        request.await,
        Err(ClientError::Closed("process died".into()))
    );
    assert!(client.0.state.lock().pending.is_empty());
    assert!(client.request("account/read", json!({})).await.is_err());
    assert_eq!(client.death().borrow().as_deref(), Some("process died"));
}

#[tokio::test]
async fn codex_rpc_client_forbids_global_config_writes() {
    let (client, mut outgoing) = client();
    for method in ["config/batchWrite", "config/value/write"] {
        assert!(client.request(method, json!({})).await.is_err());
    }
    assert!(outgoing.try_recv().is_err());
}

#[tokio::test]
async fn codex_rpc_control_history_paginates_turns_and_items_without_jsonl() {
    tokio::time::timeout(
        Duration::from_secs(5),
        history_paginates_turns_and_items_without_jsonl(),
    )
    .await
    .expect("history fake and client must complete together");
}

async fn history_paginates_turns_and_items_without_jsonl() {
    let (client, mut outgoing) = client();
    let reader = client.clone();
    let snapshot =
        tokio::spawn(async move { super::super::transcript::history(&reader, "bound").await });
    for (method, cursor, turn, response) in [
        (
            "thread/turns/list",
            None,
            None,
            json!({"data":[{"id":"old","status":"completed"}],"nextCursor":"turn-page"}),
        ),
        (
            "thread/turns/list",
            Some("turn-page"),
            None,
            json!({"data":[{"id":"active","status":"inProgress","startedAt":42},{"id":"queued","status":"inProgress","startedAt":null}]}),
        ),
        (
            "thread/items/list",
            None,
            Some("old"),
            json!({"data":[{"turnId":"old","item":{"id":"user-id","type":"userMessage","content":[{"type":"text","text":"opaque-input"}]}}],"nextCursor":"item-page"}),
        ),
        (
            "thread/items/list",
            Some("item-page"),
            Some("old"),
            json!({"data":[
                {"turnId":"old","item":{"id":"command-id","type":"commandExecution","command":"codeterm blueprint list","status":"completed","exitCode":0,"aggregatedOutput":"one\ntwo\n"}},
                {"turnId":"old","item":{"id":"mcp-id","type":"mcpToolCall","server":"codeterm","tool":"skill","arguments":{"name":"style"},"status":"completed","result":{"content":[{"type":"text","text":"skill body"}]}}},
                {"turnId":"old","item":{"id":"answer-id","type":"agentMessage","text":"opaque-answer"}}
            ]}),
        ),
        (
            "thread/items/list",
            None,
            Some("active"),
            json!({"data":[
                {"turnId":"active","startedAtMs":42,"completedAtMs":null,"item":{"id":"restored-open","type":"agentMessage","text":"hel"}},
                {"turnId":"active","completedAtMs":43,"item":{"id":"restored-closed","type":"agentMessage","text":"done"}},
                {"turnId":"active","item":{"id":"closed-tool","type":"commandExecution","command":"opaque","status":"completed"}}
            ]}),
        ),
        (
            "thread/items/list",
            None,
            Some("queued"),
            json!({"data":[]}),
        ),
        ("thread/goal/get", None, None, json!({"goal":null})),
    ] {
        let Message::Text(text) = outgoing.recv().await.unwrap() else {
            panic!("expected request")
        };
        let request: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(request["method"], method);
        assert_eq!(request["params"]["threadId"], "bound");
        assert_eq!(request["params"]["cursor"].as_str(), cursor);
        assert_eq!(request["params"]["turnId"].as_str(), turn);
        if method == "thread/turns/list" {
            assert_eq!(request["params"]["itemsView"], "notLoaded");
        }
        client
            .ingest(Frame::Response {
                id: serde_json::from_value(request["id"].clone()).unwrap(),
                result: Ok(response),
            })
            .unwrap();
    }
    let history = snapshot.await.unwrap().unwrap();
    assert_eq!(
        history
            .messages
            .iter()
            .map(|message| message.id.as_str())
            .collect::<Vec<_>>(),
        [
            "old:turn/started",
            "user-id",
            "command-id",
            "mcp-id",
            "answer-id",
            "old:turn/completed",
            "active:turn/started",
            "restored-open",
            "restored-closed",
            "closed-tool",
        ]
    );
    assert_eq!(history.messages[2].tool_kind.as_deref(), Some("command"));
    assert_eq!(history.messages[2].tool_exit_code, Some(0));
    assert_eq!(
        history.messages[2].tool_result.as_deref(),
        Some("one\ntwo\n")
    );
    assert_eq!(
        history.messages[3].tool_name.as_deref(),
        Some("codeterm · skill")
    );
    assert_eq!(history.active_turn.as_deref(), Some("active"));
    assert_eq!(history.completed_turns, ["old"]);
    assert_eq!(
        history.open_items,
        HashMap::from([("restored-open".into(), "active".into())])
    );
    let mut normalizer = history.normalizer("bound");
    assert_eq!(
        normalizer.finish_restored_turn("bound", "active"),
        std::collections::HashSet::from(["restored-open".into()])
    );
    assert!(outgoing.try_recv().is_err());
}

#[tokio::test(start_paused = true)]
async fn codex_rpc_control_history_optional_goal_never_discards_transcript() {
    for response in [
        Some(Ok(json!({"goal":null}))),
        Some(Ok(json!({}))),
        Some(Ok(json!({"goal":{"objective":"inspect"}}))),
        Some(Err(RpcError {
            code: -32601,
            message: "unsupported method".into(),
            data: None,
        })),
        None,
    ] {
        let (client, mut outgoing) = client();
        let silence = response.is_none();
        let expected_goal = response
            .as_ref()
            .and_then(|result| result.as_ref().ok())
            .and_then(|value| value["goal"]["objective"].as_str())
            .map(str::to_owned);
        let server = async {
            for (method, result) in [
                (
                    "thread/turns/list",
                    Some(Ok(json!({"data":[{"id":"turn","status":"completed"}]}))),
                ),
                (
                    "thread/items/list",
                    Some(Ok(
                        json!({"data":[{"turnId":"turn","item":{"id":"command","type":"commandExecution","command":"git status","status":"completed","exitCode":0,"aggregatedOutput":"clean"}}]}),
                    )),
                ),
                ("thread/goal/get", response),
            ] {
                let Message::Text(text) = outgoing.recv().await.unwrap() else {
                    panic!("expected history request")
                };
                let request: Value = serde_json::from_str(&text).unwrap();
                assert_eq!(request["method"], method);
                assert_eq!(request["params"]["threadId"], "bound");
                if let Some(result) = result {
                    client
                        .ingest(Frame::Response {
                            id: serde_json::from_value(request["id"].clone()).unwrap(),
                            result,
                        })
                        .unwrap();
                }
            }
        };
        let started = tokio::time::Instant::now();
        let (history, ()) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(super::super::transcript::history(&client, "bound"), server)
        })
        .await
        .expect("optional goal cannot block history");
        let history = history.unwrap();
        let command = history
            .messages
            .iter()
            .find(|row| row.id == "command")
            .unwrap();
        assert_eq!(command.tool_result.as_deref(), Some("clean"));
        assert_eq!(command.tool_exit_code, Some(0));
        assert_eq!(history.completed_turns, ["turn"]);
        let goal = history
            .messages
            .iter()
            .find_map(|row| row.session_intent.as_ref());
        assert_eq!(
            goal.and_then(|intent| intent.goal.as_deref()),
            expected_goal.as_deref()
        );
        assert!(client.0.state.lock().pending.is_empty());
        if silence {
            assert!(started.elapsed() < Duration::from_secs(5));
        }
    }
}

#[tokio::test]
async fn codex_rpc_control_turn_requests_cache_context_by_exact_thread_and_reply_ids() {
    let (client, mut outgoing) = client();
    for method in ["turn/start", "turn/steer"] {
        let request = client.request(method, json!({"threadId":"bound","additionalContext":{"key":{"kind":"application","value":method}}}));
        tokio::pin!(request);
        let message = tokio::select! {
            _ = &mut request => panic!("premature response"),
            message = outgoing.recv() => message.unwrap(),
        };
        let Message::Text(text) = message else {
            panic!("expected request")
        };
        let frame: Value = serde_json::from_str(&text).unwrap();
        client
            .ingest(Frame::Response {
                id: serde_json::from_value(frame["id"].clone()).unwrap(),
                result: Ok(json!({})),
            })
            .unwrap();
        request.await.unwrap();
        assert_eq!(client.additional_context("bound")["key"]["value"], method);
        assert!(client.additional_context("foreign").is_empty());
    }
    client
        .reply(
            &RequestId::String("server-id".into()),
            Ok(json!({"decision":"decline"})),
        )
        .unwrap();
    let Message::Text(text) = outgoing.recv().await.unwrap() else {
        panic!("expected reply")
    };
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap(),
        json!({"id":"server-id","result":{"decision":"decline"}})
    );
}

#[test]
fn codex_rpc_control_buffer_overflow_requests_resync_without_closing_transport() {
    let (client, _outgoing) = client();
    let mut resync = client.resync();
    let event = Frame::Notification {
        method: "item/started".into(),
        params: json!({"threadId":"own"}),
    };
    client.ingest(event.clone()).unwrap();
    client.ingest(event.clone()).unwrap();
    client.ingest(event).unwrap();
    assert!(resync.has_changed().unwrap());
    assert_eq!(*resync.borrow_and_update(), 1);
    assert!(client.0.state.lock().closed.is_none());
    let mut events = client.subscribe();
    client
        .ingest(Frame::ServerRequest {
            id: RequestId::Number(7),
            method: "approval".into(),
            params: json!({"threadId":"own"}),
        })
        .unwrap();
    assert!(matches!(
        events.try_recv().unwrap(),
        Frame::ServerRequest { .. }
    ));
}

#[test]
fn codex_rpc_control_retired_socket_cannot_close_new_connection() {
    let (client, _outgoing) = client();
    client.0.epoch.store(2, Ordering::Release);
    client.close_epoch(1, "retired");
    assert!(client.0.state.lock().closed.is_none());
    client.close_epoch(2, "current");
    assert!(client.0.state.lock().closed.is_some());
}

#[tokio::test]
async fn codex_rpc_pending_steer_interrupt_drops_server_buffer_without_replay() {
    use crate::service::{
        chat_send_transaction::{ChatSendLedger, ChatSendState, SendLiveBinding},
        codex_rpc::{
            control::{interrupt_turns, ControlState},
            send::{start_params, submit, Origin},
        },
    };
    let (client, mut outgoing) = client();
    let ledger = ChatSendLedger::default();
    let sent = ledger.try_start("tab-a", "pending input", None).unwrap();
    assert!(ledger.bind_rpc_send(
        &sent.send_id,
        SendLiveBinding {
            session_id: Some("thread-a".into()),
            binding_nonce: Some("nonce-a".into()),
        }
    ));
    let mut control = ControlState::default();
    control.started("thread-a", "active-a");
    let consumer = async {
        let reply = submit(
            Origin::User,
            Some("active-a"),
            start_params("thread-a", &sent.send_id, "pending input", None, json!({})),
            |method, params| client.request(method, params),
        )
        .await;
        assert_eq!(reply.unwrap(), json!({"turnId":"active-a"}));
        ledger.mark_rpc_accepted(&sent.send_id);
        interrupt_turns(&control.descendants("thread-a"), |method, params| {
            client.request(method, params)
        })
        .await
        .unwrap();
        assert!(control.completed("thread-a", "active-a"));
        assert_eq!(
            ledger.status(&sent.send_id).unwrap().state,
            ChatSendState::Written
        );
        assert!(!ledger.bind_rpc_send(&sent.send_id, SendLiveBinding::default()));
        let reply = submit(
            Origin::User,
            None,
            start_params("thread-a", "new-id", "fresh input", None, json!({})),
            |method, params| client.request(method, params),
        )
        .await;
        assert!(reply.is_ok());
        assert_eq!(
            ledger.status(&sent.send_id).unwrap().state,
            ChatSendState::Written
        );
    };
    let server = async {
        let mut pending = Vec::new();
        for expected in ["turn/steer", "turn/interrupt", "turn/start"] {
            let Message::Text(text) = outgoing.recv().await.unwrap() else {
                panic!("expected request")
            };
            let frame: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(frame["method"], expected);
            let result = match expected {
                "turn/steer" => {
                    assert_eq!(frame["params"]["expectedTurnId"], "active-a");
                    pending.push(frame["params"]["input"].clone());
                    json!({"turnId":"active-a"})
                }
                "turn/interrupt" => {
                    assert_eq!(
                        frame["params"],
                        json!({"threadId":"thread-a", "turnId":"active-a"})
                    );
                    assert_eq!(pending.len(), 1);
                    pending.clear();
                    json!({})
                }
                _ => {
                    assert!(pending.is_empty());
                    assert_eq!(
                        frame["params"]["input"],
                        json!([{"type":"text","text":"fresh input"}])
                    );
                    assert_eq!(frame["params"]["clientUserMessageId"], "new-id");
                    json!({"turn":{"id":"next-a"}})
                }
            };
            client
                .ingest(Frame::Response {
                    id: serde_json::from_value(frame["id"].clone()).unwrap(),
                    result: Ok(result),
                })
                .unwrap();
        }
        assert!(outgoing.try_recv().is_err());
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(consumer, server);
    })
    .await
    .unwrap();
}
