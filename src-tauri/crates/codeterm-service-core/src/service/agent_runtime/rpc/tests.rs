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
    let submit = client.request_guarded(&call, None);
    let (result, ()) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(submit, server)
    })
    .await
    .unwrap();
    assert_eq!(result, Err(RuntimeFailure::PipeEof));
    assert!(client
        .request_guarded(
            &RpcCall {
                target: None,
                method: "fixture/submit".into(),
                params: json!({})
            },
            None
        )
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
        matches!(events.try_recv().unwrap().frame, RuntimeFrame::Notification { method, .. } if method == "fixture/started")
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
    let call = RpcCall {
        method: "fixture/submit".into(),
        params: json!({"input":"opaque"}),
        target: None,
    };
    let request = client.request_guarded(&call, None);
    let (result, peer_stdin) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(request, server)
    })
    .await
    .unwrap();
    assert_eq!(result, Err(RuntimeFailure::PipeEof));
    assert!(client.0.state.lock().pending.is_empty());
    drop(peer_stdin);
}

struct RecordingWriter(Arc<Mutex<Vec<u8>>>);
impl tokio::io::AsyncWrite for RecordingWriter {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
        bytes: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        self.0.lock().extend_from_slice(bytes);
        std::task::Poll::Ready(Ok(bytes.len()))
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
}
#[tokio::test]
async fn writer_revalidates_authority_before_any_bytes_and_reports_proven_refusal() {
    let (read, peer) = tokio::io::duplex(4096);
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let client = Client::attach(read, RecordingWriter(bytes.clone()));
    let guard: PrewriteGuard = Arc::new(|| Err(RuntimeFailure::Superseded));
    let call = RpcCall {
        method: "fixture/submit".into(),
        params: json!({"input":"opaque"}),
        target: None,
    };
    assert!(matches!(
        client.request_guarded(&call, Some(guard)).await,
        Err(RuntimeFailure::PrewriteRefused(_))
    ));
    assert!(bytes.lock().is_empty());
    assert!(client.0.state.lock().pending.is_empty());
    drop(peer);
}

#[tokio::test]
async fn multi_mib_history_frames_fit_but_transport_remains_bounded() {
    let history = format!(
        "{{\"id\":1,\"result\":\"{}\"}}\n",
        "x".repeat(12 * 1024 * 1024)
    );
    let mut reader = tokio::io::BufReader::new(history.as_bytes());
    assert_eq!(read_frame(&mut reader).await.unwrap().unwrap(), history);
    let oversized = vec![b'x'; FRAME_LIMIT + 1];
    let mut reader = tokio::io::BufReader::new(oversized.as_slice());
    assert!(matches!(
        read_frame(&mut reader).await,
        Err(RuntimeFailure::InvalidFrame(_))
    ));
}

#[tokio::test]
async fn parallel_tool_output_burst_stays_inside_the_event_queue() {
    const BURST: usize = 6000;
    let (host, mut peer) = tokio::io::duplex(65536);
    let (read, write) = tokio::io::split(host);
    let client = Client::attach(read, write);
    let mut events = client.subscribe();
    let mut ended = client.ended();
    let mut burst = String::new();
    for index in 0..BURST {
        let tool = index % 3;
        burst.push_str(&format!(
            "{{\"method\":\"fixture/delta\",\"params\":{{\"tool\":{tool},\"seq\":{index}}}}}\n"
        ));
    }
    let produce = async {
        peer.write_all(burst.as_bytes()).await.unwrap();
        drop(peer);
    };
    tokio::time::timeout(Duration::from_secs(10), async {
        let (_, changed) = tokio::join!(produce, ended.changed());
        changed.unwrap();
    })
    .await
    .unwrap();
    let mut seen = 0;
    while let Ok(frame) = events.try_recv() {
        let RuntimeFrame::Notification { params, .. } = frame.frame else {
            panic!("unexpected frame")
        };
        assert_eq!(params["seq"], seen);
        seen += 1;
    }
    assert_eq!(seen, BURST);
}

#[tokio::test]
async fn responsiveness_disabled_observer_keeps_ingress_order_and_payload() {
    let (host, mut peer) = tokio::io::duplex(4096);
    let (read, write) = tokio::io::split(host);
    let client = Client::attach(read, write);
    client.trace_binding(&codeterm_plugin::agent_runtime::RuntimeContext::default());
    let mut events = client.subscribe();
    let frames = [
        json!({"method":"item/started","params":{"threadId":"opaque","turnId":"turn","item":{"id":"item","text":"opaque-body"}}}),
        json!({"method":"item/commandExecution/outputDelta","params":{"threadId":"opaque","turnId":"turn","itemId":"item","delta":"opaque-progress"}}),
    ];
    for frame in frames {
        peer.write_all(format!("{frame}\n").as_bytes())
            .await
            .unwrap();
        let event = tokio::time::timeout(Duration::from_secs(1), events.recv())
            .await
            .unwrap()
            .unwrap();
        let RuntimeFrame::Notification { method, params } = event.frame else {
            panic!("unexpected request");
        };
        assert_eq!(method, frame["method"].as_str().unwrap());
        assert_eq!(params, frame["params"]);
    }
    client.close(RuntimeFailure::PipeEof);
}

