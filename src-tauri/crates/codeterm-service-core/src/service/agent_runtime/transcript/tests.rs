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
            .map(|row| {
                (
                    row.id.clone(),
                    row.kind.clone(),
                    row.content.clone(),
                    row.seq,
                )
            })
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

fn append(io: &TranscriptIo, cache: &Mutex<ItemCache>, dir: &Path, rotate_at: u64) {
    io.append_archive(
        cache,
        &dir.join("archive.jsonl"),
        &dir.join("archive.prev.jsonl"),
        rotate_at,
        "fixture",
    );
}

fn tracking_cache() -> Mutex<ItemCache> {
    let mut cache = ItemCache::default();
    cache.track_archive();
    Mutex::new(cache)
}

#[test]
fn the_session_log_keeps_every_row_the_restart_snapshot_drops() {
    let root = tempfile::tempdir().unwrap();
    let (io, cache) = (TranscriptIo::default(), tracking_cache());
    let total = REREADABLE_CACHE_ROWS + 50;
    for index in 0..total {
        cache
            .lock()
            .reconcile(row(&format!("row-{index}"), 0, "assistant", "body"));
    }
    append(&io, &cache, root.path(), u64::MAX);
    let snapshot = root.path().join("snapshot.jsonl");
    ChatBackendHistory::open_at(snapshot.clone())
        .snapshot(bounded_rows(cache.lock().tail(REREADABLE_CACHE_ROWS)))
        .unwrap();
    assert_eq!(
        chatbackend_history::read_log(&snapshot).unwrap().len(),
        REREADABLE_CACHE_ROWS
    );
    let merged = merged_logs(&[snapshot, root.path().join("archive.jsonl")])
        .unwrap()
        .unwrap();
    assert_eq!(merged.len(), total);
    assert_eq!(merged[0].id, "row-0");
    assert_eq!(merged[total - 1].id, format!("row-{}", total - 1));
}

#[test]
fn the_session_log_appends_only_what_changed() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("archive.jsonl");
    let (io, cache) = (TranscriptIo::default(), tracking_cache());
    for index in 0..20 {
        cache.lock().reconcile(row(
            &format!("row-{index}"),
            0,
            "assistant",
            &"x".repeat(4096),
        ));
    }
    append(&io, &cache, root.path(), u64::MAX);
    let first = std::fs::metadata(&path).unwrap().len();
    append(&io, &cache, root.path(), u64::MAX);
    assert_eq!(std::fs::metadata(&path).unwrap().len(), first);
    let mut changed = row("row-3", 0, "assistant", &"x".repeat(4096));
    changed.content.push_str("-final");
    cache.lock().reconcile(changed);
    append(&io, &cache, root.path(), u64::MAX);
    let grown = std::fs::metadata(&path).unwrap().len() - first;
    assert!(grown > 0 && grown < 1024, "one small delta, grew {grown}");
    let rows = chatbackend_history::read_log(&path).unwrap();
    assert_eq!(rows.len(), 20);
    assert!(rows[3].content.ends_with("-final"));
}

#[test]
fn a_full_session_log_rotates_once_and_nothing_retained_is_lost() {
    let root = tempfile::tempdir().unwrap();
    let (io, cache) = (TranscriptIo::default(), tracking_cache());
    cache.lock().reconcile(row("old", 0, "assistant", "old"));
    append(&io, &cache, root.path(), 1);
    cache.lock().reconcile(row("new", 0, "assistant", "new"));
    append(&io, &cache, root.path(), 1);
    assert!(root.path().join("archive.prev.jsonl").is_file());
    let merged = merged_logs(&[
        root.path().join("archive.prev.jsonl"),
        root.path().join("archive.jsonl"),
    ])
    .unwrap()
    .unwrap();
    assert_eq!(
        merged.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
        ["old", "new"]
    );
}

