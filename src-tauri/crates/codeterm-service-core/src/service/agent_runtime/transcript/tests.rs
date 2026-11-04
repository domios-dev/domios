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
    assert_eq!(restored, initial);
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
