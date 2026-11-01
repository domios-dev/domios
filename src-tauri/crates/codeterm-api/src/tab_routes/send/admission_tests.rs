use super::*;

#[test]
fn replay_mode_is_explicit_and_defaults_to_fresh_admission() {
    let fresh: SendTextBody =
        serde_json::from_value(serde_json::json!({"text": "opaque"})).unwrap();
    assert!(!fresh.replay_only);
    for field in ["replayOnly", "replay_only"] {
        let replay: SendTextBody =
            serde_json::from_value(serde_json::json!({(field): true})).unwrap();
        assert!(replay.replay_only);
    }
}

#[test]
fn unknown_replay_admission_is_never_reported_safe_to_resend() {
    let rejection =
        chat_admission_rejection("opaque-tab", "admission_unknown: opaque detail".into());
    assert_eq!(rejection.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(rejection.body["safe_to_resend"], false);
    assert_eq!(rejection.body["error"]["code"], "admission_unknown");
}
