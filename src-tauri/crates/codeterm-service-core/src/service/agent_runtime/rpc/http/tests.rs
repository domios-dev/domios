use super::*;
use tokio::io::AsyncBufReadExt;

mod response_limits;
mod restart;

#[test]
fn http_routes_keep_origin_and_refuse_traversal() {
    let origin = Url::parse("http://127.0.0.1:12345/").unwrap();
    let params = json!({"path":{"session":"ses_a?foreign#fragment"},"query":{"limit":"20"}});
    let url = route_url(&origin, "/session/{session}/message", &params).unwrap();
    assert_eq!(url.origin(), origin.origin());
    assert_eq!(url.path(), "/session/ses_a%3Fforeign%23fragment/message");
    assert_eq!(url.query(), Some("limit=20"));
    for session in [
        "..",
        ".",
        "foreign/session",
        "foreign\\session",
        "foreign\n",
    ] {
        assert!(route_url(
            &origin,
            "/session/{session}/message",
            &json!({"path":{"session":session}})
        )
        .is_err());
    }
    assert!(route_url(&origin, "/session/{session}", &Value::Null).is_err());
}

#[test]
fn sse_decodes_split_utf8_crlf_comments_and_multiline_data() {
    let input = ":heartbeat\r\ndata: {\"text\":\r\ndata: \"雪\"}\r\n\r\ndata: {\"next\":true}\n\n";
    let mut decoder = sse::Decoder::default();
    let mut frames = Vec::new();
    for byte in input.bytes() {
        if let Some(data) = decoder.push(byte).unwrap() {
            frames.push(sse::parse_event(&data).unwrap());
        }
    }
    assert_eq!(frames, vec![json!({"text":"雪"}), json!({"next":true})]);
}

#[test]
fn sse_fails_closed_on_malformed_or_oversized_events() {
    let data = decode_sse(b"data: {bad}\n\n").unwrap();
    assert!(sse::parse_event(&data[0]).is_err());
    let mut oversized =
        br#"data: {"id":"evt_owned","type":"message.part.updated","properties":{"part":{"text":""#
            .to_vec();
    oversized.resize(sse::EVENT_LIMIT + 1, b'x');
    let failure = decode_sse(&oversized).unwrap_err();
    assert_eq!(
        failure,
        RuntimeFailure::SseEventTooLarge {
            event_type: "message.part.updated".into(),
            limit_bytes: sse::EVENT_LIMIT,
        }
    );
    assert_eq!(
        failure.to_string(),
        "Runtime SSE event 'message.part.updated' exceeded the 4194304-byte limit"
    );
    let mut decoder = sse::Decoder::default();
    for byte in b"data: {\"partial\":true}" {
        assert!(decoder.push(*byte).unwrap().is_none());
    }
}

