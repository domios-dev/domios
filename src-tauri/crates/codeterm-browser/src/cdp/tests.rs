use super::*;
use tokio::net::TcpListener;

struct FakeChrome {
    endpoint: String,
    inbound: mpsc::UnboundedReceiver<Value>,
    outbound: mpsc::UnboundedSender<Message>,
}

#[tokio::test]
async fn browser_cdp_completion_observation_overflow_reports_lost_authority() {
    let (connection, _receiver) = batch_connection();
    let stream = connection.subscribe(EventFilter::any());
    for _ in 0..=SUBSCRIBER_CAPACITY {
        connection.0.fan_out(Arc::new(CdpEvent {
            method: "Page.lifecycleEvent".into(),
            params: json!({}),
            session_id: None,
        }));
    }
    assert!(stream.has_lost_events());
    assert!(!stream.is_closed());
}

#[tokio::test]
async fn browser_cdp_completion_cancellation_releases_pending_command_receipts() {
    let (connection, _receiver) = batch_connection();
    {
        let action = connection.send_to(
            Some("session"),
            "Page.navigate",
            json!({"url":"https://page/"}),
            COMMAND_DEADLINE,
        );
        tokio::pin!(action);
        assert!(futures_util::poll!(&mut action).is_pending());
        assert_eq!(connection.0.state.lock().pending.len(), 1);
    }
    assert!(connection.0.state.lock().pending.is_empty());
}

impl FakeChrome {
    fn protocol_event(&self, method: &str, params: Value, session_id: &str) {
        self.outbound
            .send(Message::text(
                json!({
                    "method":method, "params":params, "sessionId":session_id,
                })
                .to_string(),
            ))
            .unwrap();
    }

    fn reply(&self, request: &Value, body: Value) {
        let mut message = body;
        message["id"] = request["id"].clone();
        self.outbound
            .send(Message::text(message.to_string()))
            .unwrap();
    }

    fn event(&self, method: &str, session_id: Option<&str>) {
        let mut message = json!({ "method": method, "params": { "probe": method } });
        if let Some(session_id) = session_id {
            message["sessionId"] = json!(session_id);
        }
        self.outbound
            .send(Message::text(message.to_string()))
            .unwrap();
    }
}

#[tokio::test]
async fn browser_cdp_completion_correlates_withheld_navigation_response_and_load() {
    let mut chrome = fake_chrome().await;
    let client = CdpClient::connect(&chrome.endpoint).await.unwrap();
    let page = PageChannel {
        client,
        session_id: "session".into(),
        target_id: "target".into(),
    };
    let observing = tokio::spawn({
        let page = page.clone();
        async move { crate::completion::ActionCompletion::observe(&page).await }
    });
    for method in [
        "Page.enable",
        "Page.setLifecycleEventsEnabled",
        "Page.getFrameTree",
    ] {
        let request = chrome.inbound.recv().await.unwrap();
        assert_eq!(request["method"], method);
        let result = if method == "Page.getFrameTree" {
            json!({"frameTree":{"frame":{"id":"main","loaderId":"before"}}})
        } else {
            json!({})
        };
        chrome.reply(&request, json!({"result":result}));
    }
    let observing = observing.await.unwrap().unwrap();
    let mut barrier = page.subscribe_session(&["Fixture.barrier"]);
    let action = observing.finish(
        async {
            page.send("Page.navigate", json!({"url":"https://delayed/"}))
                .await
                .map_err(|error| error.to_string())
        },
        true,
        Duration::from_secs(15),
    );
    tokio::pin!(action);
    let request = tokio::select! {
        result = &mut action => panic!("completed before navigation request: {result:?}"),
        request = chrome.inbound.recv() => request.unwrap(),
    };
    assert_eq!(request["method"], "Page.navigate");
    assert_eq!(request["sessionId"], "session");
    chrome.protocol_event(
        "Page.frameRequestedNavigation",
        json!({"frameId":"main"}),
        "session",
    );
    chrome.protocol_event("Fixture.barrier", json!({}), "session");
    barrier.recv().await.unwrap();
    assert!(futures_util::poll!(&mut action).is_pending());
    chrome.reply(
        &request,
        json!({"result":{"frameId":"main","loaderId":"own"}}),
    );
    chrome.protocol_event(
        "Page.frameNavigated",
        json!({"frame":{"id":"main","loaderId":"own","url":"https://delayed/"}}),
        "session",
    );
    chrome.protocol_event(
        "Page.lifecycleEvent",
        json!({"frameId":"child","loaderId":"own","name":"load"}),
        "session",
    );
    chrome.protocol_event("Fixture.barrier", json!({}), "session");
    barrier.recv().await.unwrap();
    assert!(futures_util::poll!(&mut action).is_pending());
    chrome.protocol_event(
        "Page.lifecycleEvent",
        json!({"frameId":"main","loaderId":"own","name":"load"}),
        "session",
    );
    assert_eq!(
        action.await.unwrap().1,
        "navigation loaded: https://delayed/"
    );
}

