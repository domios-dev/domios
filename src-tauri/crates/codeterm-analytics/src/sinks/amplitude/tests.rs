use super::*;

#[test]
fn payload_carries_standard_context_outside_catalog_properties() {
    let mut server = mockito::Server::new();
    let mut sink = Http::with_key(Config::default(), Some("test-key".into())).unwrap();
    sink.context = crate::Context::new("1.20.1", Some("10.0.26100".into()), Some("en_US.UTF-8"))
        .with_device(
            Some("Fixture manufacturer".into()),
            Some("Fixture model".into()),
        );
    sink.destination(format!("{}/2/httpapi", server.url()));
    let event = DeliveryEvent {
        insert_id: uuid::Uuid::new_v4(),
        device_id: uuid::Uuid::new_v4(),
        user_id: Some("test-account".into()),
        time: 123,
        event: crate::Event::AppOpened {},
    };
    let destination = server.mock("POST", "/2/httpapi")
        .match_body(mockito::Matcher::Json(serde_json::json!({
            "api_key": "test-key", "events": [{
                "event_type": "app_opened", "event_properties": {},
                "insert_id": event.insert_id, "device_id": event.device_id,
                "user_id": "test-account", "time": 123, "ip": "$remote",
                "platform": "Desktop", "os_name": if cfg!(windows) { "Windows" } else if cfg!(target_os = "macos") { "macOS" } else { "Linux" },
                "os_version": "10.0.26100", "app_version": "1.20.1", "language": "en",
                "device_manufacturer": "Fixture manufacturer", "device_model": "Fixture model", "carrier": "none"
            }]
        })))
        .with_status(200).expect(1).create();
    assert_eq!(sink.deliver(&[event]), DeliveryResult::Delivered);
    destination.assert();
}

#[test]
fn unavailable_context_omits_versions_and_does_not_forward_arbitrary_locale_text() {
    let context = crate::Context::new("private-version", None, Some("private/path"));
    let value = serde_json::to_value(context).unwrap();
    assert_eq!(value["language"], "und");
    assert!(value.get("app_version").is_none());
    assert!(value.get("os_version").is_none());
    assert!(value.get("device_model").is_none());
    assert!(value.get("device_manufacturer").is_none());
    assert_eq!(value["carrier"], "none");
}

#[test]
fn public_build_key_is_overridden_by_local_secret_store() {
    let store: SecretReader = std::sync::Arc::new(|name| {
        assert_eq!(name, "domios.amplitude.prod.api_key");
        Ok("local-key".into())
    });
    assert_eq!(
        resolve_key(Some("public-build-key"), Project::Prod, &store).as_deref(),
        Some("local-key")
    );
    let missing: SecretReader = std::sync::Arc::new(|_| Err("missing".into()));
    assert_eq!(
        resolve_key(Some("public-build-key"), Project::Prod, &missing).as_deref(),
        Some("public-build-key")
    );
    assert_eq!(resolve_key(None, Project::Prod, &missing), None);
    let blank: SecretReader = std::sync::Arc::new(|_| Ok(" ".into()));
    assert_eq!(
        resolve_key(Some("public-build-key"), Project::Test, &blank).as_deref(),
        Some("public-build-key")
    );
    assert_eq!(resolve_key(Some(" "), Project::Test, &blank), None);
}
