use super::*;

#[test]
fn version_skew_mismatch_round_trips_both_versions_and_recovery() {
    let error = version_skew_gate(4, 9, "recover --now").expect("older client is refused");

    assert_eq!(error.code, VERSION_SKEW);
    let data = error.data.expect("typed error carries structured data");
    assert_eq!(data["type"], "version_skew");
    assert_eq!(data["client_protocol"], 4);
    assert_eq!(data["server_protocol"], 9);
    assert_eq!(data["recovery"], "recover --now");
    assert!(error.message.contains("4"));
    assert!(error.message.contains("9"));
    assert!(error.message.contains("recover --now"));
}

#[test]
fn version_skew_same_version_gate_is_inert() {
    assert!(version_skew_gate(PROTOCOL_VERSION, PROTOCOL_VERSION, RECOVERY_COMMAND).is_none());
}

#[test]
fn version_skew_newer_client_is_not_refused() {
    assert!(version_skew_gate(PROTOCOL_VERSION + 1, PROTOCOL_VERSION, RECOVERY_COMMAND).is_none());
}

#[test]
fn version_skew_absent_or_malformed_client_version_is_treated_as_oldest() {
    assert_eq!(client_protocol_version(None), 0);
    for malformed in [
        serde_json::json!("newest"),
        serde_json::json!(-3),
        serde_json::json!(1.5),
        serde_json::json!(null),
        serde_json::json!({"protocol_version": 1}),
    ] {
        assert_eq!(client_protocol_version(Some(&malformed)), 0);
    }

    let error = version_skew_gate(
        client_protocol_version(None),
        PROTOCOL_VERSION,
        RECOVERY_COMMAND,
    )
    .expect("oldest client receives the typed error, not a crash");
    let data = error.data.expect("typed error carries structured data");
    assert_eq!(data["client_protocol"], 0);
    assert_eq!(data["server_protocol"], PROTOCOL_VERSION);
    assert_eq!(data["recovery"], RECOVERY_COMMAND);
}

#[derive(serde::Deserialize)]
struct MeshRequest {
    #[allow(dead_code)]
    method: String,
    #[serde(default)]
    protocol_version: Option<serde_json::Value>,
}

#[test]
fn version_skew_error_serializes_untranslated_over_mesh_rpc() {
    let error = version_skew_gate(2, PROTOCOL_VERSION, "recover --here")
        .expect("older mesh client is refused");
    let wire = serde_json::json!({ "result": null, "error": error });
    assert_eq!(wire["error"]["code"], VERSION_SKEW);
    assert_eq!(wire["error"]["data"]["type"], "version_skew");
    assert_eq!(wire["error"]["data"]["client_protocol"], 2);
    assert_eq!(wire["error"]["data"]["server_protocol"], PROTOCOL_VERSION);
    assert_eq!(wire["error"]["data"]["recovery"], "recover --here");
}

#[test]
fn version_skew_mesh_request_parses_with_and_without_the_version_field() {
    let versioned: MeshRequest = serde_json::from_str(&format!(
            r#"{{"method":"plan.get","params":null,"request_id":"r1","protocol_version":{PROTOCOL_VERSION}}}"#
        ))
        .unwrap();
    assert!(version_skew_gate(
        client_protocol_version(versioned.protocol_version.as_ref()),
        PROTOCOL_VERSION,
        RECOVERY_COMMAND,
    )
    .is_none());

    let legacy: MeshRequest =
        serde_json::from_str(r#"{"method":"plan.get","params":null,"request_id":"r2"}"#)
            .expect("a peer without the version field must not fail parsing");
    assert_eq!(client_protocol_version(legacy.protocol_version.as_ref()), 0);
}
