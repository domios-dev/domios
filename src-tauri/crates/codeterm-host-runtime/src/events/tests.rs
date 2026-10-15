use super::emit_everywhere;
use tauri::{Listener, Manager};

#[test]
fn emits_original_to_desktop_and_redacted_payload_to_web() {
    let app = tauri::test::mock_builder()
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    let events = std::sync::Arc::new(codeterm_events::EventBroadcaster::new());
    let mut web = events.subscribe();
    assert!(app.manage(events));
    let (sender, receiver) = std::sync::mpsc::channel();
    app.listen("config-changed", move |event| {
        sender.send(event.payload().to_owned()).unwrap();
    });
    let payload = serde_json::json!({ "api": { "token": "secret", "port": 1234 } });
    emit_everywhere(app.handle(), "config-changed", &payload);
    let desktop: serde_json::Value = serde_json::from_str(&receiver.try_recv().unwrap()).unwrap();
    assert_eq!(desktop, payload);
    match web.try_recv().unwrap().event {
        codeterm_events::SystemEvent::Generic { event_type, data } => {
            assert_eq!(event_type, "config-changed");
            assert_eq!(
                data,
                serde_json::json!({ "api": { "token": null, "port": 1234 } })
            );
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn emits_null_for_serialization_failure_without_web_broadcaster() {
    struct Unserializable;
    impl serde::Serialize for Unserializable {
        fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
            Err(serde::ser::Error::custom("probe failure"))
        }
    }
    let app = tauri::test::mock_builder()
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    app.listen("host-probe", move |event| {
        sender.send(event.payload().to_owned()).unwrap();
    });
    emit_everywhere(app.handle(), "host-probe", Unserializable);
    assert_eq!(receiver.try_recv().unwrap(), "null");
}
