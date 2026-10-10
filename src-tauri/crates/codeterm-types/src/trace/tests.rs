use super::*;

#[test]
fn default_sink_never_creates_a_directory_or_file() {
    let dir = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
    let mut sink = Sink::open(&dir, false).unwrap();
    sink.append(&Event::new(
        "send",
        "send.started",
        "worker",
        "provider",
        "accepted",
    ))
    .unwrap();
    assert!(!dir.exists());
}

#[test]
fn an_answer_from_another_flow_does_not_hide_a_missing_assistant_stage() {
    let events = [
        Event::new("one", "send.delivered", "tab", "provider", "exact_turn"),
        Event::new(
            "two",
            "transcript.assistant",
            "tab",
            "provider",
            "source_user_turn",
        ),
    ];
    let missing = gaps(&events);
    assert_eq!(missing.len(), 1);
    assert!(missing[0].starts_with("one:"));
}

#[test]
fn an_exact_source_link_retained_before_since_still_resolves_the_answer() {
    let mut start = Event::new("send", "send.started", "tab", "provider", "accepted");
    start.ts = 1;
    let mut link =
        Event::new("send", "trace.link", "tab", "provider", "exact_turn").related(Some("source"));
    link.ts = 2;
    let mut answer = Event::new(
        "source",
        "transcript.assistant",
        "tab",
        "provider",
        "source_user_turn",
    );
    answer.ts = 3;
    let chain = select(&[start, link, answer], "send", 3);
    assert_eq!(chain.len(), 1);
    assert_eq!(chain[0].trace_id, "send");
}

#[test]
fn flow_round_trip_orders_links_and_flags_missing_stages() {
    let dir = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
    let mut sink = Sink::open(&dir, true).unwrap();
    let stages = [
        "relay.expected",
        "relay.registered",
        "relay.observed",
        "report.emitted",
        "report.written",
    ];
    for (index, stage) in stages.iter().enumerate() {
        let mut event = Event::new("attempt", stage, "worker", "provider", "ok");
        event.ts = index as u64;
        event.related_id = Some(if index < 3 { "delivery" } else { "report" }.into());
        sink.append(&event).unwrap();
    }
    drop(sink);
    let events = read(&dir).unwrap();
    let chain = select(&events, "delivery", 0);
    assert_eq!(
        chain.iter().map(|e| e.stage.as_str()).collect::<Vec<_>>(),
        stages
    );
    assert!(gaps(&chain).is_empty());
    assert!(chain.iter().all(|event| event.trace_id == "attempt"));
    assert_eq!(recent(&events, 20).len(), 1);
    let missing: Vec<_> = chain
        .into_iter()
        .filter(|e| e.stage != "relay.registered")
        .collect();
    assert!(gaps(&missing)
        .iter()
        .any(|gap| gap.contains("no relay.registered")));
    assert_eq!(select(&events, "worker", 3).len(), 2);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn another_report_in_the_same_attempt_does_not_hide_a_missing_receipt() {
    let events = vec![
        Event::new("attempt", "report.emitted", "worker", "provider", "ok").related(Some("first")),
        Event::new("attempt", "report.emitted", "worker", "provider", "ok").related(Some("second")),
        Event::new("attempt", "report.written", "parent", "provider", "ok").related(Some("second")),
    ];
    assert_eq!(gaps(&events).len(), 1);
}

#[test]
fn rotation_closes_handle_and_keeps_a_bounded_generation_set() {
    let dir = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
    let mut sink = Sink::open(&dir, true).unwrap();
    let event = Event::new("id", "stage", "tab", "provider", "ok");
    for _ in 0..GENERATIONS + 2 {
        sink.written = MAX_BYTES;
        sink.append(&event).unwrap();
    }
    drop(sink);
    assert!(dir.join(format!("trace.jsonl.{GENERATIONS}")).exists());
    assert!(!dir
        .join(format!("trace.jsonl.{}", GENERATIONS + 1))
        .exists());
    assert_eq!(read(&dir).unwrap().len(), GENERATIONS + 1);
    std::fs::remove_dir_all(dir).unwrap();
}
