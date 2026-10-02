use super::*;
use serde_json::json;

#[test]
fn supporter_journal_rotation_keeps_two_bounded_files() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Journal::new(dir.path().into());
    let value = json!({ "fixture": "x".repeat(MAX_JOURNAL_BYTES as usize / 2) });
    for _ in 0..4 {
        journal.append(&value).unwrap();
    }
    let files: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap())
        .collect();
    assert_eq!(files.len(), 2);
    for file in files {
        assert!(file.metadata().unwrap().len() <= MAX_JOURNAL_BYTES);
        for line in fs::read_to_string(file.path()).unwrap().lines() {
            serde_json::from_str::<Value>(line).unwrap();
        }
    }
}

#[test]
fn supporter_feed_includes_rotated_records_newest_first() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Journal::new(dir.path().into());
    let old = json!({ "time": 1, "kind": "rerank", "label": "ranked", "action": "rerank", "reason": "all_candidates_scored" });
    let new = json!({ "time": 2, "kind": "tab_state", "label": "other", "action": "weak_hint", "reason": "launch_ready" });
    std::fs::write(
        dir.path().join("journal.previous.jsonl"),
        format!("{old}\n"),
    )
    .unwrap();
    journal.append(&new).unwrap();
    let entries = journal.feed(2, None).unwrap();
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry["time"].as_i64().unwrap())
            .collect::<Vec<_>>(),
        vec![2, 1]
    );
    assert_eq!(journal.feed(1, None).unwrap()[0]["time"], 2);
}