#[test]
fn a_failed_append_keeps_its_rows_for_the_next_one() {
    let root = tempfile::tempdir().unwrap();
    let (io, cache) = (TranscriptIo::default(), tracking_cache());
    cache.lock().reconcile(row("kept", 0, "assistant", "body"));
    let blocked = root.path().join("blocked");
    std::fs::create_dir(&blocked).unwrap();
    std::fs::write(blocked.join("file"), b"").unwrap();
    io.append_archive(
        &cache,
        &blocked.join("file").join("archive.jsonl"),
        &root.path().join("unused"),
        u64::MAX,
        "fixture",
    );
    assert!(io.archive_failed.load(Ordering::Relaxed));
    append(&io, &cache, root.path(), u64::MAX);
    let rows = chatbackend_history::read_log(&root.path().join("archive.jsonl")).unwrap();
    assert_eq!(rows[0].id, "kept");
}

#[test]
fn restored_rows_have_nothing_new_to_append() {
    let cache = tracking_cache();
    cache
        .lock()
        .restore_transcript(vec![row("restored", 4, "assistant", "body")]);
    assert!(cache.lock().take_archive_rows().is_empty());
    let mut untracked = ItemCache::default();
    untracked.reconcile(row("ignored", 0, "assistant", "body"));
    assert!(untracked.take_archive_rows().is_empty());
}

#[test]
fn mid_turn_appends_are_rate_limited_without_a_timer() {
    let last = AtomicU64::new(0);
    assert!(archive_tick_due(&last, ARCHIVE_TICK_MS));
    assert!(!archive_tick_due(&last, ARCHIVE_TICK_MS + 1));
    assert!(archive_tick_due(&last, 2 * ARCHIVE_TICK_MS));
}

#[test]
fn no_log_reads_as_absent_rather_than_empty() {
    let root = tempfile::tempdir().unwrap();
    assert!(merged_logs(&[root.path().join("missing.jsonl")])
        .unwrap()
        .is_none());
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

#[test]
fn concurrent_refusal_next_tool_turn_settles_and_restore_retains_all_rows() {
    use super::super::events::{self, RuntimeEvent, RuntimeMaps};
    use codeterm_plugin::agent_runtime::{AgentRuntimeEvent, RuntimeTurnState};
    let fixture: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../plugins/provider-acp/tests/fixtures/runtime/concurrent-rpc-refusal.json"
    )))
    .unwrap();
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("transcript.jsonl");
    let mut history = ChatBackendHistory::open_at(path.clone());
    let mut cache = ItemCache::default();
    let mut maps = RuntimeMaps::default();
    let mut settled = Vec::new();
    for value in fixture["expected"]["events"].as_array().unwrap() {
        let event: AgentRuntimeEvent = serde_json::from_value(value.clone()).unwrap();
        let Some(runtime) = events::translate_with_items(&event, &mut maps, false) else {
            continue;
        };
        events::fold(&mut maps, 1, &runtime);
        if let RuntimeEvent::ItemUpdated { item, .. } = &runtime {
            cache.reconcile(item.clone());
        }
        if let AgentRuntimeEvent::TurnState {
            turn_id: Some(turn),
            state: RuntimeTurnState::Idle,
            ..
        } = event
        {
            settled.push(turn);
            history.snapshot(cache.snapshot()).unwrap();
        }
    }
    assert_eq!(
        settled,
        ["session-fixture:turn:1", "session-fixture:turn:3"]
    );
    let stored = chatbackend_history::try_read_messages(&path).unwrap();
    let mut restored = ItemCache::default();
    restored.restore_transcript(stored.clone());
    restored.restore_transcript(stored);
    let restored = restored.snapshot();
    let identity = |rows: &[ChatMsg]| {
        rows.iter()
            .map(|row| {
                (
                    row.id.clone(),
                    row.kind.clone(),
                    row.content.clone(),
                    row.seq,
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(identity(&restored), identity(&cache.snapshot()));
    assert!(restored.iter().any(|row| row.content == "NEXT_TOOL_DONE"));
    assert!(restored
        .iter()
        .any(|row| row.tool_status.as_deref() == Some("completed")));
}
