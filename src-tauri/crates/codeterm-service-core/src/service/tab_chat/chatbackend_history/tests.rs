use super::*;
use serde_json::json;

fn msg(id: &str, content: &str) -> ChatMsg {
    ChatMsg {
        id: id.into(),
        kind: "assistant".into(),
        content: content.into(),
        ..Default::default()
    }
}

#[test]
fn writing_a_message_lands_in_jsonl() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.jsonl");
    let mut h = ChatBackendHistory::open_at(path.clone());
    assert!(h.upsert(&[msg("1", "hello")]), "first write changes file");

    let read = read_messages(&path);
    assert_eq!(read.len(), 1);
    assert_eq!(read[0].id, "1");
    assert_eq!(read[0].content, "hello");
}

#[test]
fn inbound_sender_metadata_roundtrips() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.jsonl");
    let mut h = ChatBackendHistory::open_at(path.clone());
    h.upsert(&[ChatMsg {
        id: "inbound-1".into(),
        kind: "user".into(),
        content: "hello".into(),
        from_label: Some("Edison".into()),
        from_tab: Some("e8a0ac20".into()),
        from_model: Some("opus".into()),
        inbound_kind: Some("tab".into()),
        ..Default::default()
    }]);

    let read = read_messages(&path);
    assert_eq!(read.len(), 1);
    assert_eq!(read[0].content, "hello");
    assert_eq!(read[0].from_label.as_deref(), Some("Edison"));
    assert_eq!(read[0].from_tab.as_deref(), Some("e8a0ac20"));
    assert_eq!(read[0].from_model.as_deref(), Some("opus"));
    assert_eq!(read[0].inbound_kind.as_deref(), Some("tab"));
    assert!(!read[0].content.contains("-=-codeterm"));
}

#[test]
fn streaming_message_yields_one_latest_committed_message() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.jsonl");
    let mut h = ChatBackendHistory::open_at(path.clone());
    // Same id grows across ticks (streaming tokens).
    h.upsert(&[msg("live", "He")]);
    h.upsert(&[msg("live", "Hello")]);
    h.upsert(&[msg("live", "Hello world")]);

    let raw = std::fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = raw.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines.len(), 3, "streamed versions append committed records");
    let read = read_messages(&path);
    assert_eq!(read.len(), 1);
    assert_eq!(read[0].content, "Hello world");
}

#[test]
fn unchanged_upsert_does_not_rewrite() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.jsonl");
    let mut h = ChatBackendHistory::open_at(path);
    assert!(h.upsert(&[msg("1", "a")]));
    assert!(
        !h.upsert(&[msg("1", "a")]),
        "byte-identical upsert is a no-op"
    );
}

#[test]
fn remove_allows_a_failed_delivery_to_retry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.jsonl");
    let mut h = ChatBackendHistory::open_at(path.clone());
    h.upsert(&[msg("request-1", "payload")]);
    assert!(h.remove("request-1"));
    assert!(read_messages(&path).is_empty());
    assert!(h.upsert(&[msg("request-1", "payload")]));
}

#[test]
fn multiple_messages_keep_insertion_order() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.jsonl");
    let mut h = ChatBackendHistory::open_at(path.clone());
    h.upsert(&[msg("a", "first"), msg("b", "second")]);
    h.upsert(&[msg("a", "first-edited")]);
    h.upsert(&[msg("c", "third")]);

    let read = read_messages(&path);
    let ids: Vec<&str> = read.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["a", "b", "c"], "stable order, edit in place");
    assert_eq!(read[0].content, "first-edited");
}

#[test]
fn reopen_loads_existing_transcript() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.jsonl");
    {
        let mut h = ChatBackendHistory::open_at(path.clone());
        h.upsert(&[msg("1", "a"), msg("2", "b")]);
    }
    // Fresh instance must see prior content and append, not clobber.
    let mut h2 = ChatBackendHistory::open_at(path.clone());
    h2.upsert(&[msg("3", "c")]);
    let read = read_messages(&path);
    assert_eq!(read.len(), 3);
}

