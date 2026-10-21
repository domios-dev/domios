use super::*;
fn probe() -> HttpProbe {
    serde_json::from_value(json!({"url":"https://example.com/health?token=private"})).unwrap()
}
#[test]
fn http_health_defaults_match_2xx_and_exact_status_override() {
    let mut probe = probe();
    assert_eq!(probe.timeout_ms, 3000);
    assert_eq!(probe.expected_status, "2xx");
    assert!(probe.validate().is_ok());
    for code in [200, 204, 299] {
        assert_eq!(probe.reading(Some(code)).state, "up");
    }
    for code in [301, 404, 503] {
        let reading = probe.reading(Some(code));
        assert_eq!(reading.state, "down");
        assert_eq!(reading.output["code"], code);
    }
    assert_eq!(
        probe.reading(None).output,
        json!({"state":"down","code":null})
    );
    probe.expected_status = "301".into();
    assert_eq!(probe.reading(Some(301)).state, "up");
    assert_eq!(probe.reading(Some(200)).state, "down");
    assert_eq!(probe.display_url(), "https://example.com/health");
}
#[test]
fn http_health_refuses_invalid_configuration() {
    for url in [
        "file:///tmp/health",
        "https://user:secret@example.com/",
        "not-a-url",
        "http://",
    ] {
        let mut probe = probe();
        probe.url = url.into();
        assert!(probe.validate().is_err());
    }
    for timeout in [0, 10001] {
        let mut probe = probe();
        probe.timeout_ms = timeout;
        assert!(probe.validate().is_err());
    }
    for status in ["all", "099", "600"] {
        let mut probe = probe();
        probe.expected_status = status.into();
        assert!(probe.validate().is_err());
    }
}
