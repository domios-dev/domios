use super::*;

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
