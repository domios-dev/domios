use super::*;

fn none_hint() -> VolatileHint {
    VolatileHint { bottom_rows: 0 }
}

#[test]
fn first_step_has_no_delta_but_renders_grid() {
    let mut ex = SignalExtractor::new(10, 40, none_hint());
    let s = ex.step(b"hello world\r\n", 0);

    assert_eq!(s.rows, 10);
    assert_eq!(s.cols, 40);
    assert!(s.screen.lines.iter().any(|l| l.contains("hello world")));
    assert_eq!(s.screen.last_nonblank_row, Some(0));
    // No previous grid to diff against -> no delta, no false append.
    assert_eq!(s.delta.mode, DeltaMode::None);
    assert!(s.delta.changed_rows.is_empty());
    assert!(s.delta.appended_text.is_none());
    assert_eq!(s.delta.bytes_since_last, "hello world\r\n".len() as u64);
}

#[test]
fn append_below_prior_content_is_detected() {
    let mut ex = SignalExtractor::new(10, 40, none_hint());
    ex.step(b"line one\r\n", 0);
    let s = ex.step(b"line two\r\n", 100);

    assert_eq!(s.delta.mode, DeltaMode::Append);
    assert!(s.delta.changed_rows.contains(&1));
    assert_eq!(s.delta.appended_text.as_deref(), Some("line two"));
    assert_eq!(s.screen.last_nonblank_row, Some(1));
    let churn = s.delta.churn.expect("append has a churn bbox");
    assert_eq!(churn, RowRange { top: 1, bottom: 2 });
}

#[test]
fn in_place_overwrite_outside_volatile_is_inplace_not_append() {
    let mut ex = SignalExtractor::new(10, 40, none_hint());
    ex.step(b"alpha\r\nbeta\r\ngamma\r\n", 0);
    // Overwrite row 1 in place (cursor up to it) without growing the screen.
    let s = ex.step(b"\x1b[2;1HBETA ", 100);

    assert_eq!(s.delta.mode, DeltaMode::InPlace);
    assert!(s.delta.changed_rows.contains(&1));
    assert!(s.delta.appended_text.is_none());
}

#[test]
fn full_repaint_of_new_content_is_full_repaint() {
    let mut ex = SignalExtractor::new(6, 20, none_hint());
    ex.step(b"a\r\nb\r\nc\r\nd\r\ne\r\nf", 0);
    // Clear + paint a wholly different screen: > 60% of rows change.
    let s = ex.step(b"\x1b[2J\x1b[HZ\r\nY\r\nX\r\nW\r\nV\r\nU", 100);

    assert_eq!(s.delta.mode, DeltaMode::FullRepaint);
}

#[test]
fn byte_clock_resets_on_any_byte_semantic_clock_only_outside_volatile() {
    // Bottom row is the plugin-declared volatile spinner band.
    let mut ex = SignalExtractor::new(5, 40, VolatileHint { bottom_rows: 1 });
    ex.step(b"answer text\r\n\x1b[5;1H| working", 0);

    // t=100: only the spinner glyph in the bottom row changes.
    let s1 = ex.step(b"\x1b[5;1H/ working", 100);
    assert_eq!(s1.quiescence.byte_idle_ms, 0, "any byte resets byte clock");
    assert_eq!(
        s1.quiescence.semantic_idle_ms, 100,
        "volatile-only change must not reset semantic clock"
    );
    assert!(s1.quiescence.volatile_only);
    assert_eq!(s1.quiescence.kind, Motion::Animating);

    // t=200: spinner ticks again -> semantic idle keeps climbing.
    let s2 = ex.step(b"\x1b[5;1H- working", 200);
    assert_eq!(s2.quiescence.byte_idle_ms, 0);
    assert_eq!(s2.quiescence.semantic_idle_ms, 200);
    assert!(s2.quiescence.volatile_only);

    // t=300: real output OUTSIDE the volatile region resets BOTH clocks.
    let s3 = ex.step(b"\x1b[1;1Hnew answer line\r\n", 300);
    assert_eq!(s3.quiescence.byte_idle_ms, 0);
    assert_eq!(s3.quiescence.semantic_idle_ms, 0);
    assert!(!s3.quiescence.volatile_only);
}

