use super::*;
use std::io::Write;

fn write_file(name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("codeterm-jsonl-window-{name}"));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("session.jsonl");
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(bytes).unwrap();
    f.flush().unwrap();
    path
}

fn records(n: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for i in 0..n {
        out.extend_from_slice(
            format!("{{\"seq\":{i:06},\"pad\":\"{}\"}}\n", "x".repeat(64)).as_bytes(),
        );
    }
    out
}

#[test]
fn reads_whole_records_and_reports_offset() {
    let body = records(3);
    let path = write_file("whole", &body);
    let chunk = read_records_from(&path, 0, DELTA_CHUNK_BYTES).unwrap();
    assert_eq!(chunk.bytes, body);
    assert_eq!(chunk.new_offset, body.len() as u64);
    assert!(!chunk.more_available);
}

#[test]
fn holds_back_trailing_partial_line() {
    let mut body = records(2);
    let complete = body.len() as u64;
    body.extend_from_slice(b"{\"seq\":2,\"unterminated\":true");
    let path = write_file("partial", &body);

    let chunk = read_records_from(&path, 0, DELTA_CHUNK_BYTES).unwrap();
    assert_eq!(chunk.new_offset, complete);
    assert!(chunk.bytes.ends_with(b"\n"));
    // A second read from the new offset consumes nothing until the line lands.
    let again = read_records_from(&path, chunk.new_offset, DELTA_CHUNK_BYTES).unwrap();
    assert_eq!(again.new_offset, complete);
    assert!(again.bytes.is_empty());
}

#[test]
fn cap_splits_at_record_boundary_and_flags_more() {
    let body = records(64);
    let path = write_file("capped", &body);
    let line_len = body.iter().position(|&b| b == b'\n').unwrap() as u64 + 1;

    let first = read_records_from(&path, 0, line_len * 4 + 3).unwrap();
    assert!(first.more_available);
    assert_eq!(first.new_offset % line_len, 0, "must land on a boundary");
    assert!(first.bytes.ends_with(b"\n"));

    // Draining converges on the full file with no bytes lost or duplicated.
    let mut seen = first.bytes.clone();
    let mut off = first.new_offset;
    while off < body.len() as u64 {
        let next = read_records_from(&path, off, line_len * 4 + 3).unwrap();
        assert!(next.new_offset > off, "offset must advance");
        seen.extend_from_slice(&next.bytes);
        off = next.new_offset;
    }
    assert_eq!(seen, body);
}

#[test]
fn grows_past_cap_for_an_oversized_record() {
    let big = format!("{{\"blob\":\"{}\"}}\n", "y".repeat(300_000));
    let path = write_file("oversized", big.as_bytes());
    let chunk = read_records_from(&path, 0, 4096).unwrap();
    assert_eq!(chunk.bytes.len(), big.len());
    assert_eq!(chunk.new_offset, big.len() as u64);
}

#[test]
fn oversized_first_record_does_not_consume_the_following_page() {
    let first = format!("{{\"blob\":\"{}\"}}\n", "x".repeat(1100));
    let body = format!("{first}{}", String::from_utf8(records(30)).unwrap());
    let path = write_file("oversized-first-page", body.as_bytes());
    let chunk = read_records_from(&path, 0, 1024).unwrap();
    assert_eq!(chunk.new_offset, first.len() as u64);
    assert_eq!(chunk.bytes, first.as_bytes());
    assert!(chunk.more_available);
}

#[test]
fn backward_page_keeps_complete_records_within_its_byte_budget() {
    let path = write_file("backward-page", b"aaa\nbbb\nccc\npartial");
    assert_eq!(
        page_before(&path, 19, 8).unwrap(),
        RecordPage { start: 4, end: 12 }
    );
    assert_eq!(
        page_before(&path, 4, 8).unwrap(),
        RecordPage { start: 0, end: 4 }
    );
    assert_eq!(
        page_before(&path, 100, 4).unwrap(),
        RecordPage { start: 8, end: 12 }
    );
}

#[test]
fn backward_page_returns_one_oversized_last_record_without_older_rows() {
    let path = write_file("backward-oversized", b"old\nlong-record\n");
    assert_eq!(
        page_before(&path, 16, 4).unwrap(),
        RecordPage { start: 4, end: 16 }
    );
    let path = write_file("backward-single", b"long-record\n");
    assert_eq!(
        page_before(&path, 12, 4).unwrap(),
        RecordPage { start: 0, end: 12 }
    );
}

#[test]
fn offset_at_or_past_eof_is_a_noop() {
    let body = records(2);
    let path = write_file("eof", &body);
    let total = body.len() as u64;
    for off in [total, total + 4096] {
        let chunk = read_records_from(&path, off, DELTA_CHUNK_BYTES).unwrap();
        assert_eq!(chunk, JsonlChunk::held(off));
    }
}

#[test]
fn very_large_file_reads_a_bounded_tail_without_touching_the_head() {
    // ~40 MB of records: a whole-file read here is the production regression.
    let line = format!("{{\"pad\":\"{}\"}}\n", "z".repeat(4000));
    let per = line.len() as u64;
    let count = (40 * 1024 * 1024 / per) + 1;
    let mut body = Vec::with_capacity((per * count) as usize);
    for _ in 0..count {
        body.extend_from_slice(line.as_bytes());
    }
    let total = body.len() as u64;
    let path = write_file("huge", &body);

    let window = 1024 * 1024;
    let start = tail_window_start(&path, total, window);
    assert!(start > 0 && start < total);
    assert_eq!(
        start % per,
        0,
        "tail window must start on a record boundary"
    );
    assert!(total - start <= window);

    let chunk = read_records_from(&path, start, DELTA_CHUNK_BYTES).unwrap();
    assert_eq!(chunk.new_offset, total);
    assert!(chunk.bytes.len() as u64 <= window);
    assert!(chunk.bytes.starts_with(b"{\"pad\""));
}

#[test]
fn tail_window_start_returns_zero_when_window_covers_the_file() {
    let body = records(4);
    let path = write_file("small-window", &body);
    assert_eq!(tail_window_start(&path, body.len() as u64, 1 << 20), 0);
}

#[test]
fn read_range_clamps_to_the_file() {
    let body = records(4);
    let path = write_file("range", &body);
    let total = body.len() as u64;
    let (bytes, start) = read_range(&path, total - 10, total + 500).unwrap();
    assert_eq!(start, total - 10);
    assert_eq!(bytes, body[body.len() - 10..]);

    let (empty, start) = read_range(&path, total + 20, total + 40).unwrap();
    assert!(empty.is_empty());
    assert_eq!(start, total);
}
