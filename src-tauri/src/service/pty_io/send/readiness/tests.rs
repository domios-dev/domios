use super::*;
use crate::agent::provider::ComposerCursorPosition;
use codeterm_pty::pty::screen_state::ComposerInputState;

fn input() -> ComposerInputState {
    ComposerInputState {
        cursor_visible: true,
        cursor_row: 27,
        rows: 40,
        cols: 120,
        nonblank_inverse_cells: 0,
        last_nonblank_row: Some(27),
        alternate_screen: false,
        inverse_caret_row: None,
        sole_nonblank_inverse_row: None,
    }
}

#[test]
fn prompt_line_provider_frame_is_ready_without_waiting_for_timeout() {
    assert!(wait_for_observed_readiness(
        std::time::Instant::now(),
        || provider_frame_ready(
            true,
            false,
            true,
            false,
            composer_input_ready_for_position(input(), ComposerCursorPosition::PromptLine, false)
        ),
        |_| panic!("a ready prompt must not wait for another screen event"),
    ));
}

#[test]
fn prompt_line_questions_and_non_prompt_cursor_do_not_supply_composer_evidence() {
    assert!(!composer_input_ready_for_position(
        input(),
        ComposerCursorPosition::PromptLine,
        true
    ));
    let mut displaced = input();
    displaced.cursor_row = 26;
    assert!(!composer_input_ready_for_position(
        displaced,
        ComposerCursorPosition::PromptLine,
        false
    ));
}

#[test]
fn bottom_region_preserves_existing_readiness() {
    for position in [ComposerCursorPosition::BottomRegion] {
        assert!(!composer_input_ready_for_position(input(), position, false));
        let mut bottom = input();
        bottom.cursor_row = 39;
        assert_eq!(
            composer_input_ready_for_position(bottom, position, true),
            composer_input_ready(bottom)
        );
        assert!(composer_input_ready_for_position(bottom, position, false));
    }
}

#[test]
fn visible_anywhere_accepts_a_busy_provider_cursor_above_the_bottom_region() {
    let mut above = input();
    above.cursor_row = 4;
    assert!(composer_input_ready_for_position(
        above,
        ComposerCursorPosition::VisibleAnywhere,
        false
    ));
    assert!(!composer_input_ready_for_position(
        above,
        ComposerCursorPosition::BottomRegion,
        false
    ));
    above.cursor_visible = false;
    assert!(!composer_input_ready_for_position(
        above,
        ComposerCursorPosition::VisibleAnywhere,
        false
    ));
}
