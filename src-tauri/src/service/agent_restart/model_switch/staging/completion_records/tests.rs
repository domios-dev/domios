use super::*;
use std::io::Write;

fn declaration() -> ModelSwitchTurnCompletion {
    ModelSwitchTurnCompletion {
        event_pointer: "/params/update/sessionUpdate".into(),
        event_value: "turn_completed".into(),
        session_pointer: "/params/sessionId".into(),
        start_event_value: "user_message_chunk".into(),
        turn_pointer: "/params/update/prompt_id".into(),
        active_turn_pointer: "/params/_meta/promptId".into(),
    }
}

fn record(session: &str, event: &str) -> String {
    let mut record = serde_json::json!({"params":{"sessionId":session,"update":{"sessionUpdate":event,"prompt_id":"turn"}}});
    if event == "agent_message_chunk" {
        record["params"]["_meta"] = serde_json::json!({"promptId":"turn"});
    }
    record.to_string()
}

#[test]
fn declared_completion_requires_exact_session_and_explicit_event() {
    for (session, event, expected) in [
        ("own", "turn_completed", true),
        ("foreign", "turn_completed", false),
        ("own", "agent_message_chunk", false),
        ("own", "idle", false),
    ] {
        assert_eq!(
            matches_record(
                &declaration(),
                &serde_json::from_str(&record(session, event)).unwrap(),
                "own"
            ),
            expected
        );
    }
    assert!(!matches_record(
        &declaration(),
        &serde_json::json!({}),
        "own"
    ));
}

#[test]
fn append_boundary_excludes_old_completion_and_partial_records_never_release() {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    writeln!(file, "{}", record("own", "agent_message_chunk")).unwrap();
    writeln!(file, "{}", record("own", "turn_completed")).unwrap();
    let mut boundary = RecordBoundary::capture(file.path().into(), declaration(), "own").unwrap();
    assert!(!boundary.completed("own").unwrap());
    writeln!(file, "{}", record("foreign", "turn_completed")).unwrap();
    write!(file, "{}", record("own", "turn_completed")).unwrap();
    assert!(!boundary.completed("own").unwrap());
    writeln!(file).unwrap();
    assert!(boundary.completed("own").unwrap());
    assert!(!boundary.completed("own").unwrap());
    file.as_file().set_len(0).unwrap();
    assert!(boundary.completed("own").is_err());
}

#[test]
fn newer_user_turn_invalidates_completion_and_wrong_prompt_id_cannot_release() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let mut boundary = RecordBoundary::capture(file.path().into(), declaration(), "own").unwrap();
    let active = serde_json::json!({"params":{"sessionId":"own","_meta":{"promptId":"current"}}});
    assert!(!boundary.observe(active.to_string().as_bytes(), "own"));
    assert!(!boundary.observe(record("own", "turn_completed").as_bytes(), "own"));
    let completion = serde_json::json!({"params":{"sessionId":"own","update":{"sessionUpdate":"turn_completed","prompt_id":"current"}}});
    assert!(boundary.observe(completion.to_string().as_bytes(), "own"));
    let next = serde_json::json!({"params":{"sessionId":"own","update":{"sessionUpdate":"user_message_chunk"}}});
    let batch = format!("{completion}\n{next}\n");
    assert!(!boundary.observe(batch.as_bytes(), "own"));
    assert!(!boundary.observe(completion.to_string().as_bytes(), "own"));
}

#[test]
fn watcher_wakes_on_own_append_and_the_matching_turn_releases() {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    writeln!(file, "{}", record("own", "agent_message_chunk")).unwrap();
    let mut boundary = RecordBoundary::capture(file.path().into(), declaration(), "own").unwrap();
    let (wake, events) = std::sync::mpsc::channel();
    let _watcher = boundary.watch(wake).unwrap();
    writeln!(file, "{}", record("own", "turn_completed")).unwrap();
    file.as_file().sync_all().unwrap();
    events
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    assert!(boundary.completed("own").unwrap());
}
