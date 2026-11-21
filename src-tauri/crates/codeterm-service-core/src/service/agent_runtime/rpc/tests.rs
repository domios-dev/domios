use super::*;
use tokio::io::AsyncBufReadExt;

fn switch_authority_fixture() -> (crate::service::test_fixture::ServiceFixture, String, Arc<crate::service::messenger::switch_handoff::SwitchHandoffAuthority>) {
    use crate::multiplexer::{state::MultiplexerState, types::TabBacking};
    use crate::service::messenger::switch_handoff::SwitchSourceAuthority;
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("target".into()); tab.agent_session_id = Some("target-session".into());
    tab.session_binding_nonce = Some("target-nonce".into()); tab.reported_session_binding_nonce = Some("target-nonce".into());
    tab.boot_stage = Some(codeterm_types::pty::BootStage::AgentReady);
    tab.backing = TabBacking::ChatBackend { plugin_id: "target".into(), session_id: "target-session".into() };
    let mut source = tab.clone(); source.agent_type = Some("source".into());
    source.agent_session_id = Some("source-session".into()); source.session_binding_nonce = Some("source-nonce".into());
    source.reported_session_binding_nonce = Some("source-nonce".into());
    source.backing = TabBacking::ChatBackend { plugin_id: "source".into(), session_id: "source-session".into() };
    let authority = SwitchSourceAuthority::admitted(&source).ready_target(tab).unwrap();
    (crate::service::test_fixture::ServiceFixture::new(mux), id, authority)
}

#[tokio::test]
async fn shutdown_admission_and_primary_claim_have_one_order_at_the_actual_writer() {
    use crate::service::messenger::switch_handoff::HandoffShutdown;
    for shutdown_first in [false, true] {
        let (fixture, _id, authority) = switch_authority_fixture();
        let admission = Arc::new(crate::crash::shutdown::ShutdownAdmission::default());
        assert!(fixture.service.set_ext(HandoffShutdown(admission.clone())).is_ok());
        authority.validate(&fixture.service).unwrap();
        let (read, _peer) = tokio::io::duplex(4096);
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let client = Client::attach(read, RecordingWriter(bytes.clone()));
        let owner = fixture.service.clone(); let witness = authority.clone(); let gate = admission.clone();
        let guard: PrewriteGuard = Arc::new(move || {
            if shutdown_first { gate.close(); }
            witness.claim(&owner)?;
            if !shutdown_first { gate.close(); }
            Ok(())
        });
        let call = RpcCall { method: "fixture/submit".into(), params: json!({"input":"opaque"}), target: None };
        let result = client.notify_guarded(&call, Some(guard)).await;
        assert!(authority.validate(&fixture.service).is_err());
        assert!(!authority.claim_submit_key(&fixture.service));
        if shutdown_first {
            assert!(matches!(result, Err(RuntimeFailure::PrewriteRefused(_))));
            assert!(!authority.was_claimed()); assert!(bytes.lock().is_empty());
        } else {
            result.unwrap(); assert!(authority.was_claimed());
            let owner = fixture.service.clone(); let witness = authority.clone();
            assert!(matches!(client.notify_guarded(&call, Some(Arc::new(move || witness.claim(&owner)))).await, Err(RuntimeFailure::WrittenAwaitingEvidence)));
            assert_eq!(String::from_utf8(bytes.lock().clone()).unwrap().lines().count(), 1);
        }
    }
}

