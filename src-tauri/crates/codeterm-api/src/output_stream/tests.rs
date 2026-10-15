use super::*;
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::pty::manager::OutputBuffer;
use codeterm_service_core::service::test_fixture::ServiceFixture;
use std::sync::Arc;

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

fn output_buffer_state() -> codeterm_service_core::OutputBufferState {
    Arc::new(codeterm_service_core::lock_hold::TimedMutex::new(
        "output_buf",
        OutputBuffer::new(1 << 20),
    ))
}

#[tokio::test(start_paused = true)]
async fn per_tab_output_stream_wakes_on_pty_activity_without_a_tick() {
    use codeterm_service_core::service::output_stream_activity::{self, OutputStreamActivity};
    use futures_util::StreamExt;

    let activity = output_stream_activity::channel();
    let output = output_buffer_state();
    let stream = tab_output_frames(
        activity.subscribe(),
        output.clone(),
        Some("pty-x".into()),
        0,
    );
    futures_util::pin_mut!(stream);
    let started = tokio::time::Instant::now();

    assert!(futures_util::poll!(stream.next()).is_pending());
    activity
        .send(OutputStreamActivity::Pty("pty-other".into()))
        .unwrap();
    assert!(futures_util::poll!(stream.next()).is_pending());

    output.lock().append("pty-x", b"echoed");
    activity
        .send(OutputStreamActivity::Pty("pty-x".into()))
        .unwrap();
    assert_eq!(
        stream.next().await,
        Some(TabOutputFrame {
            output: "echoed".into(),
            offset: 6,
        })
    );
    assert_eq!(tokio::time::Instant::now(), started);
}

#[tokio::test(start_paused = true)]
async fn per_tab_output_stream_offsets_and_resume_are_exact() {
    use codeterm_service_core::service::output_stream_activity::{self, OutputStreamActivity};
    use futures_util::StreamExt;

    let activity = output_stream_activity::channel();
    let output = output_buffer_state();
    output.lock().append("pty-x", b"before!");
    let initial_total = 7;
    let offset = tab_output_start_offset(Some(2), initial_total);
    assert_eq!(
        tab_output_start_offset(Some(u64::MAX), initial_total),
        initial_total
    );
    assert_eq!(tab_output_start_offset(None, initial_total), initial_total);

    let stream = tab_output_frames(
        activity.subscribe(),
        output.clone(),
        Some("pty-x".into()),
        offset,
    );
    futures_util::pin_mut!(stream);
    assert_eq!(
        stream.next().await,
        Some(TabOutputFrame {
            output: "fore!".into(),
            offset: initial_total,
        })
    );
    assert!(futures_util::poll!(stream.next()).is_pending());

    let burst = vec![b'x'; (2 << 20) + 17];
    output.lock().append("pty-x", &burst);
    for _ in 0..4097 {
        activity
            .send(OutputStreamActivity::Pty("pty-x".into()))
            .unwrap();
    }

    let total = initial_total + burst.len() as u64;
    let retained_start = total - (2 << 20);
    let dropped_burst_bytes = (retained_start - initial_total) as usize;
    let rotated = stream.next().await.unwrap();
    assert_eq!(rotated.output.as_bytes(), &burst[dropped_burst_bytes..]);
    assert_eq!(rotated.offset, total);
    assert!(futures_util::poll!(stream.next()).is_pending());

    output.lock().append("pty-x", b"tail");
    activity
        .send(OutputStreamActivity::Pty("pty-x".into()))
        .unwrap();
    assert_eq!(
        stream.next().await,
        Some(TabOutputFrame {
            output: "tail".into(),
            offset: total + 4,
        })
    );
    assert!(futures_util::poll!(stream.next()).is_pending());
}