#[test]
fn meta_header_is_first_line_and_excluded_from_messages() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.jsonl");
    let mut h = ChatBackendHistory::open_at(path.clone());
    h.ensure_meta(&json!({ "systemPrompt": "You are X", "preset": "p" }));
    h.upsert(&[msg("1", "hi")]);

    let read = read_messages(&path);
    assert_eq!(read.len(), 1, "meta line is not a message");
    let meta = read_meta(&path).expect("meta persisted");
    assert_eq!(meta["systemPrompt"], "You are X");
    assert_eq!(meta["preset"], "p");

    // First physical line is the meta header.
    let raw = std::fs::read_to_string(&path).unwrap();
    let first = raw.lines().next().unwrap();
    assert!(first.contains("_meta"));
}

#[test]
fn ensure_meta_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.jsonl");
    let mut h = ChatBackendHistory::open_at(path.clone());
    h.ensure_meta(&json!({ "preset": "first" }));
    h.ensure_meta(&json!({ "preset": "second" }));
    let meta = read_meta(&path).unwrap();
    assert_eq!(meta["preset"], "first", "first meta wins, not overwritten");
}

#[test]
fn sanitize_blocks_path_escape() {
    let p = session_path("plug/in", "../../etc/passwd");
    // sessions dir — every component sits directly under it.
    assert!(
        p.starts_with(crate::paths::data_dir().join(SESSIONS_SUBDIR)),
        "stays under sessions dir: {}",
        p.display()
    );
    assert!(
        p.components()
            .all(|c| c.as_os_str() != std::ffi::OsStr::new("..")),
        "no parent-dir component: {}",
        p.display()
    );
    assert!(p.to_string_lossy().ends_with(".jsonl"));
}

#[test]
fn migrate_renames_session_file() {
    // rename helper on data_dir-rooted paths through a unique plugin id.
    let plugin = "test-migrate-plugin-unique";
    let from = session_path(plugin, "old-sid");
    let to = session_path(plugin, "new-sid");
    let _ = std::fs::remove_file(&from);
    let _ = std::fs::remove_file(&to);

    let mut h = ChatBackendHistory::open_at(from.clone());
    h.upsert(&[msg("1", "carry me")]);
    assert!(from.exists());

    migrate(plugin, "old-sid", "new-sid");
    assert!(!from.exists(), "source moved");
    assert!(to.exists(), "dest created");
    assert_eq!(read_messages(&to)[0].content, "carry me");

    let _ = std::fs::remove_file(&to);
}

#[test]
fn shared_returns_one_instance_per_session() {
    let plugin = "test-shared-registry-unique";
    let session = "shared-sid";
    let _ = std::fs::remove_file(session_path(plugin, session));

    let a = ChatBackendHistory::shared(plugin, session);
    let b = ChatBackendHistory::shared(plugin, session);
    assert!(
        Arc::ptr_eq(&a, &b),
        "same (plugin, session) ⇒ the same shared instance"
    );

    // A different session is a distinct instance.
    let c = ChatBackendHistory::shared(plugin, "other-sid");
    assert!(!Arc::ptr_eq(&a, &c));

    // Upserts through one handle are visible to the other (shared by_id).
    a.lock().upsert(&[msg("1", "via a")]);
    let read = read_messages(&session_path(plugin, session));
    assert_eq!(read.len(), 1);
    assert_eq!(read[0].content, "via a");

    let _ = std::fs::remove_file(session_path(plugin, session));
    let _ = std::fs::remove_file(session_path(plugin, "other-sid"));
}

#[test]
fn unparseable_trailing_line_is_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.jsonl");
    std::fs::write(
        &path,
        "{\"id\":\"1\",\"type\":\"assistant\",\"content\":\"ok\"}\n{half-written",
    )
    .unwrap();
    let read = read_messages(&path);
    assert_eq!(read.len(), 1, "corrupt last line skipped, not fatal");
    assert_eq!(read[0].content, "ok");
}