async fn fake_chrome() -> FakeChrome {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!(
        "ws://{}/devtools/browser/fake",
        listener.local_addr().unwrap()
    );
    let (inbound_tx, inbound) = mpsc::unbounded_channel();
    let (outbound, mut outbound_rx) = mpsc::unbounded_channel::<Message>();
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let socket = tokio_tungstenite::accept_async(stream).await.unwrap();
        let (mut write, mut read) = socket.split();
        loop {
            tokio::select! {
                incoming = read.next() => match incoming {
                    Some(Ok(Message::Text(text))) => {
                        let _ = inbound_tx.send(serde_json::from_str::<Value>(&text).unwrap());
                    }
                    Some(Ok(_)) => {}
                    _ => break,
                },
                outgoing = outbound_rx.recv() => match outgoing {
                    Some(message) => {
                        if write.send(message).await.is_err() {
                            break;
                        }
                    }
                    None => {
                        let _ = write.close().await;
                        break;
                    }
                },
            }
        }
    });
    FakeChrome {
        endpoint,
        inbound,
        outbound,
    }
}

#[tokio::test]
async fn browser_cdp_client_correlates_out_of_order_replies() {
    let mut chrome = fake_chrome().await;
    let client = CdpClient::connect(&chrome.endpoint).await.unwrap();
    let browser = tokio::spawn({
        let client = client.clone();
        async move { client.send("Browser.getVersion", json!({})).await }
    });
    let page = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .send_to(
                    Some("S1"),
                    "Page.navigate",
                    json!({"url": "about:blank"}),
                    COMMAND_DEADLINE,
                )
                .await
        }
    });
    let first = chrome.inbound.recv().await.unwrap();
    let second = chrome.inbound.recv().await.unwrap();
    for request in [&second, &first] {
        chrome.reply(request, json!({ "result": { "echo": request["method"] } }));
    }
    assert_eq!(
        browser.await.unwrap().unwrap()["echo"],
        "Browser.getVersion"
    );
    assert_eq!(page.await.unwrap().unwrap()["echo"], "Page.navigate");
    let navigate = [&first, &second]
        .into_iter()
        .find(|request| request["method"] == "Page.navigate")
        .unwrap();
    assert_eq!(navigate["sessionId"], "S1");
    assert_ne!(first["id"], second["id"]);
}

#[tokio::test]
async fn browser_cdp_client_maps_protocol_errors() {
    let mut chrome = fake_chrome().await;
    let client = CdpClient::connect(&chrome.endpoint).await.unwrap();
    let call = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .send("DOM.getBoxModel", json!({"backendNodeId": 9}))
                .await
        }
    });
    let request = chrome.inbound.recv().await.unwrap();
    chrome.reply(
        &request,
        json!({ "error": { "code": -32000, "message": "Could not compute box model." } }),
    );
    let Err(CdpError::Protocol(error)) = call.await.unwrap() else {
        panic!("expected a protocol error");
    };
    assert_eq!(error.code, -32000);
}

#[tokio::test]
async fn browser_cdp_page_channel_receives_only_its_session_events() {
    let mut chrome = fake_chrome().await;
    let client = CdpClient::connect(&chrome.endpoint).await.unwrap();
    let attach = tokio::spawn({
        let client = client.clone();
        async move { client.attach_page("T1").await }
    });
    let request = chrome.inbound.recv().await.unwrap();
    assert_eq!(request["method"], "Target.attachToTarget");
    assert_eq!(request["params"]["flatten"], true);
    chrome.reply(&request, json!({ "result": { "sessionId": "S1" } }));
    let page = attach.await.unwrap().unwrap();
    assert_eq!(page.session_id(), "S1");
    let mut loads = page.subscribe_session(&["Page.loadEventFired"]);
    let mut all = page.subscribe(&[]);
    chrome.event("Page.loadEventFired", Some("S2"));
    chrome.event("Page.frameNavigated", Some("S1"));
    chrome.event("Target.targetCreated", None);
    chrome.event("Page.loadEventFired", Some("S1"));
    let load = loads.recv().await.unwrap();
    assert_eq!(load.session_id.as_deref(), Some("S1"));
    assert_eq!(load.method, "Page.loadEventFired");
    let seen: Vec<String> = [
        all.recv().await.unwrap(),
        all.recv().await.unwrap(),
        all.recv().await.unwrap(),
    ]
    .iter()
    .map(|event| event.method.clone())
    .collect();
    assert_eq!(
        seen,
        [
            "Page.frameNavigated",
            "Target.targetCreated",
            "Page.loadEventFired"
        ]
    );
}

