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
            }),
            next_id: AtomicI64::new(1),
            initialized: AtomicBool::new(false),
            outgoing,
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
async fn codex_rpc_client_forbids_turns_and_global_config_writes() {
    let (client, mut outgoing) = client();
    for method in [
        "turn/start",
        "turn/steer",
        "config/batchWrite",
        "config/value/write",
    ] {
        assert!(client.request(method, json!({})).await.is_err());
    }
    assert!(outgoing.try_recv().is_err());
}
