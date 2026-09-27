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
    let r = plan_emit(6, &chunk, t, PER_TAB_SWEEP_CAP);
    assert_eq!(r.truncated_from, None);
    assert_eq!(r.output, b"world");
    assert_eq!(r.new_offset, total);

    // A second read from the advanced offset yields nothing (no duplication).
    let (chunk2, t2) = ob.read_since("p", r.new_offset);
    let r2 = plan_emit(r.new_offset, &chunk2, t2, PER_TAB_SWEEP_CAP);
    assert!(r2.output.is_empty());
    assert_eq!(r2.new_offset, total);

    // Fresh output after resume is delivered exactly once.
    ob.append("p", b"!");
    let (chunk3, t3) = ob.read_since("p", r2.new_offset);
    let r3 = plan_emit(r2.new_offset, &chunk3, t3, PER_TAB_SWEEP_CAP);
    assert_eq!(r3.output, b"!");
    assert_eq!(r3.new_offset, total + 1);
}

// ── under-retention truncation marker ──

#[test]
fn evicted_front_emits_truncated_from_oldest() {
    // total=8, but only the last 4 bytes are retained; a cursor at 0 lost 4.
    let r = plan_emit(0, b"4567", 8, PER_TAB_SWEEP_CAP);
    assert_eq!(r.truncated_from, Some(4));
    assert_eq!(r.output, b"4567");
    assert_eq!(r.new_offset, 8);
}

#[test]
fn retained_offset_has_no_truncation() {
    // Cursor at 4, retained chunk is exactly bytes 4..8 → no loss.
    let r = plan_emit(4, b"4567", 8, PER_TAB_SWEEP_CAP);
    assert_eq!(r.truncated_from, None);
    assert_eq!(r.new_offset, 8);
}

// ── fairness: per-tab byte cap, leftover carried to the next sweep ──

#[test]
fn byte_cap_splits_across_sweeps_without_loss() {
    let data: Vec<u8> = (0..100u8).collect();
    let total = 100u64;

    // Sweep 1: cap=10 delivers the first 10 bytes only.
    let r1 = plan_emit(0, &data, total, 10);
    assert_eq!(r1.output.len(), 10);
    assert_eq!(r1.output, &data[..10]);
    assert_eq!(r1.new_offset, 10);
    assert!(r1.has_more);

    // Sweep 2: the retained chunk now starts at the cursor; next 10 bytes.
    let r2 = plan_emit(10, &data[10..], total, 10);
    assert_eq!(r2.output, &data[10..20]);
    assert_eq!(r2.new_offset, 20);
    assert_eq!(r2.truncated_from, None);

    let last = plan_emit(90, &data[90..], total, 10);
    assert_eq!(last.output, &data[90..]);
    assert_eq!(last.new_offset, total);
    assert!(!last.has_more);
}

// ── UTF-8 boundary: the cap never splits a multibyte codepoint ──

#[test]
fn cap_backs_off_to_utf8_boundary() {
    // "aé" = [0x61, 0xC3, 0xA9]; cap=2 would split é — back off to 1 byte.
    let bytes = "aé".as_bytes();
    let r = plan_emit(0, bytes, bytes.len() as u64, 2);
    assert_eq!(r.output, b"a");
    assert_eq!(r.new_offset, 1);
    // Next sweep delivers the whole codepoint.
    let r2 = plan_emit(1, &bytes[1..], bytes.len() as u64, 2);
    assert_eq!(r2.output, "é".as_bytes());
    assert_eq!(r2.new_offset, 3);
}

#[test]
fn empty_read_advances_to_total() {
    let r = plan_emit(5, b"", 5, PER_TAB_SWEEP_CAP);
    assert!(r.output.is_empty());
    assert_eq!(r.truncated_from, None);
    assert_eq!(r.new_offset, 5);
}

#[test]
fn tab_scoped_stream_excludes_cross_tab_output_server_side() {
    assert!(tab_is_in_scope("owned", Some("owned")));
    assert!(!tab_is_in_scope("other", Some("owned")));
    assert!(tab_is_in_scope("other", None));
}

#[test]
fn output_stream_builds_no_diagnostics_when_level_is_none() {
    let diagnostics =
        build_stream_diagnostics(None, Some("unused-session".into()), "*".into(), None, None);
    assert!(diagnostics.is_none());
}

#[tokio::test]
async fn output_written_to_pty_wakes_subscriber_and_preserves_resume_offsets() {
    use crate::service::output_stream_activity::{self, OutputStreamActivity};

    let activity = output_stream_activity::channel();
    let mut subscriber = activity.subscribe();
    let mut output = OutputBuffer::new(1 << 20);

    output.append("pty-x", b"first");
    activity
        .send(OutputStreamActivity::Pty("pty-x".into()))
        .unwrap();
    assert_eq!(
        subscriber.recv().await.unwrap(),
        OutputStreamActivity::Pty("pty-x".into())
    );
    let (chunk, total) = output.read_since("pty-x", 0);
    let first = plan_emit(0, &chunk, total, PER_TAB_SWEEP_CAP);
    assert_eq!(first.output, b"first");
    assert_eq!(first.new_offset, 5);

    output.append("pty-x", b" second");
    activity
        .send(OutputStreamActivity::Pty("pty-x".into()))
        .unwrap();
    assert_eq!(
        subscriber.recv().await.unwrap(),
        OutputStreamActivity::Pty("pty-x".into())
    );
    let (chunk, total) = output.read_since("pty-x", first.new_offset);
    let resumed = plan_emit(first.new_offset, &chunk, total, PER_TAB_SWEEP_CAP);
    assert_eq!(resumed.output, b" second");
    assert_eq!(resumed.new_offset, 12);
}

