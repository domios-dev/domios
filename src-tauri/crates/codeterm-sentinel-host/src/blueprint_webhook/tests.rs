use super::*;

fn signed(secret: &str, bytes: &[u8], timestamp: Option<&str>) -> Authentication {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    if let Some(at) = timestamp {
        mac.update(at.as_bytes());
        mac.update(b".");
    }
    mac.update(bytes);
    let signature = mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    Authentication {
        signature: Some(format!("sha256={signature}")),
        timestamp: timestamp.map(str::to_owned),
        ..Default::default()
    }
}

#[test]
fn webhook_auth_requires_graph_secret_or_exact_raw_body_hmac() {
    let bytes = br#"{"message":"opaque"}"#;
    assert_eq!(
        authenticate("graph-secret", &Authentication::default(), bytes, 1000),
        Err(Refusal::Unauthorized)
    );
    for bearer in ["daemon-api-token", "wrong", "graph-secret-extra"] {
        assert_eq!(
            authenticate(
                "graph-secret",
                &Authentication {
                    bearer: Some(bearer.into()),
                    ..Default::default()
                },
                bytes,
                1000
            ),
            Err(Refusal::Unauthorized)
        );
    }
    assert!(authenticate(
        "graph-secret",
        &Authentication {
            bearer: Some("graph-secret".into()),
            ..Default::default()
        },
        bytes,
        1000
    )
    .is_ok());
    let auth = signed("graph-secret", bytes, None);
    assert!(authenticate("graph-secret", &auth, bytes, 1000).is_ok());
    assert_eq!(
        authenticate("graph-secret", &auth, b"{}", 1000),
        Err(Refusal::Unauthorized)
    );
    assert_eq!(
        authenticate("rotated-secret", &auth, bytes, 1000),
        Err(Refusal::Unauthorized)
    );
    let malformed = Authentication {
        signature: Some(format!("sha256={}", "é".repeat(32))),
        ..Default::default()
    };
    assert_eq!(
        authenticate("graph-secret", &malformed, bytes, 1000),
        Err(Refusal::Unauthorized)
    );
}

#[test]
fn webhook_timestamp_is_signed_and_has_five_minute_tolerance() {
    let auth = signed("opaque", b"{}", Some("1000"));
    assert!(authenticate("opaque", &auth, b"{}", 1300).is_ok());
    assert_eq!(
        authenticate("opaque", &auth, b"{}", 1301),
        Err(Refusal::Unauthorized)
    );
    assert_eq!(
        authenticate("opaque", &auth, b"{}", 699),
        Err(Refusal::Unauthorized)
    );
    let mut tampered = auth;
    tampered.timestamp = Some("1200".into());
    assert_eq!(
        authenticate("opaque", &tampered, b"{}", 1200),
        Err(Refusal::Unauthorized)
    );
}

#[test]
fn webhook_json_bounds_preserve_strings_and_reject_truncation() {
    assert_eq!(parse_payload(br#""7""#).unwrap(), json!("7"));
    assert_eq!(parse_payload(br#"{"n":7}"#).unwrap(), json!({"n":7}));
    assert!(parse_payload(b"not JSON").is_err());
    assert!(parse_payload(&vec![b' '; MAX_BODY_BYTES + 1]).is_err());
    let deep = format!("{}0{}", "[".repeat(33), "]".repeat(33));
    assert!(parse_payload(deep.as_bytes()).is_err());
    let huge = serde_json::to_vec(&"x".repeat(32768)).unwrap();
    assert!(parse_payload(&huge).is_err());
}

#[test]
fn webhook_limits_are_global_per_graph_and_time_bounded() {
    let mut limits = limits::Limits::default();
    for _ in 0..120 {
        limits.attempt(1000).unwrap();
    }
    assert_eq!(limits.attempt(1001), Err(Refusal::RateLimited));
    limits.attempt(61000).unwrap();
    for _ in 0..30 {
        limits.admit("one", 1000).unwrap();
    }
    assert_eq!(limits.admit("one", 1001), Err(Refusal::RateLimited));
    limits.admit("two", 1001).unwrap();
    limits.admit("one", 61000).unwrap();
}

#[test]
fn webhook_delivery_dedup_is_scoped_bounded_and_expires() {
    let mut limits = limits::Limits::default();
    assert_eq!(
        limits.duplicate("one", Some("delivery"), 1000).unwrap(),
        None
    );
    limits.remember("one", Some("delivery".into()), "run", 1000);
    assert_eq!(
        limits.duplicate("one", Some("delivery"), 1001).unwrap(),
        Some("run".into())
    );
    assert_eq!(
        limits.duplicate("two", Some("delivery"), 1001).unwrap(),
        None
    );
    assert_eq!(
        limits.duplicate("one", Some("delivery"), 601000).unwrap(),
        None
    );
    assert_eq!(
        limits.duplicate("one", Some("\n"), 601000),
        Err(Refusal::InvalidPayload)
    );
    for index in 0..256 {
        let delivery = format!("d-{index}");
        limits.duplicate("one", Some(&delivery), 601000).unwrap();
        limits.remember("one", Some(delivery), "run", 601000);
    }
    assert_eq!(
        limits.duplicate("one", Some("next"), 601000),
        Err(Refusal::RateLimited)
    );
}
