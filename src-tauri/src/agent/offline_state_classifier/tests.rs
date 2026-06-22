use super::{classify_terminal_menu_question, QuestionSubtype};

fn lines(rows: &[&str]) -> Vec<String> {
    rows.iter().map(|row| (*row).to_string()).collect()
}

#[test]
fn detects_boxed_checkbox_terminal_question() {
    let screen = lines(&[
        " │ CLI choice",
        " │ 1. Please choose one option:",
        " │   › [ ] Alpha",
        " │     [ ] Beta",
        " │     [ ] Gamma",
        " │ ↑/↓ option · ←/→ question · Space select · Enter next/submit · Esc to skip",
    ]);
    assert_eq!(
        classify_terminal_menu_question(&screen, true),
        Some(QuestionSubtype::Unknown)
    );
}

#[test]
fn detects_boxed_numbered_terminal_question_without_selected_marker() {
    let screen = lines(&[
        "┃  Which option do you choose?",
        "┃",
        "┃  1. Alpha",
        "┃  2. Beta",
        "┃  3. Gamma",
        "┃",
        "┃  ↑↓ select  enter submit  esc dismiss",
    ]);
    assert_eq!(
        classify_terminal_menu_question(&screen, true),
        Some(QuestionSubtype::Unknown)
    );
}

#[test]
fn ignores_plain_numbered_prose_without_terminal_choice_footer() {
    let screen = lines(&[
        "Here is a plain list:",
        "1. Alpha",
        "2. Beta",
        "3. Gamma",
        "No action is required.",
    ]);
    assert_eq!(classify_terminal_menu_question(&screen, true), None);
}

#[test]
fn ignores_terminal_choice_shape_when_cursor_is_visible() {
    // ↑↓/submit/dismiss footer without the broader confirm-footer wording: with a
    // VISIBLE cursor this stays None (the strict path needs hidden cursor).
    let screen = lines(&["┃  1. Alpha", "┃  2. Beta", "┃  press the keys"]);
    assert_eq!(classify_terminal_menu_question(&screen, false), None);
}

// Permission/choice box with a VISIBLE cursor + a confirm-style key-hint footer
// (`… Enter to confirm, Esc to cancel`) must be Question — not false-idled by
// composer_ready. Mirrors synthetic permission_box_visible_cursor_v1.
#[test]
fn permission_box_with_visible_cursor_and_confirm_footer_is_question() {
    let screen = lines(&[
        "Allow command execution?",
        "  1. Yes, allow once",
        "  2. Yes, allow for this session",
        "  3. No, deny",
        "  Use up/down to choose, Enter to confirm, Esc to cancel",
    ]);
    assert_eq!(
        classify_terminal_menu_question(&screen, false),
        Some(QuestionSubtype::Unknown),
        "visible-cursor permission box must classify Question"
    );
}

// Plain numbered prose WITHOUT a confirm/choice footer stays not-a-question even
// with a visible cursor (no false Question on normal lists).
#[test]
fn numbered_prose_without_confirm_footer_is_not_question_visible_cursor() {
    let screen = lines(&[
        "Here are three options to consider:",
        "1. Alpha approach",
        "2. Beta approach",
        "3. Gamma approach",
        "Let me know which you prefer.",
    ]);
    assert_eq!(classify_terminal_menu_question(&screen, false), None);
}

use super::{
    classify_state, composer_cursor_ready, ClassifyInput, Motion, PaneVerdict, TimelineState,
};

fn working() -> PaneVerdict {
    PaneVerdict {
        state: TimelineState::Working,
        subtype: None,
    }
}

fn quiescent_input(screen: &[String]) -> ClassifyInput<'_> {
    ClassifyInput {
        picker_question: None,
        screen_lines: screen,
        tail: "",
        byte_idle_ms: 6000,
        semantic_idle_ms: 6000,
        motion: Motion::Still,
        cursor_hidden: false,
        composer_ready: false,
    }
}