#[test]
fn closing_releases_writer_after_the_last_inflight_handle() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    let registry = Mutex::new(HistoryRegistry::new());
    let key = ("plugin".into(), "session".into());
    let writer = shared_in(&registry, key.clone(), path.clone());
    let weak = Arc::downgrade(&writer);
    writer.lock().upsert(&[msg("1", "before close")]);
    drop(writer);
    assert!(weak.upgrade().is_some(), "live sessions keep their index");
    let inflight = shared_in(&registry, key.clone(), path.clone());
    release_in(&mut registry.lock(), &key);
    let late = shared_in(&registry, key.clone(), path.clone());
    assert!(Arc::ptr_eq(&inflight, &late));
    late.lock().upsert(&[msg("1", "after close")]);
    drop(inflight);
    drop(late);
    assert!(weak.upgrade().is_none());
    release_in(&mut registry.lock(), &key);
    assert!(registry.lock().is_empty());
    let reopened = shared_in(&registry, key.clone(), path.clone());
    assert!(!reopened.lock().upsert(&[msg("1", "after close")]));
    assert_eq!(read_messages(&path)[0].content, "after close");
    release_in(&mut registry.lock(), &key);
}

#[test]
fn racing_writer_acquisition_returns_one_instance() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Mutex::new(HistoryRegistry::new());
    let barrier = std::sync::Barrier::new(2);
    let key = ("plugin".into(), "racing".into());
    let path = dir.path().join("session.jsonl");
    std::thread::scope(|scope| {
        let acquire = || {
            barrier.wait();
            shared_in(&registry, key.clone(), path.clone())
        };
        let first = scope.spawn(acquire);
        let second = scope.spawn(acquire);
        let first = first.join().unwrap();
        let second = second.join().unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        release_in(&mut registry.lock(), &key);
        assert!(Arc::ptr_eq(
            &first,
            &shared_in(&registry, key.clone(), path.clone())
        ));
    });
    release_in(&mut registry.lock(), &key);
    assert!(registry.lock().is_empty());
}

#[test]
fn close_before_indexing_cannot_retain_or_duplicate_the_writer() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    let registry = Mutex::new(HistoryRegistry::new());
    let key = ("plugin".into(), "closing".into());
    let writer = shared_in(&registry, key.clone(), path.clone());
    assert!(writer.lock().owner.is_none());
    release_in(&mut registry.lock(), &key);
    let late = shared_in(&registry, key.clone(), path.clone());
    assert!(Arc::ptr_eq(&writer, &late));
    writer.lock().upsert(&[msg("1", "late append")]);
    assert_eq!(read_messages(&path)[0].content, "late append");
    let weak = Arc::downgrade(&writer);
    drop(writer);
    drop(late);
    assert!(weak.upgrade().is_none());
    release_in(&mut registry.lock(), &key);
    assert!(registry.lock().is_empty());
}

#[test]
fn a_torn_tail_is_preserved_and_replaced_by_the_next_record() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.jsonl");
    std::fs::write(
        &path,
        "{\"id\":\"1\",\"type\":\"assistant\",\"content\":\"ok\"}\n{half-written",
    )
    .unwrap();

    let mut h = ChatBackendHistory::open_at(path.clone());
    assert!(h.upsert(&[msg("2", "b")]));

    let copies: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|candidate| {
            candidate
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("s.jsonl.corrupt-"))
        })
        .collect();
    assert_eq!(copies.len(), 1);
    assert_eq!(
        std::fs::read_to_string(&copies[0]).unwrap(),
        "{half-written"
    );
    let ids: Vec<String> = read_messages(&path).into_iter().map(|m| m.id).collect();
    assert_eq!(ids, vec!["1".to_string(), "2".to_string()]);
}

fn lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn updating_an_earlier_message_appends_one_record_and_never_rewrites_the_log() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.jsonl");
    let mut h = ChatBackendHistory::open_at(path.clone());
    h.upsert(&[msg("a", "first"), msg("b", "second")]);
    let prefix = std::fs::read(&path).unwrap();
    h.upsert(&[msg("a", "first-edited")]);
    let after = std::fs::read(&path).unwrap();
    assert!(
        after.starts_with(&prefix),
        "existing bytes are never rewritten"
    );
    assert_eq!(lines(&path).len(), 3);
    let read = read_messages(&path);
    let ids: Vec<&str> = read.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["a", "b"]);
    assert_eq!(read[0].content, "first-edited");
}