#[test]
fn no_bytes_grows_byte_clock_and_yields_still() {
    let mut ex = SignalExtractor::new(5, 40, none_hint());
    ex.step(b"final answer\r\n", 0);
    // Tick the clock with an empty chunk: nothing emitted.
    let s = ex.step(b"", 800);

    assert_eq!(s.quiescence.byte_idle_ms, 800);
    assert_eq!(s.quiescence.semantic_idle_ms, 800);
    assert_eq!(s.delta.mode, DeltaMode::None);
    assert_eq!(s.quiescence.kind, Motion::Still);
}

#[test]
fn append_of_assistant_text_is_streaming() {
    let mut ex = SignalExtractor::new(10, 40, VolatileHint { bottom_rows: 1 });
    ex.step(b"prompt\r\n", 0);
    let s = ex.step(b"streamed reply\r\n", 50);

    assert_eq!(s.delta.mode, DeltaMode::Append);
    assert_eq!(s.quiescence.kind, Motion::Streaming);
    assert_eq!(s.quiescence.byte_idle_ms, 0);
    assert_eq!(s.quiescence.semantic_idle_ms, 0);
}

#[test]
fn resize_resets_prev_grid_so_repaint_is_not_a_delta() {
    let mut ex = SignalExtractor::new(10, 40, none_hint());
    ex.step(b"stable content here\r\n", 0);
    // Time passes with no output: semantic clock climbs.
    ex.step(b"", 500);

    ex.resize(12, 50);
    // App repaints the SAME logical content after SIGWINCH.
    let s = ex.step(b"\x1b[2J\x1b[Hstable content here\r\n", 1000);

    assert_eq!(s.rows, 12);
    assert_eq!(s.cols, 50);
    // No previous grid -> repaint must not register as a content delta.
    assert_eq!(s.delta.mode, DeltaMode::None);
    assert!(s.delta.changed_rows.is_empty());
    // Resize is not progress: the semantic clock is preserved, not reset.
    assert_eq!(s.quiescence.semantic_idle_ms, 1000);
}

#[test]
fn now_ms_determinism_same_inputs_same_signals() {
    let script: &[(&[u8], u64)] = &[
        (b"boot\r\n", 0),
        (b"\x1b[5;1H. spin", 40),
        (b"\x1b[5;1H.. spin", 80),
        (b"more output\r\n", 120),
        (b"", 600),
    ];
    let run = || {
        let mut ex = SignalExtractor::new(8, 40, VolatileHint { bottom_rows: 1 });
        script
            .iter()
            .map(|(c, t)| ex.step(c, *t))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        run(),
        run(),
        "same inputs + timestamps -> identical signals"
    );
}

#[test]
fn empty_screen_has_no_nonblank_row() {
    let mut ex = SignalExtractor::new(5, 20, none_hint());
    let s = ex.step(b"", 0);

    assert_eq!(s.screen.last_nonblank_row, None);
    assert!(s.tail.trim().is_empty());
    assert_eq!(s.delta.mode, DeltaMode::None);
}

#[test]
fn partial_line_then_pause_distinguishes_typing_from_done() {
    let mut ex = SignalExtractor::new(6, 40, none_hint());
    ex.step(b"$ ", 0);
    // A partial line is typed (no newline) on the input row.
    let typed = ex.step(b"git comm", 100);
    assert_eq!(typed.delta.mode, DeltaMode::Append);
    assert!(typed.cursor.on_last_nonblank_row);
    assert_eq!(typed.screen.last_nonblank_row, Some(0));

    // Then the user pauses mid-line: no bytes, cursor still parked on the line.
    let paused = ex.step(b"", 900);
    assert_eq!(paused.delta.mode, DeltaMode::None);
    assert!(paused.cursor.on_last_nonblank_row);
    assert_eq!(paused.quiescence.byte_idle_ms, 800);
}

