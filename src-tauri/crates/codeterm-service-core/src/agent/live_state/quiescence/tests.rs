use super::*;
use crate::agent::classify_input::{ClassifyInputBuilder, ClassifyParts, StatusBarRows};
use crate::agent::offline_state_classifier::{
    classify_state, extract_status_line, StatusElapsedTracker, TabVerdict, TimelineState,
};
use crate::agent::provider::ComposerCursorPosition;
use crate::agent::signal_extractor::Motion;

struct Frame {
    at: u64,
    semantic_change: bool,
    lines: Vec<String>,
    picker: Option<&'static str>,
    composer_ready: bool,
}

impl Frame {
    fn output(at: u64, rows: &[&str]) -> Self {
        Self {
            at,
            semantic_change: true,
            lines: rows.iter().map(|row| row.to_string()).collect(),
            picker: None,
            composer_ready: false,
        }
    }
}

fn verdict(state: TimelineState) -> TabVerdict {
    TabVerdict {
        state,
        subtype: None,
    }
}

fn classify(
    frame: &Frame,
    now: u64,
    semantic_at: u64,
    previous: TabVerdict,
    prompt_line: bool,
    elapsed: &mut StatusElapsedTracker,
) -> TabVerdict {
    let mut builder = ClassifyInputBuilder::for_rows(StatusBarRows::fixture(0));
    if prompt_line {
        builder = builder.with_composer_position(ComposerCursorPosition::PromptLine);
    }
    classify_state(
        &builder.build(ClassifyParts {
            picker_question: frame.picker,
            screen_lines: &frame.lines,
            tail: "",
            byte_idle_ms: now - frame.at,
            semantic_idle_ms: now - semantic_at,
            motion: Motion::Still,
            cursor_hidden: !frame.composer_ready,
            composer_ready: frame.composer_ready,
            status_elapsed: elapsed
                .observe(extract_status_line(&frame.lines).and_then(|s| s.elapsed_ms)),
        }),
        previous,
    )
}

fn assert_equivalent(
    frames: &[Frame],
    initial: TimelineState,
    prompt_line: bool,
) -> Vec<TabVerdict> {
    let mut swept = verdict(initial);
    let mut event = swept;
    let mut frame_index = 0;
    let mut semantic_at = 0;
    let mut deadline = None;
    let mut states = Vec::new();
    let mut swept_elapsed = StatusElapsedTracker::new();
    let mut event_elapsed = StatusElapsedTracker::new();
    for now in 0..=7000 {
        let output = frames
            .get(frame_index + 1)
            .is_some_and(|frame| frame.at == now);
        if output {
            frame_index += 1;
        }
        let frame = &frames[frame_index];
        if frame.at == now && frame.semantic_change {
            semantic_at = now;
        }
        // Evaluate the old sweep's classifier at every virtual millisecond to
        // compare boundary semantics independently of the old 350ms lateness.
        swept = classify(
            frame,
            now,
            semantic_at,
            swept,
            prompt_line,
            &mut swept_elapsed,
        );
        if now == 0 || output || deadline == Some(now) {
            event = classify(
                frame,
                now,
                semantic_at,
                event,
                prompt_line,
                &mut event_elapsed,
            );
            deadline = next_delay(now - frame.at, now - semantic_at)
                .map(|delay| now + delay.as_millis() as u64);
        }
        assert_eq!(event, swept, "at {now}ms, frame {frame_index}");
        states.push(event);
    }
    states
}

#[test]
fn event_classification_matches_sweep_at_quiescence_boundaries() {
    let ready = assert_equivalent(&[Frame::output(0, &["❯ "])], TimelineState::Working, false);
    assert_eq!(ready[799].state, TimelineState::Working);
    assert_eq!(ready[800].state, TimelineState::Working);
    assert_eq!(ready[1499].state, TimelineState::Working);
    assert_eq!(ready[1500].state, TimelineState::Idle);
    let silent = assert_equivalent(&[Frame::output(0, &[""])], TimelineState::Working, false);
    assert_eq!(silent[4999].state, TimelineState::Working);
    assert_eq!(silent[5000].state, TimelineState::Idle);
    let prompt = assert_equivalent(&[Frame::output(0, &[""])], TimelineState::Idle, true);
    assert_eq!(prompt[800].state, TimelineState::Working);
    assert_eq!(prompt[5000].state, TimelineState::Idle);
}

