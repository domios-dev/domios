use super::*;

fn grid(rows: &[&str]) -> Vec<String> {
    rows.iter().map(|r| r.to_string()).collect()
}

#[test]
fn first_frame_is_still_baseline() {
    let mut decoder = MotionDecoder::new();
    let frame = decoder.step(&grid(&["hello", ""]));
    assert_eq!(frame.kind, MotionKind::Still);
    assert_eq!(frame.shift_up, 0);
    assert!(frame.promoted.is_empty());
}

#[test]
fn identical_frames_are_still() {
    let mut decoder = MotionDecoder::new();
    decoder.step(&grid(&["a", "b", ""]));
    let frame = decoder.step(&grid(&["a", "b", ""]));
    assert_eq!(frame.kind, MotionKind::Still);
    assert_eq!(frame.classes, RowClasses::default());
}

#[test]
fn scroll_up_promotes_exited_rows() {
    let mut decoder = MotionDecoder::new();
    decoder.step(&grid(&[
        "line A", "line B", "line C", "line D", "line E", "line F", "", "",
    ]));
    let frame = decoder.step(&grid(&[
        "line C", "line D", "line E", "line F", "line G", "line H", "", "",
    ]));
    assert_eq!(frame.kind, MotionKind::Scroll);
    assert_eq!(frame.shift_up, 2);
    assert_eq!(
        frame.promoted,
        vec!["line A".to_string(), "line B".to_string()]
    );
    assert_eq!(frame.classes.shifted, vec![0, 1, 2, 3]);
}

#[test]
fn tail_growth_is_streaming_append() {
    let mut decoder = MotionDecoder::new();
    decoder.step(&grid(&["❯ Hello", ""]));
    let frame = decoder.step(&grid(&["❯ Hello wor", ""]));
    assert_eq!(frame.kind, MotionKind::Stream);
    assert_eq!(frame.classes.appended, vec![0]);
}

#[test]
fn fresh_row_below_content_is_streaming_append() {
    let mut decoder = MotionDecoder::new();
    decoder.step(&grid(&["out1", "", ""]));
    let frame = decoder.step(&grid(&["out1", "out2", ""]));
    assert_eq!(frame.kind, MotionKind::Stream);
    assert_eq!(frame.classes.appended, vec![1]);
}

#[test]
fn repeated_in_place_mutation_becomes_status_churn() {
    let mut decoder = MotionDecoder::new();
    decoder.step(&grid(&["✻ Thinking (1s)", "", "", ""]));
    let first = decoder.step(&grid(&["✻ Thinking (2s)", "", "", ""]));
    assert_eq!(first.kind, MotionKind::Mixed);
    assert_eq!(first.classes.mutated, vec![0]);
    let second = decoder.step(&grid(&["✻ Thinking (3s)", "", "", ""]));
    assert_eq!(second.classes.mutated, vec![0]);
    let third = decoder.step(&grid(&["✻ Thinking (4s)", "", "", ""]));
    assert_eq!(third.kind, MotionKind::StatusChurn);
    assert_eq!(third.classes.volatile, vec![0]);
}

#[test]
fn volatile_region_persists_across_quiet_frames() {
    let mut decoder = MotionDecoder::new();
    decoder.step(&grid(&["✻ Thinking (1s)", "", "", ""]));
    decoder.step(&grid(&["✻ Thinking (2s)", "", "", ""]));
    decoder.step(&grid(&["✻ Thinking (3s)", "", "", ""]));
    decoder.step(&grid(&["✻ Thinking (4s)", "", "", ""]));
    let quiet = decoder.step(&grid(&["✻ Thinking (4s)", "", "", ""]));
    assert_eq!(quiet.kind, MotionKind::Still);
    assert_eq!(quiet.volatile_region, vec![0]);
}

#[test]
fn turn_buffer_freezes_only_post_boundary_rows() {
    let mut decoder = MotionDecoder::new();
    decoder.step(&grid(&["history one", "history two", "❯", "", "", ""]));
    decoder.begin_turn();

    let live = decoder.step(&grid(&[
        "history one",
        "history two",
        "❯",
        "turn a",
        "turn b",
        "turn c",
    ]));
    assert_eq!(live.turn_live, vec!["turn a", "turn b", "turn c"]);
    assert!(live.turn_frozen.is_empty());

    let first_scroll = decoder.step(&grid(&["❯", "turn a", "turn b", "turn c", "turn d", ""]));
    assert_eq!(first_scroll.shift_up, 2);
    assert_eq!(first_scroll.promoted, vec!["history one", "history two"]);
    assert!(
        first_scroll.turn_frozen.is_empty(),
        "pre-turn rows never freeze into the turn"
    );

    let second_scroll = decoder.step(&grid(&[
        "turn b", "turn c", "turn d", "turn e", "turn f", "",
    ]));
    assert_eq!(second_scroll.shift_up, 2);
    assert_eq!(second_scroll.turn_frozen, vec!["turn a"]);
    assert_eq!(
        second_scroll.turn_live,
        vec!["turn b", "turn c", "turn d", "turn e", "turn f"]
    );
}

#[test]
fn composer_draft_below_cursor_rule_never_leaks_into_turn_live() {
    let mut decoder = MotionDecoder::new();
    decoder.step(&grid(&["❯", "", "", "", "", ""]));
    decoder.begin_turn();
    let frame = decoder.step_with_cursor(
        &grid(&[
            "reply row one",
            "reply row two",
            "──────────",
            "❯ next prompt being typed",
            "──────────",
            "  ⏵⏵ auto mode on · esc to interrupt",
        ]),
        Some(3),
    );
    assert_eq!(frame.turn_live, vec!["reply row one", "reply row two"]);
}

#[test]
fn full_redraw_is_repaint_not_motion() {
    let mut decoder = MotionDecoder::new();
    decoder.step(&grid(&["a", "b", "c", "d", "e"]));
    let frame = decoder.step(&grid(&["v", "w", "x", "y", "z"]));
    assert_eq!(frame.kind, MotionKind::Repaint);
    assert_eq!(frame.shift_up, 0);
    assert!(frame.promoted.is_empty());
}
