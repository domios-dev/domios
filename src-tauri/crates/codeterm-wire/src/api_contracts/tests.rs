use super::*;
use serde_json::json;

#[test]
fn event_payloads_preserve_nullable_and_absent_fields() {
    let event = ActivityEvent {
        id: None,
        occurred_at: 10,
        source: ActivitySource::Peer,
        source_id: None,
        target: None,
        source_peer: None,
        level: ActivityLevel::Info,
        message: "ready".into(),
        meta: json!({}),
    };
    assert_eq!(
        serde_json::to_value(event).unwrap(),
        json!({
            "id": null, "occurred_at": 10, "source": "peer", "source_id": null,
            "target": null, "level": "info", "message": "ready", "meta": {},
        })
    );
    let log = LogEventPayload {
        ts: 10,
        level: "info".into(),
        source: "daemon".into(),
        message: "ready".into(),
        context: None,
    };
    assert_eq!(
        serde_json::to_value(log).unwrap(),
        json!({
            "ts": 10, "level": "info", "source": "daemon", "message": "ready", "context": null,
        })
    );
}