#[tokio::test]
async fn claimed_handoff_key_continuation_obeys_shutdown_at_the_serialized_pty_writer() {
    use crate::service::messenger::switch_handoff::HandoffShutdown;
    struct KeyWriter(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for KeyWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> { self.0.lock().extend_from_slice(bytes); Ok(bytes.len()) }
        fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
    }
    for shutdown_first in [false, true] {
        let (fixture, _id, authority) = switch_authority_fixture();
        let admission = Arc::new(crate::crash::shutdown::ShutdownAdmission::default());
        assert!(fixture.service.set_ext(HandoffShutdown(admission.clone())).is_ok());
        let (read, _peer) = tokio::io::duplex(4096); let primary = Arc::new(Mutex::new(Vec::new()));
        let client = Client::attach(read, RecordingWriter(primary.clone()));
        let owner = fixture.service.clone(); let witness = authority.clone();
        client.notify_guarded(&RpcCall { method: "fixture/submit".into(), params: json!({"input":"opaque"}), target: None },
            Some(Arc::new(move || witness.claim(&owner)))).await.unwrap();
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let queue = crate::pty::write_queue::WriteQueue::new(Box::new(KeyWriter(bytes.clone())));
        let gate = crate::pty::write_queue::GuardedInputGate::default(); gate.rearm();
        let owner = fixture.service.clone(); let witness = authority.clone();
        let key = gate.issue().unwrap().with_submit_key_claim(b"\r", Arc::new(move || {
            if shutdown_first { admission.close(); }
            let admitted = witness.claim_submit_key(&owner);
            if !shutdown_first { admission.close(); }
            admitted
        })).for_submit_key(b"\r");
        let result = queue.send_and_wait_guarded(b"\r".to_vec(), key.clone(), Duration::from_secs(1));
        assert_eq!(result.is_ok(), !shutdown_first);
        assert_eq!(bytes.lock().as_slice(), if shutdown_first { &b""[..] } else { &b"\r"[..] });
        assert!(queue.send_and_wait_guarded(b"primary again".to_vec(), key, Duration::from_secs(1)).is_err());
        assert_eq!(String::from_utf8(primary.lock().clone()).unwrap().lines().count(), 1);
        assert!(authority.was_claimed());
    }
}

#[tokio::test]
async fn switch_authority_is_rechecked_at_request_turn_and_notification_write_claim() {
    for changed in ["team", "parent", "report", "account", "namespace", "nonce", "session", "frozen", "cancel"] {
        for door in ["request", "turn", "notification"] {
            let (fixture, id, authority) = switch_authority_fixture();
            let (read, peer) = tokio::io::duplex(4096);
            let bytes = Arc::new(Mutex::new(Vec::new()));
            let client = Client::attach(read, RecordingWriter(bytes.clone()));
            let owner = fixture.service.clone();
            let witness = authority.clone();
            let guard: PrewriteGuard = Arc::new(move || {
                {
                    let mut mux = owner.mux.write(); let current = mux.get_tab_mut(&id).unwrap();
                    match changed {
                        "team" => current.team_id = Some("replacement-team".into()),
                        "parent" => current.parent_tab_id = Some("replacement-parent".into()),
                        "report" => current.report_target_tab_id = Some("replacement-report".into()),
                        "account" => current.restorable_launch.account_id = Some("replacement-account".into()),
                        "namespace" => current.restorable_launch.account_namespace = Some("replacement-namespace".into()),
                        "nonce" => current.session_binding_nonce = Some("replacement-nonce".into()),
                        "session" => current.agent_session_id = Some("replacement-session".into()),
                        "cancel" => witness.retire(),
                        "frozen" => current.frozen = true,
                        _ => unreachable!(),
                    }
                }
                witness.claim(&owner)
            });
            let call = RpcCall { method: "fixture/submit".into(), params: json!({"input":"opaque"}), target: None };
            let result = match door {
                "request" => client.request_guarded(&call, Some(guard)).await.map(|_| ()),
                "turn" => client.request_turn(&call, Some(guard), Arc::new(WriteWitness::default())).await.map(|_| ()),
                _ => client.notify_guarded(&call, Some(guard)).await,
            };
            assert!(matches!(result, Err(RuntimeFailure::PrewriteRefused(_))), "{changed}/{door}");
            assert!(!authority.was_claimed());
            assert!(bytes.lock().is_empty()); assert!(client.0.state.lock().pending.is_empty());
            drop(peer);
        }
    }
}

#[tokio::test]
async fn switch_claim_winning_order_conserves_one_frame_and_later_revocation_stays_uncertain() {
    let (fixture, id, authority) = switch_authority_fixture();
    let (read, peer) = tokio::io::duplex(4096);
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let client = Client::attach(read, RecordingWriter(bytes.clone()));
    let owner = fixture.service.clone(); let witness = authority.clone();
    let guard: PrewriteGuard = Arc::new(move || {
        witness.claim(&owner)?;
        owner.mux.write().get_tab_mut(&id).unwrap().team_id = Some("replacement-team".into());
        Ok(())
    });
    let call = RpcCall { method: "fixture/submit".into(), params: json!({"input":"opaque"}), target: None };
    client.notify_guarded(&call, Some(guard)).await.unwrap();
    authority.retire();
    let owner = fixture.service.clone(); let witness = authority.clone();
    let guard: PrewriteGuard = Arc::new(move || witness.claim(&owner));
    assert!(matches!(client.notify_guarded(&call, Some(guard)).await, Err(RuntimeFailure::WrittenAwaitingEvidence)));
    assert!(authority.was_claimed());
    let written = String::from_utf8(bytes.lock().clone()).unwrap();
    assert_eq!(written.lines().count(), 1); assert_eq!(written.matches("opaque").count(), 1);
    drop(peer);
}
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
    let Some(Inbound::Text(incomplete)) = read_frame(&mut reader).await.unwrap() else {
        panic!("expected an incomplete text frame");
    };
    assert!(decode(&incomplete).is_err());
}

