use super::*;

#[test]
fn refuses_recursive_encoded_relative_and_traversal_paths() {
    for path in [
        "/api/stream-mux",
        "/api/stream-mux/s/requests/r",
        "/api/%73tream-mux/s",
        "https://evil/api/x",
        "//evil/api/x",
        "/api/x/../stream-mux",
        "/api/x/%2e%2e/stream-mux",
        "/api/x%5cy",
        "/api/x%00",
    ] {
        assert!(validate_path(path).is_err(), "{path}");
    }
    assert!(validate_path("/api/tabs/a%3Ab/preview/read?path=%2Ffile").is_ok());
}

#[test]
fn admission_binds_owner_refuses_duplicates_and_preserves_interactive_capacity() {
    assert_eq!(admission(false, false, false, 0, 0, 0, false), Err(403));
    assert_eq!(admission(true, true, false, 0, 0, 0, false), Err(409));
    assert_eq!(admission(true, false, false, 0, 64, 0, false), Err(429));
    assert_eq!(admission(true, false, false, 0, 48, 48, true), Err(429));
    assert!(admission(true, false, false, 0, 48, 48, false).is_ok());
    assert_eq!(admission(true, false, true, 256, 0, 0, false), Err(429));
}

#[test]
fn mutation_cancellation_cannot_be_claimed_by_metadata() {
    assert!(!cancellable(&Method::POST, "/api/tabs/t/chat-send", None));
    assert!(!cancellable(&Method::DELETE, "/api/logs", None));
    assert!(cancellable(&Method::POST, "/api/tabs/t/preview/read", None));
    assert!(!cancellable(
        &Method::POST,
        "/api/mesh/relay-to",
        Some(&RelayOperation {
            method: "POST".into(),
            path: "/api/tabs/t/chat-send".into()
        })
    ));
    assert!(cancellable(
        &Method::POST,
        "/api/mesh/relay-to",
        Some(&RelayOperation {
            method: "GET".into(),
            path: "/api/mux/state".into()
        })
    ));
}

#[test]
fn chat_snapshot_reads_are_cancellable_directly_and_through_relay() {
    assert!(cancellable(
        &Method::POST,
        "/api/tabs/t/chat-raw/start",
        None
    ));
    assert!(cancellable(
        &Method::POST,
        "/api/mesh/relay-to",
        Some(&RelayOperation {
            method: "POST".into(),
            path: "/api/tabs/t/chat-raw/start".into()
        })
    ));
    assert!(!cancellable(&Method::POST, "/api/tabs/t/chat-send", None));
}
