use super::*;

#[test]
fn disconnected_registry_peer_retains_typed_code_and_connection_message() {
    let body = serde_json::json!({
        "code": delivery::PEER_UNREACHABLE,
        "message": "Peer ROG is paired but not currently connected",
        "bytes_written": false,
        "hint": "Check `domios mesh peers` for connection progress.",
    });
    let failure = classify_relay_answer(
        "ROG",
        ("POST", "/api/tabs/main/send"),
        reqwest::StatusCode::SERVICE_UNAVAILABLE,
        &serde_json::to_vec(&body).unwrap(),
    )
    .unwrap_err();
    assert_eq!(failure.code.as_deref(), Some(delivery::PEER_UNREACHABLE));
    assert_eq!(failure.bytes_written, Some(false));
    assert_eq!(failure.class, Some(RefusalClass::Transient));
    assert_eq!(failure.reason, body["message"].as_str().unwrap());
    assert_eq!(failure.retry_hint, body["hint"].as_str().unwrap());
    let encoded = failure.to_json();
    assert_eq!(encoded["error"]["code"], delivery::PEER_UNREACHABLE);
    assert_eq!(encoded["error"]["bytes_written"], false);
    assert!(!encoded.to_string().contains("peer_not_paired"));
}

#[test]
fn protocol_upgrade_refusal_keeps_code_and_update_instruction() {
    let body = serde_json::json!({
        "code": delivery::PEER_RELAY_PROTOCOL_MISMATCH,
        "message": "Peer AIR runs an older Domios; update it through the updater.",
        "relay_protocol_version": 2,
        "required_relay_protocol_version": 3,
        "bytes_written": false,
    });
    let failure = classify_relay_answer(
        "AIR",
        ("GET", "/api/system/default-cwd"),
        reqwest::StatusCode::UPGRADE_REQUIRED,
        &serde_json::to_vec(&body).unwrap(),
    )
    .unwrap_err();
    assert_eq!(
        failure.code.as_deref(),
        Some(delivery::PEER_RELAY_PROTOCOL_MISMATCH)
    );
    assert_eq!(failure.bytes_written, Some(false));
    assert_eq!(failure.class, Some(RefusalClass::Deterministic));
    assert!(failure.reason.contains("older Domios"));
    assert!(failure.retry_hint.contains("Update the peer"));
    let encoded = failure.to_json();
    assert_eq!(
        encoded["error"]["code"],
        delivery::PEER_RELAY_PROTOCOL_MISMATCH
    );
}