#[test]
fn event_classification_matches_sweep_across_output_and_picker_changes() {
    let mut repaint = Frame::output(700, &[""]);
    repaint.semantic_change = false;
    let states = assert_equivalent(
        &[Frame::output(0, &[""]), repaint],
        TimelineState::Working,
        false,
    );
    assert_eq!(states[5000].state, TimelineState::Working);
    assert_eq!(states[5700].state, TimelineState::Idle);
    let mut picker = Frame::output(2000, &[""]);
    picker.picker = Some("fixture question");
    let states = assert_equivalent(
        &[
            Frame::output(0, &["❯ "]),
            picker,
            Frame::output(3000, &["❯ "]),
        ],
        TimelineState::Working,
        false,
    );
    assert_eq!(states[2000].state, TimelineState::Question);
    assert_eq!(states[3000].state, TimelineState::Idle);
    let mut composer = Frame::output(0, &[""]);
    composer.composer_ready = true;
    let states = assert_equivalent(&[composer], TimelineState::Working, true);
    assert_eq!(states[0].state, TimelineState::Idle);
}

#[test]
fn status_repaints_and_elapsed_progress_match_sweep() {
    let mut advance = Frame::output(200, &["✽ Xyzzying… (2s · ↓ 5 tokens)", "❯"]);
    advance.semantic_change = false;
    let states = assert_equivalent(
        &[
            Frame::output(0, &["✽ Xyzzying… (1s · ↓ 5 tokens)", "❯"]),
            advance,
        ],
        TimelineState::Working,
        false,
    );
    assert_eq!(states[5199].state, TimelineState::Working);
    assert_eq!(states[5200].state, TimelineState::Idle);
    let mut repaint = Frame::output(1000, &["✽ Xyzzying… (1s · ↓ 5 tokens)", "❯"]);
    repaint.semantic_change = false;
    let states = assert_equivalent(
        &[
            Frame::output(0, &["✽ Xyzzying… (1s · ↓ 5 tokens)", "❯"]),
            repaint,
        ],
        TimelineState::Working,
        false,
    );
    assert_eq!(states[1799].state, TimelineState::Working);
    assert_eq!(states[1800].state, TimelineState::Idle);
}

#[test]
fn deadlines_coalesce_output_and_park_after_final_boundary() {
    let now = Instant::now();
    let mut deadlines = Deadlines::default();
    for index in 0..1000 {
        deadlines.set("one", Some(now + Duration::from_millis(index)));
    }
    assert_eq!(deadlines.0.len(), 1);
    assert!(deadlines
        .take_due(now + Duration::from_millis(998))
        .is_empty());
    assert_eq!(
        deadlines.take_due(now + Duration::from_millis(999)),
        vec!["one"]
    );
    for tab in 0..15 {
        deadlines.set(&tab.to_string(), Some(now + next_delay(0, 0).unwrap()));
    }
    let mut checks = 0;
    for boundary in [800, 1500, 5000] {
        let at = now + Duration::from_millis(boundary);
        let due = deadlines.take_due(at);
        assert_eq!(due.len(), 15);
        checks += due.len();
        for tab in due {
            deadlines.set(&tab, next_delay(boundary, boundary).map(|delay| at + delay));
        }
    }
    assert_eq!(checks, 45);
    assert_eq!(deadlines.next(), None);
    assert!(deadlines
        .take_due(now + Duration::from_secs(3600))
        .is_empty());
}

#[test]
fn next_boundary_tracks_both_clocks_and_output_rearms_checks() {
    for (byte, semantic, expected) in [
        (0, 0, Some(800)),
        (799, 799, Some(1)),
        (800, 800, Some(700)),
        (1500, 1500, Some(3500)),
        (5000, 5000, None),
        (10_000, 1499, Some(1)),
        (0, 5000, Some(800)),
    ] {
        assert_eq!(
            next_delay(byte, semantic).map(|delay| delay.as_millis()),
            expected
        );
    }
}

#[test]
fn post_seed_turn_status_entry_reaches_quiescence_without_output() {
    use codeterm_types::pty::{TabStatus, TabStatusTracker};

    let now = Instant::now();
    let mut tracker = TabStatusTracker::new();
    let mut deadlines = Deadlines::default();
    assert!(tracker.all_ids().is_empty());
    assert_eq!(deadlines.next(), None);

    tracker.set_working("post-seed");
    assert_eq!(tracker.get_status("post-seed"), TabStatus::Working);
    let clocks = tracker.quiescence("post-seed", now);
    assert_eq!(clocks, Some((0, 0)));
    deadlines.set("post-seed", next_deadline(now, clocks, None));
    assert_eq!(deadlines.next(), Some(now + Duration::from_millis(800)));

    let frame = Frame::output(0, &["❯"]);
    let mut state = verdict(TimelineState::Working);
    let mut elapsed = StatusElapsedTracker::new();
    for boundary in [800, 1500, 5000] {
        let at = now + Duration::from_millis(boundary);
        assert_eq!(deadlines.take_due(at), vec!["post-seed"]);
        state = classify(&frame, boundary, 0, state, false, &mut elapsed);
        deadlines.set(
            "post-seed",
            next_deadline(at, Some((boundary, boundary)), None),
        );
        if boundary >= 1500 {
            assert_eq!(state.state, TimelineState::Idle);
        }
    }
    assert_eq!(deadlines.next(), None);
}