#[test]
fn cursor_position_and_visibility_tracked() {
    let mut ex = SignalExtractor::new(6, 40, none_hint());
    let s = ex.step(b"abc", 0);
    assert_eq!(s.cursor.row, 0);
    assert_eq!(s.cursor.col, 3);
    assert!(s.cursor.visible);

    let hidden = ex.step(b"\x1b[?25l", 50);
    assert!(!hidden.cursor.visible);
}

#[test]
fn cursor_idle_clock_resets_on_move() {
    let mut ex = SignalExtractor::new(6, 40, none_hint());
    ex.step(b"x", 0);
    // No output at all -> cursor has not moved.
    let still = ex.step(b"", 300);
    assert_eq!(still.quiescence.cursor_idle_ms, 300);

    // Cursor advances -> idle resets.
    let moved = ex.step(b"y", 400);
    assert_eq!(moved.quiescence.cursor_idle_ms, 0);
}

#[test]
fn screen_hash_stable_across_repaint_of_identical_content() {
    let mut ex = SignalExtractor::new(6, 30, none_hint());
    let a = ex.step(b"deterministic line\r\n", 0);
    // Cursor-addressed repaint resolving to the same grid.
    let b = ex.step(b"\x1b[2J\x1b[Hdeterministic line\r\n", 100);
    assert_eq!(a.screen.screen_hash, b.screen.screen_hash);
}

#[test]
fn region_model_splits_content_and_volatile_bands() {
    let mut ex = SignalExtractor::new(10, 40, VolatileHint { bottom_rows: 3 });
    let s = ex.step(b"top content\r\n", 0);
    assert_eq!(s.regions.content, RowRange { top: 0, bottom: 7 });
    assert_eq!(s.regions.volatile, RowRange { top: 7, bottom: 10 });
    assert!(s.regions.content_text.contains("top content"));
}

#[test]
fn input_mode_tracks_cursor_hidden() {
    let mut ex = SignalExtractor::new(6, 40, none_hint());
    let shown = ex.step(b"abc", 0);
    assert!(!shown.input_mode.cursor_hidden);

    let hidden = ex.step(b"\x1b[?25l", 50);
    assert!(hidden.input_mode.cursor_hidden);

    let reshown = ex.step(b"\x1b[?25h", 100);
    assert!(!reshown.input_mode.cursor_hidden);
}

#[test]
fn input_mode_tracks_alt_screen() {
    let mut ex = SignalExtractor::new(6, 40, none_hint());
    assert!(!ex.step(b"shell\r\n", 0).input_mode.alt_screen);
    assert!(ex.step(b"\x1b[?1049h", 50).input_mode.alt_screen);
    assert!(!ex.step(b"\x1b[?1049l", 100).input_mode.alt_screen);
}

#[test]
fn input_mode_tracks_bracketed_paste() {
    let mut ex = SignalExtractor::new(6, 40, none_hint());
    assert!(!ex.step(b"x", 0).input_mode.bracketed_paste);
    assert!(ex.step(b"\x1b[?2004h", 50).input_mode.bracketed_paste);
    assert!(!ex.step(b"\x1b[?2004l", 100).input_mode.bracketed_paste);
}

#[test]
fn private_mode_split_across_chunks_is_reassembled() {
    let mut ex = SignalExtractor::new(6, 40, none_hint());
    let mid = ex.step(b"\x1b[?2", 0);
    assert!(
        !mid.input_mode.cursor_hidden,
        "incomplete sequence -> no mode change yet"
    );
    let done = ex.step(b"5l", 50);
    assert!(done.input_mode.cursor_hidden, "completed across chunks");
}

