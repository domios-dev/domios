use super::*;
use crate::service::test_fixture::RecordedUiEvents;

#[test]
fn status_emission_preserves_payload_and_event() {
    let recorded = Arc::new(RecordedUiEvents::default());
    let emitter = UiStatusEmitter(recorded.clone());
    let payload = serde_json::json!({
        "active": true,
        "display": false,
        "expires_at": 42,
        "warnings": ["probe-warning"],
        "admin_commands": ["probe-command"],
        "gnome_policy": true,
        "system_block": false
    });
    let status = serde_json::from_value(payload.clone()).unwrap();
    emitter.emit_status(&status);
    assert_eq!(
        *recorded.0.lock(),
        vec![("keepawake-changed".to_owned(), payload)]
    );
}
