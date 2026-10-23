use super::*;

#[test]
fn daemon_frontend_waits_for_a_complete_ready_snapshot() {
    let mut events = StartupEvents::default();
    for chunk in [
        b"event: startup-state\r\ndata: {\"state\":\"starting\"}\r\n\r\n".as_slice(),
        b"event: other\ndata: {\"state\":\"ready\"}\n\n",
        b"event: startup-state\ndata: {\"sta",
        b"te\":\"ready\"}\n",
    ] {
        assert!(!events.push(chunk).unwrap());
    }
    assert!(events.push(b"\n").unwrap());
}

#[test]
fn daemon_frontend_refuses_failed_or_invalid_startup() {
    for snapshot in ["{\"state\":\"failed\"}", "{}", "invalid"] {
        let mut events = StartupEvents::default();
        assert!(events
            .push(format!("event: startup-state\ndata: {snapshot}\n\n").as_bytes())
            .is_err());
    }
}

#[test]
fn daemon_frontend_bounds_startup_event_memory() {
    assert!(StartupEvents::default()
        .push(&vec![b'x'; 64 * 1024 + 1])
        .is_err());
}
