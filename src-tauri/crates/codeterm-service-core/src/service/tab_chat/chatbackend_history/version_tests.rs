use super::*;
use std::io::Write;

fn message(content: &str) -> ChatMsg {
    ChatMsg {
        id: "stream".into(),
        kind: "assistant".into(),
        content: content.into(),
        ..Default::default()
    }
}

#[test]
fn streaming_growth_appends_only_delta_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("growth.jsonl");
    let mut history = ChatBackendHistory::open_at(path.clone());
    let mut content = String::new();
    for _ in 0..64 {
        let prefix = std::fs::read(&path).unwrap_or_default();
        content.push_str(&"x".repeat(4096));
        let mut row = message(&content);
        row.tool_detail = Some(content.clone());
        row.tool_input = Some(serde_json::json!({"nested": [{"text": content}]}));
        assert!(history.upsert(&[row]));
        assert!(std::fs::read(&path).unwrap().starts_with(&prefix));
    }
    let bytes = std::fs::metadata(&path).unwrap().len();
    assert!(
        bytes < 8 * 3 * content.len() as u64 + 64 * 512,
        "{bytes} bytes for {} bytes of growth",
        content.len()
    );
    assert_eq!(read_messages(&path)[0].content, content);
}

#[test]
fn every_torn_version_prefix_preserves_the_last_commit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("torn.jsonl");
    let mut history = ChatBackendHistory::open_at(path.clone());
    let baseline = format!("{}last good 🌱", "x".repeat(1024));
    let updated = format!("{baseline} with more text");
    assert!(history.upsert(&[message(&baseline)]));
    let committed = std::fs::read(&path).unwrap();
    assert!(history.upsert(&[message(&updated)]));
    let full = std::fs::read(&path).unwrap();
    let update = &full[committed.len()..];
    assert!(update.starts_with(b"{\"_version\":"));
    drop(history);
    for cut in 0..update.len() {
        let mut torn = committed.clone();
        torn.extend_from_slice(&update[..cut]);
        std::fs::write(&path, torn).unwrap();
        assert_eq!(read_messages(&path)[0].content, baseline, "cut {cut}");
        let mut restarted = ChatBackendHistory::open_at(path.clone());
        assert!(restarted.upsert(&[message("recovered")]));
        assert_eq!(
            read_messages(&path)[0].content,
            "recovered",
            "restart cut {cut}"
        );
    }
    std::fs::write(&path, &full).unwrap();
    assert_eq!(read_messages(&path)[0].content, updated);
}

#[test]
fn reopen_recovers_committed_versions_and_discards_a_complete_json_without_newline() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("restart.jsonl");
    let mut history = ChatBackendHistory::open_at(path.clone());
    history.upsert(&[message("a")]);
    history.upsert(&[message("ab")]);
    drop(history);
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(
            serde_json::to_string(&message("uncommitted"))
                .unwrap()
                .as_bytes(),
        )
        .unwrap();
    assert_eq!(read_messages(&path)[0].content, "ab");
    let mut reopened = ChatBackendHistory::open_at(path.clone());
    assert!(!reopened.upsert(&[message("ab")]));
    assert!(reopened.upsert(&[message("abc")]));
    assert_eq!(read_messages(&path)[0].content, "abc");
}

#[test]
fn a_page_resolves_version_bases_before_its_byte_window() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("page.jsonl");
    let mut history = ChatBackendHistory::open_at(path.clone());
    history.upsert(&[message(&"x".repeat(8192))]);
    history.upsert(&[message(&format!("{}tail", "x".repeat(8192)))]);
    let end = std::fs::metadata(&path).unwrap().len();
    let window = read_window(&path, end, 512).unwrap();
    assert!(window.start_offset > 0);
    assert!(window.end_offset - window.start_offset < 512);
    assert_eq!(
        window.messages[0].content,
        format!("{}tail", "x".repeat(8192))
    );
}

#[test]
fn arbitrary_unicode_edits_and_metadata_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("unicode.jsonl");
    let mut history = ChatBackendHistory::open_at(path.clone());
    for content in [
        "🌱hello world🌱",
        "🌴hello world🌱",
        "🌴hello 🌍🌱",
        "",
        "🌱",
    ] {
        let mut row = message(content);
        row.tool_status = Some("running".into());
        row.tool_detail = Some(content.into());
        history.upsert(&[row.clone()]);
        assert_eq!(read_messages(&path), vec![row.with_segments()]);
    }
}

#[test]
fn compaction_replaces_the_file_atomically_and_reindexes_version_bases() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("compact.jsonl");
    let mut history = ChatBackendHistory::open_at(path.clone());
    history.ensure_meta(&serde_json::json!({"preset": "p"}));
    history.upsert(&[message("a")]);
    history.upsert(&[message("ab")]);
    let original = std::fs::read(&path).unwrap();
    let mut before = std::fs::File::open(&path).unwrap();
    let expected = read_messages(&path);
    history.compact().unwrap();
    let mut old_bytes = Vec::new();
    before.read_to_end(&mut old_bytes).unwrap();
    assert_eq!(
        old_bytes, original,
        "readers holding the old file see the complete prior log"
    );
    assert_eq!(read_messages(&path), expected);
    assert_eq!(read_meta(&path).unwrap()["preset"], "p");
    assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 2);
    assert!(history.upsert(&[message("abc")]));
    drop(history);
    let mut restarted = ChatBackendHistory::open_at(path.clone());
    assert!(!restarted.upsert(&[message("abc")]));
    assert_eq!(read_messages(&path)[0].content, "abc");
}

#[test]
fn failed_persistence_cannot_advance_the_live_index() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().join("blocked");
    std::fs::write(&parent, b"not a directory").unwrap();
    let path = parent.join("history.jsonl");
    let mut history = ChatBackendHistory::open_at(path.clone());
    assert!(history.try_upsert(&[message("a")]).is_err());
    std::fs::remove_file(&parent).unwrap();
    assert!(history.try_upsert(&[message("a")]).unwrap());
    assert_eq!(read_messages(&path)[0].content, "a");
}

#[test]
fn nested_array_growth_truncation_and_field_removal_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested.jsonl");
    let mut history = ChatBackendHistory::open_at(path.clone());
    for value in [
        serde_json::json!({"items": [{"text": "a"}], "drop": true}),
        serde_json::json!({"items": [{"text": "abc"}, {"text": "new"}]}),
        serde_json::json!({"items": []}),
        serde_json::json!({"items": "changed type"}),
    ] {
        let mut row = message("stable");
        row.tool_input = Some(value);
        assert!(history.upsert(&[row.clone()]));
        assert_eq!(read_messages(&path), vec![row.with_segments()]);
    }
    assert!(history.upsert(&[message("stable")]));
    assert_eq!(
        read_messages(&path),
        vec![message("stable").with_segments()]
    );
}

#[test]
fn repeated_edits_append_checkpoints_to_bound_version_reconstruction() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("checkpoint.jsonl");
    let mut history = ChatBackendHistory::open_at(path.clone());
    let mut row = message(&"x".repeat(4096));
    for index in 0..96 {
        row.tool_status = Some(format!("status-{index}"));
        assert!(history.upsert(&[row.clone()]));
    }
    let log = std::fs::read_to_string(&path).unwrap();
    assert!(
        log.lines()
            .filter(|line| !line.starts_with(r#"{"_version":"#))
            .count()
            > 1
    );
    let end = log.len() as u64;
    assert_eq!(
        read_window(&path, end, 512).unwrap().messages,
        vec![row.with_segments()]
    );
}