// The codex idle composer: a `›` prompt line followed by the model/cwd footer.
// ready_prompt must recognize it as idle even when the TUI leaves blank
// padding rows below the footer (real grids are full-height with trailing
// blanks). Regression: `.rev().take(4)` over raw rows let >=2 trailing blanks
// push the `›` line out of the window, so a previously-Working pane stuck
// Working forever.
#[test]
fn idle_composer_with_trailing_blank_padding_returns_idle() {
    let screen = lines(&[
        "• Да, удобно для быстрых развилок.",
        "› Implement {feature}",
        "  gpt-5.5 medium · ~/.codeterm-dev/general",
        "",
        "",
        "",
    ]);
    let verdict = classify_state(&quiescent_input(&screen), working());
    assert_eq!(
        verdict.state,
        TimelineState::Idle,
        "trailing-blank composer"
    );
}

// Safety net: even when no provider-specific ready-prompt glyph is on screen,
// a long-quiescent pane with no active-work evidence (no status widget, no
// active-turn footer, no motion) must not stay Working indefinitely.
#[test]
fn long_quiescent_unknown_layout_does_not_stay_working_forever() {
    let screen = lines(&["some completed assistant output", "with no composer glyph"]);
    let mut input = quiescent_input(&screen);
    input.byte_idle_ms = 8000;
    input.semantic_idle_ms = 8000;
    let verdict = classify_state(&input, working());
    assert_eq!(
        verdict.state,
        TimelineState::Idle,
        "stuck-working safety net"
    );
}

// Cursor (app) post-turn composer: footer arrow `→` (no stop-hint) must read
// Idle PROMPTLY at low idle clocks (structural, not the 5s timeout).
#[test]
fn cursor_composer_arrow_returns_idle_promptly() {
    let screen = lines(&[
        "  коде. Напиши тему.",
        "→ Add a follow-up",
        "Composer 2.5 · 7%                          Run Everything",
        "/private/var/folders/x/ct-replay-cursor",
    ]);
    let mut input = quiescent_input(&screen);
    input.byte_idle_ms = 300;
    input.semantic_idle_ms = 300;
    let verdict = classify_state(&input, working());
    assert_eq!(
        verdict.state,
        TimelineState::Idle,
        "cursor → composer arrow"
    );
}

// Opencode full-screen composer: no glyph, but a visible cursor parked in the
// bottom input region of an alt-screen ⇒ composer_ready ⇒ Idle promptly.
#[test]
fn opencode_visible_cursor_in_composer_returns_idle_promptly() {
    let screen = lines(&[
        "┃  Build · DeepSeek V4 Flash Free",
        "  9.4K (5%)  ctrl+p commands",
    ]);
    let mut input = quiescent_input(&screen);
    // Past WORKING_GRACE_MS: composer_ready is the post-grace structural idle
    // (it must NOT pre-empt grace — see composer_ready_does_not_preempt_working_grace).
    input.byte_idle_ms = 2000;
    input.semantic_idle_ms = 2000;
    input.composer_ready = composer_cursor_ready(true, 26, 30);
    assert!(input.composer_ready, "geometry should be composer-ready");
    let verdict = classify_state(&input, working());
    assert_eq!(
        verdict.state,
        TimelineState::Idle,
        "opencode visible-cursor composer"
    );
}

#[test]
fn composer_ready_visible_cursor_in_bottom_region_primary_or_alt() {
    assert!(
        composer_cursor_ready(true, 26, 30),
        "opencode idle (visible, bottom)"
    );
    assert!(
        composer_cursor_ready(true, 20, 24),
        "opencode primary-screen idle"
    );
    assert!(
        !composer_cursor_ready(false, 26, 30),
        "hidden cursor (picker)"
    );
    assert!(
        !composer_cursor_ready(true, 5, 30),
        "cursor mid-screen, not bottom"
    );
}

// composer_ready must NOT pre-empt the working-grace window: a visible cursor in
// the box appears mid-turn too, so within grace the verdict stays Working.
#[test]
fn composer_ready_does_not_preempt_working_grace() {
    let screen = lines(&["┃  Build · model", "  9.4K (5%)  ctrl+p commands"]);
    let mut input = quiescent_input(&screen);
    input.composer_ready = true;
    input.semantic_idle_ms = 500; // < WORKING_GRACE_MS
    input.byte_idle_ms = 500;
    let verdict = classify_state(&input, working());
    assert_eq!(
        verdict.state,
        TimelineState::Working,
        "grace must win over composer_ready"
    );
}

