use super::*;

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
        if let Some(frame) = decoder.push(byte).unwrap() {
            frames.push(frame);
        }
    }
    assert_eq!(frames, vec![json!({"text":"雪"}), json!({"next":true})]);
}

#[test]
fn sse_fails_closed_on_malformed_or_oversized_events() {
    for bytes in [b"data: {bad}\n\n".to_vec(), vec![b'x'; 256 * 1024 + 1]] {
        let mut decoder = sse::Decoder::default();
        assert!(bytes.into_iter().any(|byte| decoder.push(byte).is_err()));
    }
    let mut decoder = sse::Decoder::default();
    for byte in b"data: {\"partial\":true}" {
        assert!(decoder.push(*byte).unwrap().is_none());
    }
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
