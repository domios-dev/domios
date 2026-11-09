use super::web_safe_event_payload;

#[test]
fn config_events_do_not_publish_api_tokens_to_sse() {
    let payload = serde_json::json!({
        "api": { "token": "permanent-secret", "port": 7685 },
        "mesh": { "enabled": true }
    });
    let safe = web_safe_event_payload("config-changed", payload);
    assert_eq!(safe["api"]["token"], serde_json::Value::Null);
    assert_eq!(safe["api"]["port"], 7685);
}