#[test]
fn null_id_errors_preserve_agent_failure_and_reject_other_null_ids() {
    assert!(matches!(
        decode(r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"Invalid request"}}"#),
        Err(RuntimeFailure::ProtocolError { code: -32600, message }) if message == "Invalid request"
    ));
    for frame in [
        json!({"id":null,"result":null}),
        json!({"id":null,"method":"request"}),
        json!({"id":null,"error":{},"result":null}),
        json!({"id":false,"error":{"code":-32600,"message":"invalid"}}),
        json!({"id":1.5,"result":null}),
        json!({"id":[],"result":null}),
        json!({"id":{},"result":null}),
    ] {
        assert!(matches!(
            decode(&frame.to_string()),
            Err(RuntimeFailure::InvalidFrame(_))
        ));
    }
    let frame = json!({"id":null,"error":{"code":-32700,"message":"x".repeat(10000)}});
    assert!(
        matches!(decode(&frame.to_string()), Err(RuntimeFailure::ProtocolError { message, .. }) if message.len() <= 4096)
    );
}

#[tokio::test]
async fn declared_envelope_covers_requests_notifications_and_replies_without_changing_default_bytes(
) {
    for envelope in [
        None,
        Some(codeterm_plugin::agent_runtime::AgentRuntimeEnvelope::Jsonrpc2),
    ] {
        let (host, peer) = tokio::io::duplex(4096);
        let (read, write) = tokio::io::split(host);
        let client = Client::attach_with_envelope(read, write, envelope.clone());
        let call = RpcCall {
            target: None,
            method: "fixture".into(),
            params: json!({}),
        };
        let server = async {
            let mut peer = BufReader::new(peer);
            for expected in [
                json!({"id":1,"method":"fixture","params":{}}),
                json!({"method":"fixture","params":{}}),
                json!({"id":"reply","result":{}}),
                json!({"id":"reply","error":{"code":-1,"message":"denied"}}),
            ] {
                let mut line = String::new();
                peer.read_line(&mut line).await.unwrap();
                let mut expected = expected;
                if envelope.is_some() {
                    expected["jsonrpc"] = json!("2.0");
                }
                assert_eq!(line, format!("{expected}\n"));
                if expected.get("method").is_some() && expected.get("id").is_some() {
                    peer.get_mut()
                        .write_all(b"{\"id\":1,\"result\":{}}\n")
                        .await
                        .unwrap();
                }
            }
        };
        let writes = async {
            client.request_guarded(&call, None).await.unwrap().unwrap();
            client.notify(&call).await.unwrap();
            client
                .reply(&RpcId::String("reply".into()), Ok(json!({})))
                .await
                .unwrap();
            client
                .reply(
                    &RpcId::String("reply".into()),
                    Err(serde_json::from_value(json!({"code":-1,"message":"denied"})).unwrap()),
                )
                .await
                .unwrap();
        };
        tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(writes, server)
        })
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn unattributable_agent_error_closes_epoch_and_all_pending_requests() {
    let (host, peer) = tokio::io::duplex(4096);
    let (read, write) = tokio::io::split(host);
    let client = Client::attach(read, write);
    let mut ended = client.ended();
    let server = async {
        let mut peer = BufReader::new(peer);
        let mut line = String::new();
        peer.read_line(&mut line).await.unwrap();
        peer.get_mut().write_all(b"{\"jsonrpc\":\"2.0\",\"id\":null,\"error\":{\"code\":-32600,\"message\":\"Invalid request\"}}\n").await.unwrap();
        ended.changed().await.unwrap();
    };
    let call = RpcCall {
        target: None,
        method: "initialize".into(),
        params: json!({}),
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(client.request_guarded(&call, None), server)
    })
    .await
    .unwrap();
    let failure = RuntimeFailure::ProtocolError {
        code: -32600,
        message: "Invalid request".into(),
    };
    assert_eq!(result, Err(failure.clone()));
    assert_eq!(*client.ended().borrow(), Some(failure));
    assert!(client.0.state.lock().pending.is_empty());
}

