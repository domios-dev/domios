use super::*;
use crate::pty::manager::OutputBuffer;

// ── cursor wire form ──

#[test]
fn cursor_round_trips_b64url() {
    let json = serde_json::json!({ "boot": "abc123", "tabs": { "p1": 42u64, "p2": 0u64 } });
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json.to_string());
    let c = parse_cursor(&raw).expect("decodes");
    assert_eq!(c.boot, "abc123");
    assert_eq!(c.tabs.get("p1"), Some(&42));
    assert_eq!(c.tabs.get("p2"), Some(&0));
}

#[test]
fn cursor_rejects_garbage() {
    assert!(parse_cursor("!!!not-base64!!!").is_none());
}

// ── since-resume: no byte loss or duplication across a retained offset ──

#[test]
fn resume_from_offset_yields_exact_tail_no_dup() {
    let mut ob = OutputBuffer::new(1 << 20);
    ob.append("p", b"hello ");
    ob.append("p", b"world");
    let total = ob.total_written("p");

    // Client resumes after "hello " (offset 6): must get exactly "world".
    let (chunk, t) = ob.read_since("p", 6);
    let r = plan_emit(6, &chunk, t, PER_TAB_TICK_CAP);
    assert_eq!(r.truncated_from, None);
    assert_eq!(r.output, b"world");
    assert_eq!(r.new_offset, total);

    // A second read from the advanced offset yields nothing (no duplication).
    let (chunk2, t2) = ob.read_since("p", r.new_offset);
    let r2 = plan_emit(r.new_offset, &chunk2, t2, PER_TAB_TICK_CAP);
    assert!(r2.output.is_empty());
    assert_eq!(r2.new_offset, total);

    // Fresh output after resume is delivered exactly once.
    ob.append("p", b"!");
    let (chunk3, t3) = ob.read_since("p", r2.new_offset);
    let r3 = plan_emit(r2.new_offset, &chunk3, t3, PER_TAB_TICK_CAP);
    assert_eq!(r3.output, b"!");
    assert_eq!(r3.new_offset, total + 1);
}

// ── under-retention truncation marker ──

#[test]
fn evicted_front_emits_truncated_from_oldest() {
    // total=8, but only the last 4 bytes are retained; a cursor at 0 lost 4.
    let r = plan_emit(0, b"4567", 8, PER_TAB_TICK_CAP);
    assert_eq!(r.truncated_from, Some(4));
    assert_eq!(r.output, b"4567");
    assert_eq!(r.new_offset, 8);
}

#[test]
fn retained_offset_has_no_truncation() {
    // Cursor at 4, retained chunk is exactly bytes 4..8 → no loss.
    let r = plan_emit(4, b"4567", 8, PER_TAB_TICK_CAP);
    assert_eq!(r.truncated_from, None);
    assert_eq!(r.new_offset, 8);
}

// ── fairness: per-tab byte cap, leftover carried to the next tick ──

#[test]
fn byte_cap_splits_across_ticks_without_loss() {
    let data: Vec<u8> = (0..100u8).collect();
    let total = 100u64;

    // Tick 1: cap=10 delivers the first 10 bytes only.
    let r1 = plan_emit(0, &data, total, 10);
    assert_eq!(r1.output.len(), 10);
    assert_eq!(r1.output, &data[..10]);
    assert_eq!(r1.new_offset, 10);

    // Tick 2: the retained chunk now starts at the cursor; next 10 bytes.
    let r2 = plan_emit(10, &data[10..], total, 10);
    assert_eq!(r2.output, &data[10..20]);
    assert_eq!(r2.new_offset, 20);
    assert_eq!(r2.truncated_from, None);
}

// ── UTF-8 boundary: the cap never splits a multibyte codepoint ──

#[test]
fn cap_backs_off_to_utf8_boundary() {
    // "aé" = [0x61, 0xC3, 0xA9]; cap=2 would split é — back off to 1 byte.
    let bytes = "aé".as_bytes();
    let r = plan_emit(0, bytes, bytes.len() as u64, 2);
    assert_eq!(r.output, b"a");
    assert_eq!(r.new_offset, 1);
    // Next tick delivers the whole codepoint.
    let r2 = plan_emit(1, &bytes[1..], bytes.len() as u64, 2);
    assert_eq!(r2.output, "é".as_bytes());
    assert_eq!(r2.new_offset, 3);
}

#[test]
fn empty_read_advances_to_total() {
    let r = plan_emit(5, b"", 5, PER_TAB_TICK_CAP);
    assert!(r.output.is_empty());
    assert_eq!(r.truncated_from, None);
    assert_eq!(r.new_offset, 5);
}