#[test]
fn removing_an_earlier_message_appends_a_tombstone_that_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.jsonl");
    let mut h = ChatBackendHistory::open_at(path.clone());
    h.upsert(&[msg("a", "x"), msg("b", "y")]);
    assert!(h.remove("a"));
    assert_eq!(lines(&path).len(), 3);
    let ids: Vec<String> = read_messages(&path).into_iter().map(|m| m.id).collect();
    assert_eq!(ids, ["b"]);
    let mut reopened = ChatBackendHistory::open_at(path.clone());
    assert!(
        !reopened.upsert(&[msg("b", "y")]),
        "reopen indexes live digests"
    );
    assert!(
        reopened.upsert(&[msg("a", "x")]),
        "a retired id writes again"
    );
}

#[test]
fn meta_added_after_messages_compacts_atomically_then_appends_versions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.jsonl");
    let mut h = ChatBackendHistory::open_at(path.clone());
    h.upsert(&[msg("1", "a"), msg("live", "He")]);
    h.ensure_meta(&json!({ "preset": "late" }));
    h.upsert(&[msg("live", "Hello")]);
    let lines = lines(&path);
    assert!(lines[0].starts_with("{\"_meta\":"));
    assert_eq!(lines.len(), 4);
    assert_eq!(read_meta(&path).unwrap()["preset"], "late");
    let read = read_messages(&path);
    assert_eq!(read.len(), 2);
    assert_eq!(read[1].content, "Hello");
}

#[test]
fn bounded_windows_page_back_over_the_whole_log() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.jsonl");
    let mut h = ChatBackendHistory::open_at(path.clone());
    let all: Vec<ChatMsg> = (0..200)
        .map(|index| msg(&format!("m{index:03}"), &"x".repeat(100)))
        .collect();
    h.upsert(&all);
    let total = std::fs::metadata(&path).unwrap().len();
    let mut until = total;
    let mut pages = Vec::new();
    while until > 0 {
        let window = read_window(&path, until, 2_000).unwrap();
        assert!(window.end_offset - window.start_offset <= 2_000);
        assert!(window.start_offset < until);
        pages.push(window.messages);
        until = window.start_offset;
    }
    let ids: Vec<String> = pages.into_iter().rev().flatten().map(|m| m.id).collect();
    let expected: Vec<String> = all.into_iter().map(|m| m.id).collect();
    assert_eq!(ids, expected);
}

#[test]
fn one_upsert_batch_lands_as_one_write_and_stays_readable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.jsonl");
    let mut h = ChatBackendHistory::open_at(path.clone());
    let body = "x".repeat(2048);
    h.try_upsert(&[
        msg("a", &body),
        msg("b", "other"),
        msg("a", &format!("{body}-second")),
    ])
    .unwrap();
    let read = read_log(&path).unwrap();
    assert_eq!(read.len(), 2);
    assert_eq!(read[0].id, "a");
    assert!(read[0].content.ends_with("-second"));
    assert_eq!(read[1].id, "b");
    assert_eq!(read_messages(&path), read);
    drop(h);
    let mut reopened = ChatBackendHistory::open_at(path.clone());
    reopened
        .try_upsert(&[msg("a", &format!("{body}-third"))])
        .unwrap();
    assert!(read_log(&path).unwrap()[0].content.ends_with("-third"));
}

#[test]
fn session_logs_are_distinct_from_the_restart_snapshot() {
    assert_ne!(
        archive_path("fixture", "part/one"),
        runtime_path("fixture", "part/one")
    );
    assert_ne!(
        archive_path("fixture", "part/one"),
        archive_path("fixture", "part_one")
    );
    assert_ne!(
        archive_path("fixture", "part/one"),
        archive_rotated_path("fixture", "part/one")
    );
}