#[tokio::test]
async fn undeclared_request_keeps_its_thirty_second_deadline() {
    let (host, peer) = tokio::io::duplex(4096);
    let (read, write) = tokio::io::split(host);
    let client = Client::attach(read, write);
    let call = RpcCall {
        target: None,
        method: "opaque/submit".into(),
        params: json!({}),
    };
    let request = client.request_guarded(&call, None);
    tokio::pin!(request);
    let ready = async {
        let mut peer = BufReader::new(peer);
        let mut line = String::new();
        peer.read_line(&mut line).await.unwrap();
        client
            .notify(&RpcCall {
                target: None,
                method: "opaque/barrier".into(),
                params: json!({}),
            })
            .await
            .unwrap();
        peer
    };
    let _peer = tokio::select! {
        biased;
        result = &mut request => panic!("request completed before its reply or deadline: {result:?}"),
        peer = ready => peer,
    };
    tokio::time::pause();
    let (reply, ()) = tokio::join!(biased;
        &mut request,
        async { tokio::time::advance(Duration::from_secs(31)).await; },
    );
    assert_eq!(reply, Err(RuntimeFailure::Deadline));
    assert!(client.0.state.lock().pending.is_empty());
}

#[tokio::test]
async fn first_ingress_during_write_keeps_the_prewrite_sequence_boundary() {
    use std::{
        io,
        pin::Pin,
        task::{Context, Poll},
    };
    struct FlushGate<W> {
        writer: W,
        release: oneshot::Receiver<()>,
    }
    impl<W: AsyncWrite + Unpin> AsyncWrite for FlushGate<W> {
        fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            bytes: &[u8],
        ) -> Poll<io::Result<usize>> {
            Pin::new(&mut self.writer).poll_write(cx, bytes)
        }
        fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            match Pin::new(&mut self.release).poll(cx) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(_) => Pin::new(&mut self.writer).poll_flush(cx),
            }
        }
        fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Pin::new(&mut self.writer).poll_shutdown(cx)
        }
    }
    use std::future::Future;
    let (host, peer) = tokio::io::duplex(4096);
    let (read, write) = tokio::io::split(host);
    let (release, flushing) = oneshot::channel();
    let client = Client::attach(
        read,
        FlushGate {
            writer: write,
            release: flushing,
        },
    );
    let mut events = client.subscribe();
    let witness = Arc::new(WriteWitness::default());
    let call = RpcCall {
        target: None,
        method: "fixture/submit".into(),
        params: json!({}),
    };
    let request = client.request_turn(&call, None, witness.clone());
    let server = async {
        let mut peer = BufReader::new(peer);
        let mut line = String::new();
        peer.read_line(&mut line).await.unwrap();
        let request: Value = serde_json::from_str(&line).unwrap();
        peer.get_mut()
            .write_all(b"{\"method\":\"fixture/first\",\"params\":{\"text\":\"first chunk\"}}\n")
            .await
            .unwrap();
        let first = events.recv().await.unwrap();
        assert_eq!(first.sequence, 1);
        assert_eq!(witness.after_sequence(), Some(0));
        assert!(!witness.written());
        assert!(
            matches!(first.frame,RuntimeFrame::Notification{params,..} if params["text"]=="first chunk")
        );
        release.send(()).unwrap();
        witness.wait().await.unwrap();
        witness.finish(Err(RuntimeFailure::Deadline));
        assert!(
            witness.written(),
            "a late failure cannot erase the write receipt"
        );
        let reply = json!({"id":request["id"],"result":{}});
        peer.get_mut()
            .write_all(format!("{reply}\n").as_bytes())
            .await
            .unwrap();
        peer
    };
    let (result, _peer) = tokio::join!(request, server);
    assert!(result.is_ok());
}

#[test]
fn host_mcp_credential_echo_is_redacted_before_reply_or_event_publication() {
    let placeholder = "<credential-placeholder>";
    for value in [
        json!({"jsonrpc":"2.0","id":1,"error":{"code":-32603,"message":placeholder,"data":{"env":[{"name":"CODETERM_API_TOKEN","value":placeholder}]}}}),
        json!({"jsonrpc":"2.0","method":"session/update","params":{"text":placeholder}}),
        json!({"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":placeholder}}),
    ] {
        let result = decode_redacted(&value.to_string(), placeholder);
        let text = match result {
            Ok(WireFrame::Response { result, .. }) => match result {
                Ok(value) => value.to_string(),
                Err(error) => serde_json::to_string(&error).unwrap(),
            },
            Ok(WireFrame::Event(frame)) => serde_json::to_string(&frame).unwrap(),
            Err(error) => error.to_string(),
        };
        assert!(!text.contains(placeholder));
    }
}