#[tokio::test]
async fn output_written_to_pty_wakes_subscriber_and_preserves_resume_offsets() {
    use codeterm_service_core::service::output_stream_activity::{self, OutputStreamActivity};

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
fn dirty_untracked_tab_output_before_mux_event_is_seeded_and_emitted() {
    let (mut mux, tab_id) = MultiplexerState::new();
    mux.get_tab_mut(&tab_id).unwrap().pty_id = Some("pty-new".into());
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .output_buf
        .lock()
        .append("pty-new", b"first");
    let service = Arc::new(fixture.service.clone());
    let mut tracked = HashMap::new();
    let dirty = HashSet::from(["pty-new".to_string()]);

    let (frames, starts, _) = sweep(&service, &mut tracked, None, &dirty, false);

    assert_eq!(starts.len(), 1);
    assert_eq!(starts[0].tab_id, tab_id);
    assert_eq!(starts[0].resume_offset, Some(0));
    assert!(frames.contains(&output_bytes_frame(
        &tab_id,
        "pty-new",
        service.events.boot_id(),
        b"first",
        5
    )));
    assert_eq!(
        tracked.get(&tab_id).map(|state| state.pty_id.as_str()),
        Some("pty-new")
    );
}

#[test]
fn replacement_pty_output_before_mux_event_starts_at_zero() {
    let (mut mux, tab_id) = MultiplexerState::new();
    mux.get_tab_mut(&tab_id).unwrap().pty_id = Some("pty-old".into());
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .pty_id = Some("pty-new".into());
    fixture
        .service
        .output_buf
        .lock()
        .append("pty-new", b"fresh");
    let service = Arc::new(fixture.service.clone());
    let mut tracked = HashMap::from([(
        tab_id.clone(),
        Tracked {
            pty_id: "pty-old".into(),
            offset: 7,
            ended: false,
        },
    )]);
    let dirty = HashSet::from(["pty-new".to_string()]);

    let (frames, starts, _) = sweep(&service, &mut tracked, None, &dirty, false);

    assert_eq!(starts.len(), 1);
    assert_eq!(starts[0].pty_id.as_deref(), Some("pty-new"));
    assert_eq!(starts[0].resume_offset, Some(0));
    assert!(frames.contains(&output_bytes_frame(
        &tab_id,
        "pty-new",
        service.events.boot_id(),
        b"fresh",
        5
    )));
    assert!(!frames.iter().any(|frame| frame.get("truncated").is_some()));
    assert_eq!(
        tracked.get(&tab_id).map(|state| state.pty_id.as_str()),
        Some("pty-new")
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

#[test]
fn chunk_ending_mid_codepoint_waits_for_the_rest() {
    // "a─" = [0x61, 0xE2, 0x94, 0x80]; a partial PTY read delivered only the first two bytes of "─".
    let bytes = "a─".as_bytes();
    let r = plan_emit(0, &bytes[..3], 3, PER_TAB_SWEEP_CAP);
    assert_eq!(r.output, b"a");
    assert_eq!(r.new_offset, 1);
    assert!(!r.has_more);
    let waiting = plan_emit(1, &bytes[1..3], 3, PER_TAB_SWEEP_CAP);
    assert!(waiting.output.is_empty());
    assert_eq!(waiting.new_offset, 1);
    let r2 = plan_emit(1, &bytes[1..], 4, PER_TAB_SWEEP_CAP);
    assert_eq!(r2.output, "─".as_bytes());
    assert_eq!(r2.new_offset, 4);
}

#[test]
fn every_split_of_a_box_drawing_line_decodes_without_replacement() {
    let line = "──┤ab├──".as_bytes();
    for split in 0..=line.len() {
        let first = plan_emit(0, &line[..split], split as u64, PER_TAB_SWEEP_CAP);
        let rest = plan_emit(
            first.new_offset,
            &line[first.new_offset as usize..],
            line.len() as u64,
            PER_TAB_SWEEP_CAP,
        );
        let mut out = first.output.clone();
        out.extend_from_slice(&rest.output);
        assert_eq!(
            String::from_utf8(out).as_deref(),
            Ok("──┤ab├──"),
            "split at {split}"
        );
    }
}

#[test]
fn limited_reads_preserve_emit_plans_at_every_utf8_boundary() {
    let mut buffer = OutputBuffer::new(16);
    let data = "a€😀z".repeat(PER_TAB_SWEEP_CAP / 3);
    buffer.append("p", data.as_bytes());
    let total = buffer.total_written("p");
    let mut offset = 0;
    while offset < total {
        let (whole, end) = buffer.read_since("p", offset);
        let expected = plan_emit(offset, &whole, end, PER_TAB_SWEEP_CAP);
        let chunk = buffer.read_since_limited("p", offset, PER_TAB_SWEEP_CAP);
        let actual = plan_emit_limited(offset, &chunk);
        assert_eq!(actual, expected);
        assert!(actual.new_offset > offset);
        offset = actual.new_offset;
    }
    buffer.append("p", &[0xf0, 0x9f]);
    let chunk = buffer.read_since_limited("p", offset, PER_TAB_SWEEP_CAP);
    let waiting = plan_emit_limited(offset, &chunk);
    assert!(waiting.output.is_empty());
    assert_eq!(waiting.new_offset, offset);
    buffer.append("p", &[0x98, 0x80]);
    let chunk = buffer.read_since_limited("p", offset, PER_TAB_SWEEP_CAP);
    assert_eq!(plan_emit_limited(offset, &chunk).output, "😀".as_bytes());
}

#[test]
fn limited_reads_preserve_malformed_boundary_and_truncation_plans() {
    let mut buffer = OutputBuffer::new(16);
    let mut bytes = vec![b'x'; 2 * 1024 * 1024 + 13];
    for boundary in [PER_TAB_SWEEP_CAP, 2 * PER_TAB_SWEEP_CAP] {
        bytes[13 + boundary] = 0x80;
        bytes[13 + boundary - 1] = b'a';
    }
    buffer.append("p", &bytes);
    let mut offset = 0;
    while offset < buffer.total_written("p") {
        let (whole, total) = buffer.read_since("p", offset);
        let expected = plan_emit(offset, &whole, total, PER_TAB_SWEEP_CAP);
        let chunk = buffer.read_since_limited("p", offset, PER_TAB_SWEEP_CAP);
        assert!(chunk.bytes.len() <= PER_TAB_SWEEP_CAP);
        let actual = plan_emit_limited(offset, &chunk);
        assert_eq!(actual, expected);
        assert!(actual.new_offset > offset);
        offset = actual.new_offset;
    }
}
