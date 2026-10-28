use super::*;
use std::cell::Cell;

#[test]
fn server_events_redact_generic_web_payloads() {
    let events = Arc::new(codeterm_events::EventBroadcaster::new());
    let mut web = events.subscribe();
    let ui = ServerEvents {
        events,
        native: Arc::new(parking_lot::RwLock::new(None)),
    };
    ui.emit(
        "config-changed",
        serde_json::json!({ "api": { "token": "secret", "port": 1234 } }),
    );
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
    assert!(web.try_recv().is_err());
}

#[test]
fn server_events_keep_native_audiences_off_web() {
    let events = Arc::new(codeterm_events::EventBroadcaster::new());
    let mut web = events.subscribe();
    let ui = ServerEvents {
        events,
        native: Arc::new(parking_lot::RwLock::new(None)),
    };
    ui.emit_native("native-probe", serde_json::json!({ "native": true }));
    ui.emit_to("main", "targeted-probe", serde_json::Value::Null);
    assert!(web.try_recv().is_err());
}

#[test]
fn development_frontend_runtime_identity_is_validated_and_gated() {
    for (debug, development) in [(false, true), (true, false), (false, false)] {
        assert_eq!(
            development_frontend_configuration(debug, development, "invalid", "com.domios.app")
                .unwrap(),
            (None, String::new())
        );
    }
    for (port, identifier) in [
        ("1821", "com.domios.dev.first"),
        ("1822", "com.domios.dev.second"),
    ] {
        let (origin, actual) =
            development_frontend_configuration(true, true, port, identifier).unwrap();
        assert_eq!(
            origin.unwrap().as_str(),
            format!("http://127.0.0.1:{port}/")
        );
        assert_eq!(actual, identifier);
    }
    for port in ["0", "65536", "-1", "1821/path", " 1821"] {
        assert!(
            development_frontend_configuration(true, true, port, "com.domios.dev.first").is_err()
        );
    }
    for identifier in ["", "com.domios.app", "foreign", "com.domios.dev.bad/path"] {
        assert!(development_frontend_configuration(true, true, "1821", identifier).is_err());
    }
}

#[test]
fn development_frontend_requires_a_debug_development_server_and_loopback_origin() {
    let value = "http://127.0.0.1:1420";
    assert!(development_frontend_origin(false, true, value)
        .unwrap()
        .is_none());
    assert!(development_frontend_origin(true, false, value)
        .unwrap()
        .is_none());
    assert!(development_frontend_origin(true, true, "https://foreign.invalid").is_err());
    assert_eq!(
        development_frontend_origin(true, true, value)
            .unwrap()
            .unwrap()
            .origin()
            .ascii_serialization(),
        value
    );
}

#[test]
fn process_split_contract_native_listener_failure_is_distinct_from_inventory_rejection() {
    use crate::native_host::NativeHostBindError;
    for kind in [io::ErrorKind::PermissionDenied, io::ErrorKind::Other] {
        let unavailable = optional_native_listener::<()>(Err(
            NativeHostBindError::ListenerUnavailable(io::Error::from(kind)),
        ));
        assert!(unavailable.unwrap().is_none());
        let rejected = optional_native_listener::<()>(Err(NativeHostBindError::InventoryRejected(
            io::Error::from(kind),
        )));
        assert!(rejected.is_err());
    }
}

#[test]
fn process_split_contract_readiness_ack_failure_still_runs_cleanup_once() {
    let controls = Cell::new(0);
    let cleanups = Cell::new(0);
    let control = continue_after_ack(Err(io::Error::from(io::ErrorKind::InvalidData)), || {
        controls.set(controls.get() + 1);
        Ok(None::<()>)
    });
    let result = finish_server_control(
        control,
        || {
            cleanups.set(cleanups.get() + 1);
            Ok(())
        },
        |_, _| panic!("No shutdown was admitted"),
    );
    assert!(result.is_err());
    assert_eq!(controls.get(), 0);
    assert_eq!(cleanups.get(), 1);
}

#[test]
fn process_split_contract_launcher_disconnect_keeps_service_control_independent() {
    let controls = Cell::new(0);
    let result = continue_after_ack(Err(io::Error::from(io::ErrorKind::BrokenPipe)), || {
        controls.set(controls.get() + 1);
        Ok(42)
    })
    .unwrap();
    assert_eq!(result, 42);
    assert_eq!(controls.get(), 1);
}
