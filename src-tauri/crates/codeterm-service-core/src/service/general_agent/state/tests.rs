use super::*;
use std::io::Write;

mod ownership;

#[test]
fn saved_runtime_thread_is_validated_by_resume_without_filesystem_discovery() {
    assert!(can_resume_saved_session(true, || panic!(
        "runtime thread must use exact resume"
    )));
    assert!(can_resume_saved_session(false, || Some(true)));
    assert!(!can_resume_saved_session(false, || Some(false)));
    assert!(!can_resume_saved_session(false, || None));
}

#[test]
fn provider_switch_preserves_each_providers_recorded_session_id() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state").join("general-agent-session.json");
    let registry = crate::agent::registry::AgentRegistry::new();
    registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::with_session("provider-a", "session-a-1"),
    ));
    registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::with_session("provider-b", "session-b-1"),
    ));

    persist_session_id_to(&path, "provider-a", "session-a-1");
    persist_session_id_to(&path, "provider-b", "session-b-1");

    assert!(has_recorded_session_in(dir.path()));
    let first = load_session_record_from(&registry, "provider-a", "cwd", &[], &path)
        .expect("previous provider session must survive switching providers");
    assert_eq!(first.session_id, "session-a-1");
    let second = load_session_record_from(&registry, "provider-b", "cwd", &[], &path)
        .expect("current provider session must load");
    assert_eq!(second.session_id, "session-b-1");
}

#[test]
fn first_seen_is_durable_and_never_moves() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(first_seen_at(dir.path(), 100).unwrap(), 100);
    assert_eq!(first_seen_at(dir.path(), 200).unwrap(), 100);
    assert_eq!(first_seen_at(dir.path(), 50).unwrap(), 100);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("state/first-seen")).unwrap(),
        "100"
    );
}

#[test]
fn existing_first_seen_marker_is_reused_without_rewriting() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("state")).unwrap();
    let path = dir.path().join("state/first-seen");
    std::fs::write(&path, "000100").unwrap();
    assert_eq!(first_seen_at(dir.path(), 200).unwrap(), 100);
    assert_eq!(std::fs::read_to_string(path).unwrap(), "000100");
}

#[test]
fn invalid_existing_marker_is_not_reset() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("state")).unwrap();
    std::fs::write(dir.path().join("state/first-seen"), "invalid").unwrap();
    assert!(first_seen_at(dir.path(), 200).is_err());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("state/first-seen")).unwrap(),
        "invalid"
    );
}

#[test]
fn shared_general_prompt_cannot_discover_main_after_independent_sessions() {
    let mut searches = 0;
    let selected = discover_main_session(true, || {
        searches += 1;
        Some("newest-sibling-session".into())
    });
    assert_eq!(selected, None);
    assert_eq!(searches, 0);
    assert_eq!(
        discover_main_session(false, || Some("main-session".into())),
        Some("main-session".into())
    );
}

#[test]
fn picks_ga_marked_session_over_newer_worker_session() {
    let dir = tempfile::tempdir().unwrap();
    let ga = dir.path().join("ga.jsonl");
    let worker = dir.path().join("worker.jsonl");
    write_jsonl(
        &ga,
        r##"{"type":"system","message":{"content":"# CodeTerm General Agent (v1)\nYou are the General Agent - the always-on system brain."}}"##,
    );
    write_jsonl(
        &worker,
        r#"{"type":"user","message":{"content":"domios agent report haiku facts done\nGPU facts"}}"#,
    );

    let mut candidates = vec![
        SessionCandidate {
            session_id: "ga-session".into(),
            file_path: ga.to_string_lossy().to_string(),
            sort_ts: 10,
        },
        SessionCandidate {
            session_id: "worker-session".into(),
            file_path: worker.to_string_lossy().to_string(),
            sort_ts: 20,
        },
    ];

    assert_eq!(
        pick_general_agent_session(&mut candidates).as_deref(),
        Some("ga-session")
    );
}

#[test]
fn rejects_worker_only_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let worker = dir.path().join("worker.jsonl");
    write_jsonl(
        &worker,
        r#"{"type":"user","message":{"content":"domios agent report haiku facts done\nGPU facts"}}"#,
    );

    let mut candidates = vec![SessionCandidate {
        session_id: "worker-session".into(),
        file_path: worker.to_string_lossy().to_string(),
        sort_ts: 20,
    }];

    assert_eq!(pick_general_agent_session(&mut candidates), None);
}

#[test]
fn scans_oversized_session_with_multibyte_text_at_the_limit() {
    let dir = tempfile::tempdir().unwrap();
    let ga = dir.path().join("ga.jsonl");

    // Pad so a Cyrillic codepoint straddles MARKER_SCAN_LIMIT, leaving a
    // UTF-8 continuation byte at the limit — what used to panic startup.
    let mut line =
        String::from(r##"{"type":"system","message":{"content":"# CodeTerm General Agent"}}"##);
    line.push_str(&"a".repeat(MARKER_SCAN_LIMIT - line.len() - 1));
    line.push('я');
    line.push_str(&"b".repeat(1024));
    assert!(line.len() > MARKER_SCAN_LIMIT);
    assert!(!line.is_char_boundary(MARKER_SCAN_LIMIT));
    write_jsonl(&ga, &line);

    let mut candidates = vec![SessionCandidate {
        session_id: "ga-session".into(),
        file_path: ga.to_string_lossy().to_string(),
        sort_ts: 10,
    }];

    assert_eq!(
        pick_general_agent_session(&mut candidates).as_deref(),
        Some("ga-session")
    );
}

#[test]
fn general_agent_marker_scan_reads_at_most_limit() {
    let mut bytes = vec![b'x'; 3 * 1024 * 1024];
    bytes.extend_from_slice(b"# CodeTerm General Agent");
    let mut reader = std::io::Cursor::new(bytes);
    assert!(!session_file_has_ga_marker(&mut reader));
    assert_eq!(reader.position(), MARKER_SCAN_LIMIT as u64);
}

#[test]
fn general_agent_scan_skips_sqlite_and_repeated_paths() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("shared.db");
    let mut file = std::fs::File::create(&path).unwrap();
    file.write_all(b"SQLite format 3\0# CodeTerm General Agent")
        .unwrap();
    let mut candidates: Vec<_> = (0..5)
        .map(|index| SessionCandidate {
            session_id: format!("session-{index}"),
            file_path: path.to_string_lossy().into_owned(),
            sort_ts: 5 - index,
        })
        .collect();
    let mut scans = 0;
    let selected = pick_general_agent_session_with(&mut candidates, |candidate_path| {
        scans += 1;
        is_general_agent_session_file(candidate_path)
    });
    assert_eq!(selected, None);
    assert_eq!(scans, 1);
}

fn write_jsonl(path: &Path, line: &str) {
    let mut file = std::fs::File::create(path).unwrap();
    writeln!(file, "{line}").unwrap();
}
