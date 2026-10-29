use super::*;
#[test]
fn json_upload_keeps_secret_in_host_headers_and_uses_bounded_sinks() {
    let mut reply = Reply::bytes(br#"{"job":"opaque"}"#.to_vec());
    reply.mime = "application/json".into();
    let (_root, session, _, network) = fixture(vec![reply]);
    let mut request = request();
    request.method = "POST".into();
    request
        .headers
        .insert("content-type".into(), "text/plain".into());
    request.credential = Some(Credential::Secret {
        name: "key".into(),
        header: "authorization".into(),
        prefix: Some("Bearer ".into()),
    });
    request.body = Some(InputBody::Json {
        value: serde_json::json!({"prompt":"opaque"}),
    });
    request.response = Output::Json {
        max_bytes: 128,
        statuses: vec![200],
    };
    let receipt = execute(session, request, TransferAbort::default()).unwrap();
    assert_eq!(
        serde_json::to_value(receipt).unwrap()["data"],
        serde_json::json!({"job":"opaque"})
    );
    let calls = network.calls.lock();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].1["authorization"], "Bearer opaque-secret");
    assert!(!calls[0].1.contains_key("content-type"));
    assert_eq!(calls[0].2, br#"{"prompt":"opaque"}"#);
}
#[test]
fn json_upload_bounds_refuse_before_network_and_upload_redirect_never_replays() {
    let (_root, session, _, network) = fixture(vec![]);
    let mut oversized = request();
    oversized.method = "POST".into();
    oversized.body = Some(InputBody::Json {
        value: serde_json::json!({"prompt":"x".repeat(MAX_METADATA_BYTES as usize)}),
    });
    assert_eq!(
        execute(session, oversized, TransferAbort::default())
            .unwrap_err()
            .error,
        TransferRefusal::TooLarge
    );
    assert!(network.calls.lock().is_empty());
    let mut redirect = Reply::bytes(vec![]);
    redirect.status = 307;
    redirect.location = Some("https://api.example/again".into());
    let (_root, session, _, network) = fixture(vec![redirect]);
    let mut request = request();
    request.method = "POST".into();
    request.body = Some(InputBody::Json {
        value: serde_json::json!({"prompt":"opaque"}),
    });
    assert_eq!(
        execute(session, request, TransferAbort::default())
            .unwrap_err()
            .error,
        TransferRefusal::RedirectDenied
    );
    assert_eq!(network.calls.lock().len(), 1);
}
