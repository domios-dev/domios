use super::*;

#[test]
fn stored_window_holds_a_partial_tail_and_reads_only_complete_records() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    std::fs::write(&path, b"{\"id\":1}\n{\"id\":2}\n{\"id\":").unwrap();
    let (records, start, end) = read_window(&path, None).unwrap();
    assert_eq!(start, 0);
    assert_eq!(end, 18);
    assert_eq!(records, "{\"id\":1}\n{\"id\":2}\n");
    assert_eq!(
        read_window(&path, Some(9)).unwrap(),
        ("{\"id\":1}\n".into(), 0, 9)
    );
}

#[test]
fn stored_window_is_bounded_and_keeps_an_exact_backfill_boundary() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    let record = format!("{{\"opaque\":\"{}\"}}\n", "x".repeat(1024));
    let mut file = std::fs::File::create(&path).unwrap();
    use std::io::Write;
    for _ in 0..6000 {
        file.write_all(record.as_bytes()).unwrap();
    }
    drop(file);
    let (records, start, end) = read_window(&path, None).unwrap();
    assert!(start > 0);
    assert!(records.len() as u64 <= WINDOW_BYTES);
    assert_eq!(start % record.len() as u64, 0);
    assert_eq!(end, std::fs::metadata(&path).unwrap().len());
    let (_, previous_start, previous_end) = read_window(&path, Some(start)).unwrap();
    assert_eq!(previous_end, start);
    assert!(previous_start < start);
}

#[test]
fn on_demand_stored_windows_reach_first_record_without_gaps() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.jsonl");
    let record = format!("{{\"content\":\"{}\"}}\n", "x".repeat(1024));
    use std::io::Write;
    let mut file = std::fs::File::create(&path).unwrap();
    file.write_all(b"{\"id\":\"first\"}\n").unwrap();
    for _ in 0..3000 {
        file.write_all(record.as_bytes()).unwrap();
    }
    drop(file);
    let mut cursor = std::fs::metadata(&path).unwrap().len();
    let mut pages = 0;
    while cursor > 0 {
        let (records, start, end) = read_window(&path, Some(cursor)).unwrap();
        assert_eq!(end, cursor);
        assert!(start < cursor);
        assert!(records.len() as u64 <= WINDOW_BYTES);
        if start == 0 {
            assert!(records.starts_with("{\"id\":\"first\"}\n"));
        }
        cursor = start;
        pages += 1;
    }
    assert!(pages > 2);
}