#[tokio::test]
async fn browser_cdp_socket_loss_fails_pending_and_ends_streams() {
    let mut chrome = fake_chrome().await;
    let client = CdpClient::connect(&chrome.endpoint).await.unwrap();
    let mut events = client.subscribe(EventFilter::any());
    let call = tokio::spawn({
        let client = client.clone();
        async move { client.send("Page.reload", json!({})).await }
    });
    chrome.inbound.recv().await.unwrap();
    drop(chrome);
    assert!(matches!(call.await.unwrap(), Err(CdpError::Closed(_))));
    assert!(events.recv().await.is_none());
    assert!(client.close_reason().is_some());
    assert!(matches!(
        client.send("Page.reload", json!({})).await,
        Err(CdpError::Closed(_))
    ));
}

#[tokio::test]
async fn browser_cdp_refuses_non_loopback_endpoint() {
    let Err(error) = CdpClient::connect("ws://10.1.2.3:9222/devtools/browser/x").await else {
        panic!("non-loopback endpoint must be refused");
    };
    assert!(error.contains("loopback"), "{error}");
}

#[test]
fn browser_cdp_websocket_reports_capacity_error_over_cap() {
    use std::io::Cursor;
    use tokio_tungstenite::tungstenite::{protocol::Role, WebSocket};
    let mut frame = vec![0x81, 127];
    frame.extend_from_slice(&((MAX_MESSAGE_SIZE as u64) + 1).to_be_bytes());
    let mut socket =
        WebSocket::from_raw_socket(Cursor::new(frame), Role::Client, Some(websocket_config()));
    assert!(matches!(socket.read(), Err(WsError::Capacity(_))));
}

#[tokio::test]
async fn browser_cdp_input_batch_sends_every_phase_before_any_reply() {
    let mut chrome = fake_chrome().await;
    let client = CdpClient::connect(&chrome.endpoint).await.unwrap();
    let page = PageChannel {
        client,
        session_id: "input".into(),
        target_id: "target".into(),
    };
    let sent = tokio::spawn(async move {
        page.send_batch(vec![
            ("Input.dispatchMouseEvent", json!({"type": "mousePressed"})),
            ("Input.dispatchMouseEvent", json!({"type": "mouseReleased"})),
        ])
        .await
    });
    let first = tokio::time::timeout(Duration::from_secs(2), chrome.inbound.recv())
        .await
        .unwrap()
        .unwrap();
    let second = tokio::time::timeout(Duration::from_secs(2), chrome.inbound.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first["params"]["type"], "mousePressed");
    assert_eq!(second["params"]["type"], "mouseReleased");
    assert_eq!(second["sessionId"], "input");
    chrome.reply(&second, json!({"result": {}}));
    chrome.reply(&first, json!({"result": {}}));
    assert_eq!(sent.await.unwrap().unwrap().len(), 2);
}

fn batch_connection() -> (SocketConnection, mpsc::Receiver<Vec<Message>>) {
    let (outgoing, receiver) = mpsc::channel(1);
    let (closed, _) = watch::channel(None);
    let connection = SocketConnection(Arc::new(Inner {
        endpoint: "ws://127.0.0.1/test".into(),
        next_id: AtomicU64::new(1),
        next_subscriber: AtomicU64::new(1),
        state: Mutex::new(State::default()),
        outgoing,
        closed,
    }));
    (connection, receiver)
}

fn click_phases() -> Vec<(String, Value)> {
    vec![
        (
            "Input.dispatchMouseEvent".into(),
            json!({ "type": "mousePressed" }),
        ),
        (
            "Input.dispatchMouseEvent".into(),
            json!({ "type": "mouseReleased" }),
        ),
    ]
}

#[tokio::test]
async fn browser_cdp_batch_on_closed_socket_returns_close_reason() {
    let (connection, mut receiver) = batch_connection();
    connection.close("browser process exited");
    assert_eq!(
        connection
            .send_batch_to(Some("page"), click_phases(), COMMAND_DEADLINE)
            .await,
        Err(CdpError::Closed("browser process exited".into()))
    );
    assert!(connection.0.state.lock().pending.is_empty());
    assert!(receiver.try_recv().is_err());
}

#[tokio::test]
async fn browser_cdp_batch_writer_loss_keeps_close_reason_for_every_pending_phase() {
    let (connection, receiver) = batch_connection();
    connection.0.outgoing.try_send(vec![]).unwrap();
    let batch = connection.send_batch_to(Some("page"), click_phases(), COMMAND_DEADLINE);
    tokio::pin!(batch);
    assert!(futures_util::poll!(&mut batch).is_pending());
    assert_eq!(connection.0.state.lock().pending.len(), 2);
    connection.close("chrome closed the CDP socket");
    drop(receiver);
    assert_eq!(
        batch.await,
        Err(CdpError::Closed("chrome closed the CDP socket".into()))
    );
    assert!(connection.0.state.lock().pending.is_empty());
}