#[test]
fn sse_keeps_large_echo_and_later_events_after_oversized_comment() {
    let text = "λ\n".repeat(110_000);
    let echo = json!({"type":"message.part.updated","properties":{"part":{"text":text}}});
    let idle = json!({"type":"session.idle","properties":{"sessionID":"ses_owned"}});
    let echo_line = format!("data: {echo}\n\n");
    assert!(echo_line.len() > 256 * 1024);
    let mut input = vec![b'x'; sse::EVENT_LIMIT + 1];
    input[0] = b':';
    input.extend_from_slice(format!("\r\n\r\n{echo_line}data: {idle}\n\n").as_bytes());

    let frames = decode_sse(&input)
        .unwrap()
        .into_iter()
        .map(|data| sse::parse_event(&data).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(frames, [echo, idle]);
}

fn decode_sse(input: &[u8]) -> Result<Vec<Vec<u8>>, RuntimeFailure> {
    let mut decoder = sse::Decoder::default();
    input
        .iter()
        .filter_map(|&byte| decoder.push(byte).transpose())
        .collect()
}

#[tokio::test]
async fn http_event_stream_reconnects_once_and_marks_the_history_gap() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
    let endpoint = origin.join("event").unwrap();
    let launch = Launch {
        args: vec![],
        password: "fixture-password".into(),
        declaration: serde_json::from_value(json!({
            "readinessPrefix":"fixture listening on ", "passwordEnv":"FIXTURE_PASSWORD",
            "username":"fixture", "usernameEnv":"FIXTURE_USERNAME",
            "eventPath":"/event", "eventMethod":"fixture/event", "routes":{}
        }))
        .unwrap(),
        url: origin,
    };
    let (client, _writes) = Client::channel(None, 16, 16);
    let mut events = client.subscribe();
    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    let fixture = async {
        for value in ["first", "after-gap"] {
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = BufReader::new(socket);
            let mut request = String::new();
            socket.read_line(&mut request).await.unwrap();
            assert_eq!(request, "GET /event HTTP/1.1\r\n");
            loop {
                let mut header = String::new();
                assert_ne!(socket.read_line(&mut header).await.unwrap(), 0);
                if header == "\r\n" {
                    break;
                }
            }
            let body = format!("data: {}\n\n", json!({"value":value}));
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            socket
                .get_mut()
                .write_all(response.as_bytes())
                .await
                .unwrap();
        }
    };
    let read = async {
        let initial = connect_events(&http, &endpoint, &launch).await.unwrap();
        assert!(matches!(
            stream_events(
                initial,
                &client,
                &http,
                &endpoint,
                &launch,
                &Mutex::new(requests::Requests::default())
            )
            .await,
            Err(RuntimeFailure::PipeFailure { .. })
        ));
        let first = events.try_recv().unwrap();
        let gap = events.try_recv().unwrap();
        let resumed = events.try_recv().unwrap();
        assert_eq!([first.sequence, gap.sequence, resumed.sequence], [1, 2, 3]);
        assert!(
            matches!(first.frame, Some(RuntimeFrame::Notification { params, .. }) if params["value"] == "first")
        );
        assert!(gap.frame.is_none());
        assert!(
            matches!(resumed.frame, Some(RuntimeFrame::Notification { params, .. }) if params["value"] == "after-gap")
        );
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(fixture, read);
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn http_readiness_requires_exact_owned_origin_and_stdout_signal() {
    let declaration = serde_json::from_value(json!({"readinessPrefix":"fixture listening on ","passwordEnv":"FIXTURE_PASSWORD","username":"fixture","usernameEnv":"FIXTURE_USERNAME","eventPath":"/event","eventMethod":"fixture/event","routes":{}})).unwrap();
    let launch = Launch {
        args: vec![],
        password: "opaque".into(),
        declaration,
        url: Url::parse("http://127.0.0.1:12345/").unwrap(),
    };
    let output = b"fixture listening on http://127.0.0.1:54321\nfixture listening on http://localhost:12345\nfixture listening on http://127.0.0.1:12345\n";
    readiness(&mut &output[..], &launch).await.unwrap();
    assert!(readiness(
        &mut &b"fixture listening on http://127.0.0.1:54321\n"[..],
        &launch
    )
    .await
    .is_err());
    assert!(readiness(&mut &vec![b'x'; 8193][..], &launch)
        .await
        .is_err());
}

#[tokio::test]
async fn http_readiness_selects_the_declared_protocol_without_version_guessing() {
    let mut declaration: RuntimeHttpSse = serde_json::from_value(json!({"readinessPrefix":"legacy listening on ","passwordEnv":"FIXTURE_PASSWORD","username":"fixture","usernameEnv":"FIXTURE_USERNAME","eventPath":"/event","eventMethod":"fixture/event","routes":{}})).unwrap();
    let mut modern = declaration.clone();
    modern.readiness_prefix = "server listening on ".into();
    modern.event_path = "/api/event".into();
    declaration.alternatives.push(modern);
    let launch = Launch {
        args: vec![],
        password: "opaque".into(),
        declaration,
        url: Url::parse("http://127.0.0.1:12345/").unwrap(),
    };
    let selected = readiness(
        &mut &b"server listening on http://127.0.0.1:12345\n"[..],
        &launch,
    )
    .await
    .unwrap();
    assert_eq!(selected.event_path, "/api/event");
    let selected = readiness(
        &mut &b"legacy listening on http://127.0.0.1:12345\n"[..],
        &launch,
    )
    .await
    .unwrap();
    assert_eq!(selected.event_path, "/event");
    assert!(readiness(
        &mut &b"server listening on http://127.0.0.1:54321\n"[..],
        &launch
    )
    .await
    .is_err());
}

#[test]
fn http_requests_drop_foreign_and_duplicate_events_and_claim_answers_once() {
    let (client, _messages) = Client::channel(None, 16, 16);
    client.trace_binding(&codeterm_plugin::agent_runtime::RuntimeContext {
        thread_id: Some("own".into()),
        ..Default::default()
    });
    let declaration = serde_json::from_value(json!({"readinessPrefix":"fixture listening on ","passwordEnv":"FIXTURE_PASSWORD","username":"fixture","usernameEnv":"FIXTURE_USERNAME","eventPath":"/event","eventMethod":"fixture/event","routes":{},"requestEvents":{"opaque.request":{"typePointer":"/type","idPointer":"/properties/id","threadPointer":"/properties/sessionID","replyMethod":"opaque.reply","rejectMethod":"opaque.reject","pathParams":{"request":"/properties/id"}}}})).unwrap();
    let mut requests = requests::Requests::default();
    let foreign =
        json!({"type":"opaque.request","properties":{"id":"request","sessionID":"foreign"}});
    assert!(requests
        .frame(&client, &declaration, foreign)
        .unwrap()
        .is_none());
    let own = json!({"type":"opaque.request","properties":{"id":"request","sessionID":"own"}});
    assert!(matches!(
        requests.frame(&client, &declaration, own.clone()).unwrap(),
        Some(RuntimeFrame::Request { .. })
    ));
    assert!(requests
        .frame(&client, &declaration, own.clone())
        .unwrap()
        .is_none());
    let (id, reply) = requests
        .reply(&json!({"id":"request","result":{"decision":"opaque"}}))
        .unwrap();
    assert_eq!(reply["method"], "opaque.reply");
    assert_eq!(
        reply["params"],
        json!({"path":{"request":"request"},"body":{"decision":"opaque"}})
    );
    requests.claim(&id).unwrap();
    assert!(requests.claim(&id).is_err());
    assert!(requests
        .reply(&json!({"id":"request","result":{}}))
        .is_err());
    assert!(requests
        .frame(&client, &declaration, own)
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn http_v2_reply_refusal_keeps_transport_open_and_never_replays() {
    let fixture: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../tests/fixtures/runtime-http-sse.json"
    )))
    .unwrap();
    let declaration: RuntimeHttpSse =
        serde_json::from_value(fixture["capability"]["httpSse"]["alternatives"][0].clone())
            .unwrap();
    let (client, mut messages) = Client::channel(None, 16, 16);
    client.trace_binding(&codeterm_plugin::agent_runtime::RuntimeContext {
        thread_id: Some("ses_owned".into()),
        ..Default::default()
    });
    let mut requests = requests::Requests::default();
    for frame in fixture["frames"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|frame| frame["kind"] == "request")
    {
        let params = frame["params"].clone();
        let foreign: Value =
            serde_json::from_str(&params.to_string().replace("ses_owned", "ses_foreign")).unwrap();
        assert!(requests
            .frame(&client, &declaration, foreign)
            .unwrap()
            .is_none());
        let Some(RuntimeFrame::Request { request_id, .. }) =
            requests.frame(&client, &declaration, params).unwrap()
        else {
            panic!("own request required")
        };
        let permission = frame["method"] == "permission.asked";
        if permission {
            let pending = Mutex::new(requests);
            let mut bookkeeping = std::pin::pin!(client.reply(
                &request_id,
                Err(RpcFailure {
                    code: -32602,
                    message: "Request not handled by host".into(),
                    data: None,
                })
            ));
            let write = tokio::select! {
                write = messages.recv() => write.unwrap(),
                unexpected = &mut bookkeeping => panic!("bookkeeping completed before transport: {unexpected:?}"),
            };
            // Removing routes proves no HTTP request is attempted.
            let mut no_routes = declaration.clone();
            no_routes.routes.clear();
            execute(
                &client,
                &reqwest::Client::new(),
                &Url::parse("http://127.0.0.1:1").unwrap(),
                &no_routes,
                "unused",
                &pending,
                write,
            )
            .await
            .unwrap();
            assert_eq!(bookkeeping.await, Ok(()));
            requests = pending.into_inner();
        }
        let body = if permission {
            json!({"decision":"always"})
        } else {
            json!({"answer":{"choice":"beta"}})
        };
        let mut result = std::pin::pin!(client.reply(&request_id, Ok(body.clone())));
        let write = tokio::select! {
            write = messages.recv() => write.unwrap(),
            unexpected = &mut result => panic!("reply finished before HTTP status: {unexpected:?}"),
        };
        let (id, reply) = requests
            .reply(&serde_json::from_str(&write.text).unwrap())
            .unwrap();
        assert_eq!(id, request_id);
        assert_eq!(reply["params"]["path"]["sessionID"], "ses_owned");
        assert_eq!(reply["params"]["body"], body);
        assert_eq!(
            reply["method"],
            if permission {
                "v2.permission.reply"
            } else {
                "v2.form.reply"
            }
        );
        requests.claim(&request_id).unwrap();
        if permission {
            finish_reply(write, reqwest::StatusCode::NOT_FOUND);
            assert_eq!(
                result.await,
                Err(RuntimeFailure::ReplyRefused { status: 404 })
            );
        } else {
            finish_reply(write, reqwest::StatusCode::NO_CONTENT);
            assert_eq!(result.await, Ok(()));
        }
        assert!(client.ended().borrow().is_none());
        assert!(requests
            .reply(&json!({"id":request_id,"result":{}}))
            .is_err());
    }
}

#[test]
fn http_protocol_candidates_preserve_bounded_auth_and_route_declarations() {
    let fixture: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../tests/fixtures/runtime-http-sse.json"
    )))
    .unwrap();
    let capability: AgentRuntimeCapability =
        serde_json::from_value(fixture["capability"].clone()).unwrap();
    let declaration = capability.http_sse.as_ref().unwrap();
    assert!(declaration.valid_for(&capability));
    for malformed in ["nested", "duplicate", "completion", "missing-route"] {
        let mut candidate = declaration.clone();
        match malformed {
            "nested" => candidate.alternatives[0]
                .alternatives
                .push(declaration.alternatives[0].clone()),
            "duplicate" => {
                candidate.alternatives[0].readiness_prefix = candidate.readiness_prefix.clone()
            }
            "completion" => candidate.alternatives[0].completion_events = false,
            "missing-route" => {
                candidate.alternatives[0].routes.remove("v2.session.prompt");
            }
            _ => unreachable!(),
        }
        assert!(!candidate.valid_for(&capability), "{malformed}");
    }
}
