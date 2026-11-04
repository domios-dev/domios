use super::super::model::ItemCache;
use super::*;

fn row(id: &str, seq: i64, kind: &str, content: &str) -> ChatMsg {
    ChatMsg {
        id: id.into(),
        seq,
        kind: kind.into(),
        content: content.into(),
        ..Default::default()
    }
}

#[test]
fn two_turns_restore_in_order_and_new_turn_keeps_distinct_ids() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("transcript.jsonl");
    let initial = vec![
        row("one-user", 0, "user", "first"),
        row("one-answer", 1, "assistant", "one"),
        row("two-user", 2, "user", "second"),
        row("two-answer", 3, "assistant", "two"),
    ];
    ChatBackendHistory::open_at(path.clone())
        .snapshot(initial.clone())
        .unwrap();
    let restored = chatbackend_history::try_read_messages(&path).unwrap();
    let identity = |rows: &[ChatMsg]| {
        rows.iter()
            .map(|row| (row.id.clone(), row.kind.clone(), row.content.clone(), row.seq))
            .collect::<Vec<_>>()
    };
    assert_eq!(identity(&restored), identity(&initial));
    let mut cache = ItemCache::default();
    cache.restore_transcript(restored.clone());
    cache.restore_transcript(restored);
    cache.reconcile(row("three-user", 4, "user", "third"));
    cache.reconcile(row("three-answer", 5, "assistant", "three"));
    let expected = [
        "one-user",
        "one-answer",
        "two-user",
        "two-answer",
        "three-user",
        "three-answer",
    ];
    assert_eq!(
        cache
            .snapshot()
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        expected
    );
    ChatBackendHistory::open_at(path.clone())
        .snapshot(bounded_rows(cache.snapshot()))
        .unwrap();
    let final_rows = chatbackend_history::try_read_messages(&path).unwrap();
    assert_eq!(final_rows.len(), 6);
    assert_eq!(
        final_rows.iter().map(|row| row.seq).collect::<Vec<_>>(),
        vec![0, 1, 2, 3, 4, 5]
    );
}

#[test]
fn declared_remote_or_stored_history_never_selects_host_fallback() {
    let capability: AgentRuntimeCapability = serde_json::from_value(serde_json::json!({
        "runtime":"stdio-jsonrpc","command":{"binary":"fixture","args":[]},"requestMethods":{},"notificationMethods":[]
    })).unwrap();
    assert!(enabled(&capability));
    let mut remote = capability.clone();
    remote
        .request_methods
        .insert("readHistory".into(), vec!["history/read".into()]);
    assert!(!enabled(&remote));
    let mut stored = capability;
    stored.stored_history = true;
    assert!(!enabled(&stored));
}

#[test]
fn transcript_snapshot_bounds_disk_and_retains_only_rows() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("bounded.jsonl");
    let input = (0..12)
        .map(|index| {
            row(
                &format!("row-{index}"),
                index,
                "assistant",
                &"x".repeat(512 * 1024),
            )
        })
        .collect();
    let retained = bounded_rows(input);
    assert!(!retained.is_empty());
    let expected = retained.clone();
    let mut writer = ChatBackendHistory::open_at(path.clone());
    writer.snapshot(retained).unwrap();
    writer.snapshot(expected.clone()).unwrap();
    assert!(std::fs::metadata(&path).unwrap().len() <= chatbackend_history::TAIL_BYTES);
    assert_eq!(
        chatbackend_history::try_read_messages(&path).unwrap(),
        expected
    );
    assert!(chatbackend_history::read_meta(&path).is_none());
}

#[test]
fn opaque_session_ids_do_not_alias_runtime_transcripts() {
    assert_ne!(
        chatbackend_history::runtime_path("fixture", "part/one"),
        chatbackend_history::runtime_path("fixture", "part_one")
    );
    assert_ne!(
        chatbackend_history::runtime_path("fixture", "part/one"),
        chatbackend_history::runtime_path("other", "part/one")
    );
}

#[test]
fn transcript_io_failures_are_nonfatal_and_recovery_remains_available() {
    let root = tempfile::tempdir().unwrap();
    let io = TranscriptIo::default();
    assert!(io
        .read_rows(&root.path().join("missing.jsonl"), "fixture")
        .is_empty());
    assert!(!io.read_failed.load(Ordering::Relaxed));
    assert!(chatbackend_history::try_read_messages(root.path()).is_err());
    assert!(io.read_rows(root.path(), "fixture").is_empty());
    assert!(io.read_failed.load(Ordering::Relaxed));
    assert!(io.read_rows(root.path(), "fixture").is_empty());
    let mut invalid = ChatBackendHistory::open_at(root.path().to_path_buf());
    assert!(invalid
        .snapshot(vec![row("item", 0, "assistant", "body")])
        .is_err());
    io.write_rows(
        &mut invalid,
        vec![row("item", 0, "assistant", "body")],
        "fixture",
    );
    assert!(io.write_failed.load(Ordering::Relaxed));
    let path = root.path().join("valid.jsonl");
    let expected = vec![row("item", 0, "assistant", "body")];
    io.write_rows(
        &mut ChatBackendHistory::open_at(path.clone()),
        expected.clone(),
        "fixture",
    );
    assert_eq!(io.read_rows(&path, "fixture"), expected);
}