// composer_ready must NOT mask a real (even uncaught) PTY question: a horizontal
// button / options block at the bottom vetoes the composer-ready idle path.
#[test]
fn composer_ready_vetoed_by_options_block() {
    let screen = lines(&["Allow this tool to run?", "❯ Yes      No      Always"]);
    let mut input = quiescent_input(&screen);
    input.composer_ready = true;
    input.byte_idle_ms = 3000; // past grace
    input.semantic_idle_ms = 3000;
    let verdict = classify_state(&input, working());
    assert_ne!(
        verdict.state,
        TimelineState::Idle,
        "horizontal buttons must veto composer idle"
    );
}

#[test]
fn options_block_horizontal_only_not_vertical_or_idle_box() {
    // Horizontal buttons (bottom row, column-gapped) -> vetoed.
    assert!(super::looks_like_options_block(&lines(&[
        "Allow?",
        "❯ Yes      No      Cancel"
    ])));
    // Vertical numbered menu is NOT vetoed here (handled by
    // classify_terminal_menu_question when active; answered ones sit above the
    // composer and must still resolve Idle).
    assert!(!super::looks_like_options_block(&lines(&[
        "┃ 1. Alpha",
        "┃ 2. Beta",
        "┃ 3. Gamma"
    ])));
    // Answered cursor poll above the composer -> not vetoed (composer is bottom).
    assert!(!super::looks_like_options_block(&lines(&[
        "[x] A",
        "[ ] B",
        "→ Add a follow-up"
    ])));
    // opencode idle box -> not vetoed.
    assert!(!super::looks_like_options_block(&lines(&[
        "┃  Build · model",
        "  9.4K (5%)  ctrl+p commands"
    ])));
}

// ── active_turn_footer copy-fragility gate ──────────────────────────────────
// `active_turn_footer` intentionally keys off the literal interrupt-affordance
// copy ("esc to interrupt" / "ctrl+c to stop"). That copy is the load-bearing
// SEMANTIC signal — the interrupt hint is the only thing the TUI shows *only
// while a turn is active*; the surrounding structural shapes (⏵⏵ marker,
// progress meter, elapsed timer) ALSO appear on idle/completed screens, so they
// cannot disambiguate active-vs-idle on their own. These tests pin both halves:
// the literal must still fire on real active footers, and the structural shapes
// must NOT fire without it. If a vendor renames the hint, this gate goes red
// (loud) instead of the classifier silently mis-reading "working" vs "done".

#[test]
fn active_turn_footer_fires_on_real_active_footers() {
    // claude: bypass marker + interrupt hint
    assert!(super::active_turn_footer(&lines(&[
        "Working on it…",
        "⏵⏵ accept edits on · esc to interrupt",
    ])));
    // claude: interrupt hint + elapsed timer
    assert!(super::active_turn_footer(&lines(&[
        "Thinking…",
        "✶ Cogitating (3m 54s · esc to interrupt)",
    ])));
    // claude: interrupt hint + progress-meter shape
    assert!(super::active_turn_footer(&lines(&[
        "■■■■──────  esc to interrupt",
    ])));
    // codex: arrow prefix + ctrl+c stop hint
    assert!(super::active_turn_footer(&lines(&[
        "→ Working (ctrl+c to stop)",
    ])));
}

#[test]
fn active_turn_footer_silent_on_idle_and_completed_screens() {
    // claude IDLE footer: has the ⏵⏵ marker but NO interrupt hint → not active.
    assert!(!super::active_turn_footer(&lines(&[
        "⏵⏵ bypass permissions on (shift+tab to cycle)",
    ])));
    // completed turn: elapsed timer present, but the interrupt hint is gone.
    assert!(!super::active_turn_footer(&lines(&[
        "✓ Done (3m 54s · 17.0k tokens)",
    ])));
    // progress-meter glyphs alone (no interrupt hint) must not fire.
    assert!(!super::active_turn_footer(&lines(&["■■■■──────  100%"])));
    // plain shell.
    assert!(!super::active_turn_footer(&lines(&["$ ls", "file.txt"])));
}
