use super::*;
use crate::offline_state_classifier::{prompt_line_cursor_ready, WAIT_MS};
use codeterm_plugin::provider::ComposerCursorPosition;

fn input(screen: &[String], visible: bool, row: u16) -> ClassifyInput<'_> {
    let last = screen
        .iter()
        .rposition(|line| !line.trim().is_empty())
        .map(|row| row as u16);
    ClassifyInputBuilder::for_rows(StatusBarRows::fixture(0))
        .with_composer_position(ComposerCursorPosition::PromptLine)
        .build(ClassifyParts {
            picker_question: None,
            screen_lines: screen,
            tail: "",
            byte_idle_ms: 0,
            semantic_idle_ms: 0,
            motion: Motion::Still,
            cursor_hidden: !visible,
            composer_ready: prompt_line_cursor_ready(visible, row, last),
            status_elapsed: StatusElapsedProgress::NONE,
        })
}

#[test]
fn prompt_line_settles_at_visible_prompt_without_working_grace() {
    let screen = lines(&["answer", ">", "", ""]);
    assert_eq!(
        classify_state(&input(&screen, true, 1), working()).state,
        TimelineState::Idle
    );
}

#[test]
fn prompt_line_requires_visible_cursor_on_last_nonblank_row() {
    assert!(prompt_line_cursor_ready(true, 2, Some(2)));
    assert!(!prompt_line_cursor_ready(false, 2, Some(2)));
    assert!(!prompt_line_cursor_ready(true, 1, Some(2)));
    assert!(!prompt_line_cursor_ready(true, 0, None));
}

#[test]
fn prompt_line_byte_activity_starts_work_without_status_widget() {
    let screen = lines(&["> request", "partial answer", ""]);
    let idle = TabVerdict {
        state: TimelineState::Idle,
        subtype: None,
    };
    assert_eq!(
        classify_state(&input(&screen, false, 1), idle).state,
        TimelineState::Working
    );
    let mut observation = input(&screen, true, 0);
    observation.byte_idle_ms = WAIT_MS;
    observation.motion = Motion::Streaming;
    assert_eq!(
        classify_state(&observation, idle).state,
        TimelineState::Working
    );
    observation.motion = Motion::Still;
    assert_eq!(
        classify_state(&observation, idle).state,
        TimelineState::Idle
    );
}

#[test]
fn byte_activity_alone_does_not_start_work_for_other_composers() {
    let screen = lines(&["output"]);
    let idle = TabVerdict {
        state: TimelineState::Idle,
        subtype: None,
    };
    let mut observation = input(&screen, false, 0);
    observation.prompt_line = false;
    assert_eq!(classify_state(&observation, idle), idle);
}

#[test]
fn prompt_line_preserves_structured_question_priority() {
    let screen = lines(&[">"]);
    let mut observation = input(&screen, true, 0);
    observation.picker_question = Some("choice");
    assert_eq!(
        classify_state(&observation, working()).state,
        TimelineState::Question
    );
}