#[tokio::test]
async fn browser_cdp_batch_dropped_phase_replies_keep_close_reason() {
    let (connection, _receiver) = batch_connection();
    let batch = connection.send_batch_to(Some("page"), click_phases(), COMMAND_DEADLINE);
    tokio::pin!(batch);
    assert!(futures_util::poll!(&mut batch).is_pending());
    {
        let mut state = connection.0.state.lock();
        assert_eq!(state.pending.len(), 2);
        state.closed = Some("CDP reader stopped".into());
        state.pending.clear();
    }
    assert_eq!(
        batch.await,
        Err(CdpError::Closed("CDP reader stopped".into()))
    );
    assert!(connection.0.state.lock().pending.is_empty());
}

#[tokio::test]
async fn browser_cdp_deep_reply_fails_its_request_without_waiting_for_deadline() {
    let mut chrome = fake_chrome().await;
    let client = CdpClient::connect(&chrome.endpoint).await.unwrap();
    let deep = client.send("Fixture.deep", json!({}));
    let ordinary = client.send("Fixture.ordinary", json!({}));
    tokio::pin!(deep, ordinary);
    let mut requests = Vec::new();
    while requests.len() != 2 {
        tokio::select! {
            result = &mut deep => panic!("deep request completed before reply: {result:?}"),
            result = &mut ordinary => panic!("ordinary request completed before reply: {result:?}"),
            request = chrome.inbound.recv() => requests.push(request.unwrap()),
        }
    }
    let deep_id = requests
        .iter()
        .find(|request| request["method"] == "Fixture.deep")
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    chrome
        .outbound
        .send(Message::text(format!(
            "{{\"result\":{}0{},\"id\":{deep_id}}}",
            "[".repeat(180),
            "]".repeat(180)
        )))
        .unwrap();
    let ordinary_request = requests
        .iter()
        .find(|request| request["method"] == "Fixture.ordinary")
        .unwrap();
    chrome.reply(ordinary_request, json!({"result":{"ok":true}}));
    let (deep_result, ordinary_result) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(deep, ordinary)
    })
    .await
    .expect("decoded replies must complete before the command deadline");
    assert!(
        matches!(deep_result, Err(CdpError::Malformed(detail)) if detail.contains("recursion limit"))
    );
    assert_eq!(ordinary_result.unwrap(), json!({"ok":true}));
    assert!(client.close_reason().is_none());
    client.close("fixture complete");
}

#[tokio::test]
async fn browser_cdp_document_decodes_deep_replies_and_bounds_failures_per_request() {
    let mut chrome = fake_chrome().await;
    let client = CdpClient::connect(&chrome.endpoint).await.unwrap();
    for depth in [400, 4097, 0] {
        let document = client.send("DOM.getDocument", json!({"depth":-1,"pierce":true}));
        let ordinary = client.send("Fixture.ordinary", json!({}));
        tokio::pin!(document, ordinary);
        let mut requests = Vec::new();
        while requests.len() < 2 {
            tokio::select! {
                result = &mut document => panic!("document completed before reply: {result:?}"),
                result = &mut ordinary => panic!("ordinary completed before reply: {result:?}"),
                request = chrome.inbound.recv() => requests.push(request.unwrap()),
            }
        }
        let id = requests
            .iter()
            .find(|request| request["method"] == "DOM.getDocument")
            .unwrap()["id"]
            .as_u64()
            .unwrap();
        let payload = if depth == 0 {
            format!("{{\"id\":{id},\"result\":broken}}")
        } else {
            format!(
                "{{\"result\":{}0{},\"id\":{id}}}",
                "[".repeat(depth),
                "]".repeat(depth)
            )
        };
        chrome.outbound.send(Message::text(payload)).unwrap();
        let request = requests
            .iter()
            .find(|request| request["method"] == "Fixture.ordinary")
            .unwrap();
        chrome.reply(request, json!({"result":{"ok":true}}));
        let (document, ordinary) = tokio::time::timeout(Duration::from_secs(2), async {
            tokio::join!(document, ordinary)
        })
        .await
        .expect("document failures must not wait for the command deadline");
        if depth == 400 {
            let mut value = document.unwrap();
            for _ in 0..depth {
                value = value.as_array_mut().unwrap().pop().unwrap();
            }
            assert_eq!(value, json!(0));
        } else if depth == 4097 {
            assert!(
                matches!(document, Err(CdpError::Malformed(detail)) if detail.contains("maximum JSON depth 4096"))
            );
        } else {
            assert!(matches!(document, Err(CdpError::Malformed(_))));
        }
        assert_eq!(ordinary.unwrap(), json!({"ok":true}));
        assert!(client.close_reason().is_none());
    }
    client.close("fixture complete");
}
