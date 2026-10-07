use super::*;

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
            json!({"data":[{"turnId":"old","item":{"id":"answer-id","type":"agentMessage","text":"opaque-answer"}}]}),
        ),
        (
            "thread/items/list",
            None,
            Some("active"),
            json!({"data":[]}),
        ),
        (
            "thread/items/list",
            None,
            Some("queued"),
            json!({"data":[]}),
        ),
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
        ["user-id", "answer-id"]
    );
    assert_eq!(history.active_turn.as_deref(), Some("active"));
    assert_eq!(history.completed_turns, ["old"]);
    assert!(outgoing.try_recv().is_err());
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
