use super::*;

fn write_fixture(dir: &Path, name: &str, content: &str) -> std::path::PathBuf {
    let path = dir.join(format!("{}.jsonl", name));
    std::fs::write(&path, content).unwrap();
    path
}

#[test]
fn empty_file_returns_zero_snapshot() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_fixture(tmp.path(), "empty", "");
    let snap = read_from_path(&path);
    assert_eq!(snap, UsageSnapshot::default());
}

#[test]
fn missing_file_returns_default_snapshot() {
    let tmp = tempfile::tempdir().unwrap();
    let snap = read_from_path(&tmp.path().join("nonexistent.jsonl"));
    assert_eq!(snap, UsageSnapshot::default());
}

#[test]
fn sums_cumulative_across_messages() {
    let tmp = tempfile::tempdir().unwrap();
    let content = concat!(
        r#"{"timestamp":"2026-05-07T10:00:00Z","message":{"usage":{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":10,"cache_read_input_tokens":5}}}"#,
        "\n",
        r#"{"timestamp":"2026-05-07T10:01:00Z","message":{"usage":{"input_tokens":200,"output_tokens":75,"cache_creation_input_tokens":20,"cache_read_input_tokens":15}}}"#,
        "\n",
        r#"{"timestamp":"2026-05-07T10:02:00Z","type":"system_event"}"#,
        "\n",
    );
    let path = write_fixture(tmp.path(), "session", content);
    let snap = read_from_path(&path);

    assert_eq!(snap.input_tokens, 300);
    assert_eq!(snap.output_tokens, 125);
    assert_eq!(snap.cache_creation_input_tokens, 30);
    assert_eq!(snap.cache_read_input_tokens, 20);
    assert_eq!(
        snap.message_count, 2,
        "system_event line has no usage block"
    );
}

#[test]
fn skips_malformed_lines_without_panic() {
    let tmp = tempfile::tempdir().unwrap();
    let content = concat!(
        r#"{"timestamp":"2026-05-07T10:00:00Z","message":{"usage":{"input_tokens":10,"output_tokens":5,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#,
        "\n",
        "this is not json at all\n",
        r#"{"unterminated":"#,
        "\n",
        r#"{"timestamp":"2026-05-07T10:01:00Z","message":{"usage":{"input_tokens":20,"output_tokens":10,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#,
        "\n",
    );
    let path = write_fixture(tmp.path(), "messy", content);
    let snap = read_from_path(&path);
    assert_eq!(snap.input_tokens, 30);
    assert_eq!(snap.output_tokens, 15);
    assert_eq!(snap.message_count, 2);
}

#[test]
fn captures_first_and_last_timestamps() {
    let tmp = tempfile::tempdir().unwrap();
    let content = concat!(
        r#"{"timestamp":"2026-05-07T10:00:00Z","message":{"usage":{"input_tokens":1,"output_tokens":1,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#,
        "\n",
        r#"{"timestamp":"2026-05-07T10:30:00Z","message":{"usage":{"input_tokens":1,"output_tokens":1,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#,
        "\n",
        r#"{"timestamp":"2026-05-07T11:15:00Z","message":{"usage":{"input_tokens":1,"output_tokens":1,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#,
        "\n",
    );
    let path = write_fixture(tmp.path(), "timestamped", content);
    let snap = read_from_path(&path);

    let first = snap.window_started_at.expect("first ts present");
    let last = snap.last_activity_at.expect("last ts present");
    assert_eq!(first.to_rfc3339(), "2026-05-07T10:00:00+00:00");
    assert_eq!(last.to_rfc3339(), "2026-05-07T11:15:00+00:00");
}

#[test]
fn missing_token_fields_default_to_zero() {
    let tmp = tempfile::tempdir().unwrap();
    let content = concat!(
        r#"{"timestamp":"2026-05-07T10:00:00Z","message":{"usage":{"input_tokens":42}}}"#,
        "\n",
    );
    let path = write_fixture(tmp.path(), "partial", content);
    let snap = read_from_path(&path);
    assert_eq!(snap.input_tokens, 42);
    assert_eq!(snap.output_tokens, 0);
    assert_eq!(snap.cache_creation_input_tokens, 0);
    assert_eq!(snap.cache_read_input_tokens, 0);
    assert_eq!(snap.message_count, 1);
}

fn usage_line(input: u64) -> String {
    format!(
            "{{\"timestamp\":\"2026-05-07T10:00:00Z\",\"message\":{{\"usage\":{{\"input_tokens\":{input}}}}}}}\n"
        )
}

#[test]
fn rereads_fold_only_appended_complete_lines() {
    use std::io::Write;
    let tmp = tempfile::tempdir().unwrap();
    let first = usage_line(10);
    let path = write_fixture(tmp.path(), "growing", &first);
    let cursors = UsageCursors::default();
    assert_eq!(cursors.read(&path).input_tokens, 10);
    assert_eq!(cursors.offset(&path), Some(first.len() as u64));

    let second = usage_line(5);
    let (complete, partial) = second.split_at(second.len() - 3);
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    file.write_all(complete.as_bytes()).unwrap();
    let held = cursors.read(&path);
    assert_eq!(held.input_tokens, 10);
    assert_eq!(cursors.offset(&path), Some(first.len() as u64));

    file.write_all(partial.as_bytes()).unwrap();
    let folded = cursors.read(&path);
    assert_eq!(folded.input_tokens, 15);
    assert_eq!(folded.message_count, 2);
    assert_eq!(
        cursors.offset(&path),
        Some((first.len() + second.len()) as u64)
    );
    assert_eq!(cursors.read(&path), folded);
}

#[test]
fn a_truncated_file_refolds_from_the_start() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_fixture(tmp.path(), "rewritten", &(usage_line(7) + &usage_line(8)));
    let cursors = UsageCursors::default();
    assert_eq!(cursors.read(&path).input_tokens, 15);
    std::fs::write(&path, usage_line(3)).unwrap();
    let snap = cursors.read(&path);
    assert_eq!(snap.input_tokens, 3);
    assert_eq!(snap.message_count, 1);
}
#[test]
fn equal_and_longer_rewrites_replace_usage_instead_of_double_counting() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_fixture(dir.path(), "rewrite", &usage_line(7));
    let cache = UsageCursors::default();
    assert_eq!(cache.read(&path).input_tokens, 7);
    std::fs::write(&path, usage_line(3)).unwrap();
    assert_eq!(cache.read(&path).input_tokens, 3);
    std::fs::write(&path, usage_line(8) + &usage_line(9)).unwrap();
    assert_eq!(cache.read(&path).input_tokens, 17);
    assert_eq!(cache.read(&path).message_count, 2);
}

#[test]
fn cursor_capacity_evicts_only_the_least_recently_used_file() {
    let dir = tempfile::tempdir().unwrap();
    let cache = UsageCursors::default();
    let paths: Vec<_> = (0..MAX_CURSORS + 1)
        .map(|index| write_fixture(dir.path(), &index.to_string(), &usage_line(1)))
        .collect();
    for path in &paths[..MAX_CURSORS] {
        cache.read(path);
    }
    cache.read(&paths[0]);
    cache.read(&paths[MAX_CURSORS]);
    assert_eq!(cache.cursors.lock().len(), MAX_CURSORS);
    assert!(cache.offset(&paths[0]).is_some());
    assert!(cache.offset(&paths[1]).is_none());
    assert!(cache.offset(&paths[2]).is_some());
}
