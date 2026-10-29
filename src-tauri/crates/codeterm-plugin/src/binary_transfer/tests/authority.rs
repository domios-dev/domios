use super::*;
#[test]
fn redirects_reauthorize_and_never_forward_initial_credentials_or_custom_headers_across_origins() {
    let mut redirect = Reply::bytes(vec![]);
    redirect.status = 302;
    redirect.location = Some("https://cdn.example/output".into());
    let (_directory, session, _, network) = fixture(vec![redirect, Reply::bytes(vec![0, 255])]);
    let mut request = request();
    request.credential = Some(Credential::Secret {
        name: "key".into(),
        header: "authorization".into(),
        prefix: Some("Bearer ".into()),
    });
    request
        .headers
        .insert("idempotency-key".into(), "operation".into());
    execute(session, request, TransferAbort::default()).unwrap();
    let calls = network.calls.lock();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].1["authorization"], "Bearer opaque-secret");
    assert!(!calls[1].1.contains_key("authorization"));
    assert!(!calls[1].1.contains_key("idempotency-key"));
}
#[test]
fn redirects_to_ungranted_or_private_origins_and_upload_redirects_never_issue_a_second_request() {
    for location in [
        "https://foreign.example/output",
        "http://api.example/output",
        "https://127.0.0.1/output",
        "https://169.254.169.254/token",
    ] {
        let mut reply = Reply::bytes(vec![]);
        reply.status = 302;
        reply.location = Some(location.into());
        let (_directory, session, _, network) = fixture(vec![reply]);
        let error = execute(session, request(), TransferAbort::default()).unwrap_err();
        assert_eq!(error.error, TransferRefusal::NotAllowed);
        assert_eq!(network.calls.lock().len(), 1);
    }
    let mut reply = Reply::bytes(vec![]);
    reply.status = 307;
    reply.location = Some("https://api.example/again".into());
    let (_directory, session, _, network) = fixture(vec![reply]);
    std::fs::write(session.root.join("source"), [0, 255]).unwrap();
    let mut request = request();
    request.method = "PUT".into();
    request.body = Some(InputBody::File {
        file: Input::Owned {
            identity: None,
            path: "source".into(),
            content_type: "application/octet-stream".into(),
            file_name: "Source.bin".into(),
        },
    });
    let error = execute(session, request, TransferAbort::default()).unwrap_err();
    assert_eq!(error.error, TransferRefusal::RedirectDenied);
    assert_eq!(network.calls.lock().len(), 1);
}
#[test]
fn forged_headers_grants_protocols_and_expired_deadlines_refuse_before_transport() {
    let (_directory, session, _, network) = fixture(vec![]);
    for name in [
        "authorization",
        "cookie",
        "x-api-key",
        "host",
        "content-length",
        "proxy-authorization",
    ] {
        let mut request = request();
        request.headers.insert(name.into(), "opaque-secret".into());
        assert!(execute(session.clone(), request, TransferAbort::default()).is_err());
    }
    for url in [
        "https://user:secret@api.example/file",
        "https://api.example/file#fragment",
        "wss://api.example/stream",
        "https://localhost/file",
        "https://[::ffff:127.0.0.1]/file",
    ] {
        let mut request = request();
        request.url = url.into();
        assert!(execute(session.clone(), request, TransferAbort::default()).is_err());
    }
    let error = execute_at_deadline(session.clone(), request());
    assert_eq!(error.error, TransferRefusal::Deadline);
    assert!(network.calls.lock().is_empty());
}
fn execute_at_deadline(session: Arc<TransferSession>, request: TransferRequest) -> TransferFailure {
    super::super::worker::execute_at(
        session,
        request,
        TransferAbort::default(),
        Arc::new(Network::default()),
        Some(std::time::Instant::now()),
    )
    .unwrap_err()
}
#[test]
fn public_ip_and_exact_grants_refuse_private_rebinding_and_port_expansion() {
    for ip in [
        "127.0.0.1",
        "10.0.0.1",
        "100.64.0.1",
        "169.254.169.254",
        "192.0.2.1",
        "198.18.0.1",
        "224.0.0.1",
        "::1",
        "fc00::1",
        "fe80::1",
        "::ffff:127.0.0.1",
        "2001:db8::1",
        "2002:a00:1::1",
    ] {
        assert!(!public_ip(ip.parse().unwrap()), "{ip}");
    }
    for ip in ["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111"] {
        assert!(public_ip(ip.parse().unwrap()), "{ip}");
    }
    assert!(exact_grant(
        &Url::parse("https://api.example/path").unwrap(),
        &["api.example".into()]
    ));
    assert!(!exact_grant(
        &Url::parse("https://api.example:444/path").unwrap(),
        &["api.example".into()]
    ));
    assert!(!exact_grant(
        &Url::parse("https://sub.api.example/path").unwrap(),
        &["api.example".into()]
    ));
}

#[test]
fn credential_references_require_their_own_declared_destination_origin_pin() {
    let (_directory, session, _, network) = fixture(vec![]);
    let mut request = request();
    request.url = "https://cdn.example/output".into();
    request.credential = Some(Credential::Secret {
        name: "key".into(),
        header: "authorization".into(),
        prefix: None,
    });
    let error = execute(session, request, TransferAbort::default()).unwrap_err();
    assert_eq!(error.error, TransferRefusal::NotAllowed);
    assert!(!error.request_may_have_started);
    assert!(network.calls.lock().is_empty());
    for descriptor in [
        serde_json::json!({"credentialOrigins":{"declared:":["https://api.example"]}}),
        serde_json::json!({"credentialOrigins":{"secret:key":["https://api.example/path"]}}),
        serde_json::json!({"requestHeaders":["Authorization"]}),
        serde_json::json!({"requestHeaders":["x-version","X-Version"]}),
    ] {
        assert!(
            !serde_json::from_value::<BinaryTransferDescriptor>(descriptor)
                .unwrap()
                .valid()
        );
    }
}
#[test]
fn declared_public_headers_are_bounded_and_cannot_replace_host_credential_or_transport_headers() {
    let (_directory, base, authority, network) = fixture(vec![Reply::bytes(vec![0, 255])]);
    let mut limits = base.limits.clone();
    limits.request_headers = vec!["x-api-version".into()];
    let session =
        TransferSession::at_root(base.plugin_id(), base.root.clone(), limits, authority).unwrap();
    *session.test_network.lock() = Some(network.clone());
    let mut allowed = request();
    allowed
        .headers
        .insert("x-api-version".into(), "2026-01".into());
    execute(session.clone(), allowed, TransferAbort::default()).unwrap();
    assert_eq!(network.calls.lock()[0].1["x-api-version"], "2026-01");
    let mut unknown = request();
    unknown
        .headers
        .insert("x-other-header".into(), "opaque".into());
    assert_eq!(
        execute(session, unknown, TransferAbort::default())
            .unwrap_err()
            .error,
        TransferRefusal::NotAllowed
    );
    assert_eq!(network.calls.lock().len(), 1);
}
