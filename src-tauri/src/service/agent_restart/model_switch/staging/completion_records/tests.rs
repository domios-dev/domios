use super::*;
use std::io::Write;

fn declaration() -> ModelSwitchTurnCompletion {
    ModelSwitchTurnCompletion {
        source: crate::agent::provider::ModelSwitchCompletionSource::SessionJsonl,
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

#[test]
fn captured_grok_busy_prompt_completes_only_on_its_matching_update() {
    let session = "01a122fc-abe9-7281-9837-24a92a378d30";
    let prompt = "6993c5a8-10bb-4991-843f-b5ad9a2a1ede";
    let mut file = tempfile::NamedTempFile::new().unwrap();
    writeln!(file, "{}", serde_json::json!({"timestamp":1791588406,"method":"session/update","params":{"sessionId":session,"update":{"sessionUpdate":"user_message_chunk"}}})).unwrap();
    let mut boundary = RecordBoundary::capture(file.path().into(), declaration(), session).unwrap();
    writeln!(file, "{}", serde_json::json!({"timestamp":1791588409,"method":"session/update","params":{"sessionId":session,"update":{"sessionUpdate":"agent_thought_chunk"},"_meta":{"promptId":prompt,"agentTimestampMs":1791588409737u64}}})).unwrap();
    assert!(!boundary.completed(session).unwrap());
    writeln!(file, "{}", serde_json::json!({"timestamp":1791588642,"method":"_x.ai/session/update","params":{"sessionId":session,"update":{"sessionUpdate":"turn_completed","prompt_id":prompt,"stop_reason":"end_turn"},"_meta":{"agentTimestampMs":1791588642873u64}}})).unwrap();
    assert!(boundary.completed(session).unwrap());
}

#[test]
fn captured_start_after_accepted_stop_blocks_reuse_before_the_new_completion() {
    let session = "01a123ba-0840-7101-9fb5-3bdb4745a9e3";
    let captured = include_str!("fixtures/grok-start-after-stop.jsonl");
    let lines = captured.lines().collect::<Vec<_>>();
    let mut file = tempfile::NamedTempFile::new().unwrap();
    writeln!(file, "{}\n{}", lines[0], lines[1]).unwrap();
    let completed = RecordBoundary::capture(file.path().into(), declaration(), session).unwrap();
    assert!(!completed.in_progress());
    writeln!(file, "{}", lines[2]).unwrap();
    let mut active = RecordBoundary::capture(file.path().into(), declaration(), session).unwrap();
    assert!(active.in_progress());
    assert!(!active.observe(lines[1].as_bytes(), session));
    assert!(active.in_progress());
    let prompt = "bb318ab2-57d1-4554-9d3f-1f11104c2084";
    let started = serde_json::json!({"params":{"sessionId":session,"_meta":{"promptId":prompt}}});
    assert!(!active.observe(started.to_string().as_bytes(), session));
    let completion = serde_json::json!({"params":{"sessionId":session,"update":{"sessionUpdate":"turn_completed","prompt_id":prompt}}});
    assert!(active.observe(completion.to_string().as_bytes(), session));
    assert!(!active.in_progress());
}