#[tokio::test]
async fn a_complete_final_frame_without_newline_is_processed_before_eof() {
    let mut reader = BufReader::new(&b"{\"id\":1,\"result\":{}}"[..]);
    let Some(Inbound::Text(last)) = read_frame(&mut reader).await.unwrap() else {
        panic!("expected a text frame");
    };
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
        matches!(events.try_recv().unwrap().frame, Some(RuntimeFrame::Notification { method, .. }) if method == "fixture/started")
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
    assert_eq!(
        read_frame(&mut reader).await.unwrap(),
        Some(Inbound::Text(history))
    );
}

#[tokio::test]
async fn oversized_response_fails_only_its_request_and_keeps_stdio_usable() {
    let (host, peer) = tokio::io::duplex(64 * 1024);
    let (read, write) = tokio::io::split(host);
    let client = Client::attach(read, write);
    let ended = client.ended();
    let server = async {
        let mut peer = BufReader::new(peer);
        let mut requests = HashMap::new();
        for _ in 0..2 {
            let mut line = String::new();
            peer.read_line(&mut line).await.unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            requests.insert(
                request["method"].as_str().unwrap().to_owned(),
                request["id"].clone(),
            );
        }
        let image_chunk = [b'A'; 64 * 1024];
        for (header, trailer) in [
            (
                r#"{"params":{"data":"data:image/png;base64,"#,
                format!(
                    r#""}},"id":{},"method":"fixture/prompt"}}"#,
                    requests["fixture/other"]
                ),
            ),
            (
                r#"{"result":{"data":"data:image/png;base64,"#,
                format!(
                    r#""}},"\u0069d":{},"jsonrpc":"2.0"}}"#,
                    requests["fixture/history"]
                ),
            ),
        ] {
            peer.write_all(header.as_bytes()).await.unwrap();
            for _ in 0..INBOUND_FRAME_LIMIT / image_chunk.len() {
                peer.write_all(&image_chunk).await.unwrap();
            }
            peer.write_all(trailer.as_bytes()).await.unwrap();
            peer.write_all(b"\n").await.unwrap();
        }
        let response = json!({"id":requests["fixture/other"],"result":"other"});
        peer.write_all(format!("{response}\n").as_bytes())
            .await
            .unwrap();
        let mut line = String::new();
        peer.read_line(&mut line).await.unwrap();
        let next: Value = serde_json::from_str(&line).unwrap();
        let response = json!({"id":next["id"],"result":"next"});
        peer.write_all(format!("{response}\n").as_bytes())
            .await
            .unwrap();
        peer
    };
    let request = |method: &str| RpcCall {
        method: method.into(),
        params: Value::Null,
        target: None,
    };
    let requests = async {
        let history = request("fixture/history");
        let other = request("fixture/other");
        let (oversized, unaffected) = tokio::join!(
            client.request_guarded(&history, None),
            client.request_guarded(&other, None),
        );
        let error = oversized.unwrap().unwrap_err();
        assert_eq!(error.code, -32010);
        assert_eq!(
            error.data.unwrap(),
            json!({"kind":"response_too_large","boundary":"stdio_frame","limitBytes":INBOUND_FRAME_LIMIT})
        );
        assert_eq!(unaffected.unwrap().unwrap(), "other");
        let next = client.request_guarded(&request("fixture/next"), None).await;
        assert_eq!(next.unwrap().unwrap(), "next");
    };
    let ((), _peer) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(requests, server)
    })
    .await
    .unwrap();
    assert!(ended.borrow().is_none());
    assert!(client.0.state.lock().pending.is_empty());
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
        let Some(RuntimeFrame::Notification { params, .. }) = frame.frame else {
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
        let Some(RuntimeFrame::Notification { method, params }) = event.frame else {
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
            matches!(first.frame,Some(RuntimeFrame::Notification{params,..}) if params["text"]=="first chunk")
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
