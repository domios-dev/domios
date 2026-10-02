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
