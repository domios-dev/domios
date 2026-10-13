use super::*;
use tokio::net::TcpListener;

struct FakeChrome {
    endpoint: String,
    inbound: mpsc::UnboundedReceiver<Value>,
    outbound: mpsc::UnboundedSender<Message>,
}

impl FakeChrome {
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
