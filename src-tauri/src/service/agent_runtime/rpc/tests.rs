use super::*;
#[test]
fn generic_framing_rejects_ambiguous_responses_and_distinguishes_server_ids() {
    for text in [
        "[]",
        "{}",
        r#"{"id":true,"result":null}"#,
        r#"{"id":1,"error":{},"result":null}"#,
    ] {
        assert!(decode(text).is_err());
    }
    assert!(matches!(
        decode(r#"{"id":1,"method":"fixture/prompt","params":{}}"#).unwrap(),
        WireFrame::Event(RuntimeFrame::Request {
            request_id: RpcId::Number(1),
            ..
        })
    ));
    assert!(matches!(
        decode(r#"{"id":"1","result":{}}"#).unwrap(),
        WireFrame::Response {
            id: RpcId::String(_),
            ..
        }
    ));
}
#[tokio::test]
async fn pipe_eof_fails_pending_without_replaying_payload() {
    let (host, peer) = tokio::io::duplex(4096);
    let (read, write) = tokio::io::split(host);
    let client = Client::attach(read, write);
    let server = async {
        let mut peer = BufReader::new(peer);
        let mut line = String::new();
        peer.read_line(&mut line).await.unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&line).unwrap()["params"]["input"],
            "opaque-payload"
        );
        drop(peer);
    };
    let call = RpcCall {
        target: None,
        method: "fixture/submit".into(),
        params: json!({"input":"opaque-payload"}),
    };
    let submit = client.request(&call);
    let (result, ()) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(submit, server)
    })
    .await
    .unwrap();
    assert_eq!(result, Err(RuntimeFailure::PipeEof));
    assert!(client
        .request(&RpcCall {
            target: None,
            method: "fixture/submit".into(),
            params: json!({})
        })
        .await
        .is_err());
    assert!(client.0.state.lock().pending.is_empty());
}
#[tokio::test]
async fn partial_frame_is_a_typed_failure_not_a_successful_reply() {
    let mut reader = BufReader::new(&b"{\"id\":1"[..]);
    let incomplete = read_frame(&mut reader).await.unwrap().unwrap();
    assert!(decode(&incomplete).is_err());
}

#[tokio::test]
async fn a_complete_final_frame_without_newline_is_processed_before_eof() {
    let mut reader = BufReader::new(&b"{\"id\":1,\"result\":{}}"[..]);
    let last = read_frame(&mut reader).await.unwrap().unwrap();
    assert!(matches!(
        decode(&last),
        Ok(WireFrame::Response {
            id: RpcId::Number(1),
            ..
        })
    ));
    assert!(read_frame(&mut reader).await.unwrap().is_none());
}

#[tokio::test]
async fn process_start_notification_is_retained_before_subscriber_attachment() {
    let (host, mut peer) = tokio::io::duplex(4096);
    let (read, write) = tokio::io::split(host);
    let client = Client::attach(read, write);
    peer.write_all(b"{\"method\":\"fixture/started\",\"params\":{}}\n")
        .await
        .unwrap();
    drop(peer);
    let mut ended = client.ended();
    tokio::time::timeout(Duration::from_secs(5), async {
        while ended.borrow().is_none() {
            ended.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    let mut events = client.subscribe();
    assert!(
        matches!(events.try_recv().unwrap(), RuntimeFrame::Notification { method, .. } if method == "fixture/started")
    );
}

#[tokio::test]
async fn stdout_eof_fails_inflight_while_peer_stdin_remains_open() {
    let (host, peer) = tokio::io::duplex(4096);
    let (read, write) = tokio::io::split(host);
    let client = Client::attach(read, write);
    let (peer_read, mut peer_write) = tokio::io::split(peer);
    let server = async {
        let mut peer_read = BufReader::new(peer_read);
        let mut frame = String::new();
        peer_read.read_line(&mut frame).await.unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&frame).unwrap()["method"],
            "fixture/submit"
        );
        peer_write.shutdown().await.unwrap();
        peer_read
    };
    let request = client.request(&RpcCall {
        method: "fixture/submit".into(),
        params: json!({"input":"opaque"}),
        target: None,
    });
    let (result, peer_stdin) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(request, server)
    })
    .await
    .unwrap();
    assert_eq!(result, Err(RuntimeFailure::PipeEof));
    assert!(client.0.state.lock().pending.is_empty());
    drop(peer_stdin);
}