#[test]
fn sweep_reads_only_tabs_with_dirty_ptys() {
    let live = HashMap::from([
        ("tab-x".to_string(), "pty-x".to_string()),
        ("tab-y".to_string(), "pty-y".to_string()),
    ]);
    let dirty = HashSet::from(["pty-x".to_string()]);

    assert_eq!(
        selected_tabs(&live, &dirty, &HashSet::new(), false),
        vec![("tab-x".to_string(), "pty-x".to_string())]
    );
    assert_eq!(
        selected_tabs(&live, &dirty, &HashSet::new(), true).len(),
        2,
        "a broadcast lag still triggers a full output rescan"
    );
}

#[test]
fn mux_refresh_reads_only_new_or_replaced_ptys() {
    let live = HashMap::from([
        ("tab-x".to_string(), "pty-x".to_string()),
        ("tab-y".to_string(), "pty-y".to_string()),
    ]);
    let changed = HashSet::from(["tab-y".to_string()]);

    assert_eq!(
        selected_tabs(&live, &HashSet::new(), &changed, false),
        vec![("tab-y".to_string(), "pty-y".to_string())]
    );
}

#[test]
fn seed_initial_without_cursor_emits_tab_boot_frame_and_starts_at_zero() {
    let (boot_frame, tracked, starts) = seed_initial_state(
        "boot-current".into(),
        None,
        Some("tab-1"),
        vec![InitialTab {
            tab_id: "tab-1".into(),
            pty_id: "pty-1".into(),
            total_written: 42,
            retained_from: 4,
        }],
    );

    assert_eq!(
        boot_frame,
        Some(serde_json::json!({ "boot": "boot-current" }))
    );
    assert_eq!(tracked.get("tab-1").map(|tab| tab.offset), Some(0));
    assert_eq!(starts[0].resume_offset, Some(0));
    assert_eq!(starts[0].replay_truncated_from, Some(4));
    assert!(!starts[0].boot_changed);

    let (boot_frame, tracked, starts) = seed_initial_state(
        "boot-current".into(),
        None,
        None,
        vec![InitialTab {
            tab_id: "tab-1".into(),
            pty_id: "pty-1".into(),
            total_written: 42,
            retained_from: 4,
        }],
    );
    assert_eq!(boot_frame, None);
    assert_eq!(tracked.get("tab-1").map(|tab| tab.offset), Some(42));
    assert_eq!(starts[0].resume_offset, Some(42));
    assert_eq!(starts[0].replay_truncated, Some(false));
}

#[test]
fn seed_initial_same_boot_resumes_cursor_without_boot_frame() {
    let mut tabs = HashMap::new();
    tabs.insert("tab-1".into(), 17);
    let (boot_frame, tracked, starts) = seed_initial_state(
        "boot-current".into(),
        Some(Cursor {
            boot: "boot-current".into(),
            tabs,
        }),
        Some("tab-1"),
        vec![InitialTab {
            tab_id: "tab-1".into(),
            pty_id: "pty-1".into(),
            total_written: 42,
            retained_from: 4,
        }],
    );

    assert_eq!(boot_frame, None);
    assert_eq!(tracked.get("tab-1").map(|tab| tab.offset), Some(17));
    assert_eq!(starts[0].resume_offset, Some(17));
    assert_eq!(starts[0].replay_truncated, Some(false));
    assert!(!starts[0].boot_changed);
}

#[test]
fn seed_initial_different_boot_emits_boot_changed_and_starts_at_tail() {
    let mut tabs = HashMap::new();
    tabs.insert("tab-1".into(), 17);
    let (boot_frame, tracked, starts) = seed_initial_state(
        "boot-current".into(),
        Some(Cursor {
            boot: "boot-old".into(),
            tabs,
        }),
        Some("tab-1"),
        vec![InitialTab {
            tab_id: "tab-1".into(),
            pty_id: "pty-1".into(),
            total_written: 42,
            retained_from: 4,
        }],
    );

    assert_eq!(
        boot_frame,
        Some(serde_json::json!({ "boot_changed": true, "boot": "boot-current" }))
    );
    assert_eq!(tracked.get("tab-1").map(|tab| tab.offset), Some(42));
    assert_eq!(starts[0].resume_offset, Some(42));
    assert_eq!(starts[0].replay_truncated, Some(false));
    assert!(starts[0].boot_changed);
}