#[test]
fn picker_inactive_on_alt_screen_alone() {
    let mut ex = SignalExtractor::new(6, 40, none_hint());
    let s = ex.step(b"\x1b[?1049h\x1b[2J\x1b[Hmenu\x1b[?25l", 0);
    assert!(s.input_mode.alt_screen);
    assert!(s.input_mode.cursor_hidden);
    assert!(
        !s.picker_active,
        "newly drawn alt-screen TUI must not read as a picker"
    );
    let quiescent = ex.step(b"", 800);
    assert_eq!(quiescent.quiescence.kind, Motion::Still);
    assert!(
        !quiescent.picker_active,
        "parked full-screen TUI with a hidden cursor must not read as a picker"
    );
}

#[test]
fn picker_active_on_cursor_hidden_and_still() {
    let mut ex = SignalExtractor::new(6, 40, none_hint());
    // Inline picker hides the cursor and parks on a menu.
    ex.step(b"Pick one:\r\n> a\r\n  b\x1b[?25l", 0);
    let parked = ex.step(b"", 500);
    assert!(parked.input_mode.cursor_hidden);
    assert_eq!(parked.quiescence.kind, Motion::Still);
    assert!(!parked.input_mode.bracketed_paste);
    assert!(parked.picker_active);
}

#[test]
fn picker_inactive_while_spinner_churns_cursor_hidden() {
    let mut ex = SignalExtractor::new(6, 40, none_hint());
    // A spinner also hides the cursor, but it keeps churning -> not Still.
    ex.step(b"\x1b[?25l| working", 0);
    let spinning = ex.step(b"\x1b[1;1H/ working", 100);
    assert!(spinning.input_mode.cursor_hidden);
    assert_ne!(spinning.quiescence.kind, Motion::Still);
    assert!(!spinning.picker_active);
}

#[test]
fn picker_inactive_on_empty_hidden_cursor_boot() {
    let mut ex = SignalExtractor::new(6, 40, none_hint());
    ex.step(b"\x1b[?25l", 0);
    let idle = ex.step(b"", 500);
    assert!(idle.input_mode.cursor_hidden);
    assert_eq!(idle.quiescence.kind, Motion::Still);
    assert!(!idle.picker_active);
}

#[test]
fn picker_inactive_for_shell_prompt_bracketed_paste_only() {
    let mut ex = SignalExtractor::new(6, 40, none_hint());
    // Normal shell line prompt: bracketed-paste on, cursor visible.
    ex.step(b"\x1b[?2004h$ ", 0);
    let idle = ex.step(b"", 500);
    assert!(idle.input_mode.bracketed_paste);
    assert!(!idle.input_mode.cursor_hidden);
    assert_eq!(idle.quiescence.kind, Motion::Still);
    assert!(!idle.picker_active);
}

#[test]
fn picker_inactive_when_bracketed_paste_even_if_cursor_hidden_and_still() {
    let mut ex = SignalExtractor::new(6, 40, none_hint());
    // transiently hidden must NOT read as a picker.
    ex.step(b"\x1b[?2004h\x1b[?25l$ ", 0);
    let idle = ex.step(b"", 500);
    assert!(idle.input_mode.bracketed_paste);
    assert!(idle.input_mode.cursor_hidden);
    assert_eq!(idle.quiescence.kind, Motion::Still);
    assert!(
        !idle.picker_active,
        "bracketed-paste excludes the inline branch"
    );
}

#[test]
fn picker_active_clears_when_alt_screen_exits() {
    let mut ex = SignalExtractor::new(6, 40, none_hint());
    assert!(!ex.step(b"\x1b[?1049h\x1b[Hmenu", 0).picker_active);
    let exited = ex.step(b"\x1b[?1049l", 100);
    assert!(!exited.input_mode.alt_screen);
    assert!(!exited.picker_active);
}

#[test]
fn signals_round_trip_through_json() {
    let mut ex = SignalExtractor::new(6, 30, VolatileHint { bottom_rows: 1 });
    ex.step(b"first\r\n", 0);
    let s = ex.step(b"second\r\n", 50);
    let json = serde_json::to_string(&s).expect("serialize");
    let back: StreamSignals = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(s, back);
}
