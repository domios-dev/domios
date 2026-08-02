use super::{classify_terminal_menu_question, extract_question_message, QuestionSubtype};

fn lines(rows: &[&str]) -> Vec<String> {
    rows.iter().map(|row| (*row).to_string()).collect()
}

#[test]
fn extracts_complete_authoritative_auth_message() {
    let screen = lines(&[
        "unrelated output",
        "",
        "Authentication required.",
        "Sign in to continue with this session.",
        "",
        "footer",
    ]);
    assert_eq!(
        extract_question_message(&screen, Some(QuestionSubtype::Auth)).as_deref(),
        Some("Authentication required.\nSign in to continue with this session.")
    );
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
    // VISIBLE cursor this stays None (the strict path needs hidden cursor).
    let screen = lines(&["┃  1. Alpha", "┃  2. Beta", "┃  press the keys"]);
    assert_eq!(classify_terminal_menu_question(&screen, false), None);
}

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

#[test]
fn terminal_menu_question_does_not_preempt_working_grace() {
    let screen = lines(&[
        "┃  Which option do you choose?",
        "┃",
        "┃  1. Alpha",
        "┃  2. Beta",
        "┃",
        "┃  ↑↓ select  enter submit  esc dismiss",
    ]);
    let mut input = quiescent_input(&screen);
    input.cursor_hidden = true;
    input.byte_idle_ms = 300;
    input.semantic_idle_ms = 300;
    let verdict = classify_state(&input, working());
    assert_eq!(verdict.state, TimelineState::Working);

    input.byte_idle_ms = 2000;
    input.semantic_idle_ms = 2000;
    let verdict = classify_state(&input, working());
    assert_eq!(verdict.state, TimelineState::Question);
}

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
    classify_state, composer_cursor_ready, offline_picker_is_coherent, ClassifyInput, Motion,
    TabVerdict, TimelineState,
};
use crate::agent::attributed_screen::PickerDetection;
use crate::agent::signal_extractor::StreamSignals;

fn working() -> TabVerdict {
    TabVerdict {
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

#[test]
fn authentication_prose_without_provider_prompt_is_not_a_question() {
    let screen = lines(&[
        "Authentication required is an example error message.",
        "The UI may say Sign in to continue.",
        "› Continue editing",
    ]);
    let verdict = classify_state(&quiescent_input(&screen), working());
    assert_eq!(verdict.state, TimelineState::Idle);
    assert_eq!(verdict.subtype, None);
}

// Idle promptly once Working grace has elapsed (structural, not the 5s timeout).
#[test]
fn cursor_composer_arrow_returns_idle_promptly() {
    let screen = lines(&[
        "  коде. Напиши тему.",
        "→ Add a follow-up",
        "Composer 2.5 · 7%                          Run Everything",
        "/private/var/folders/x/ct-replay-cursor",
    ]);
    let mut input = quiescent_input(&screen);
    input.byte_idle_ms = 2000;
    input.semantic_idle_ms = 2000;
    let verdict = classify_state(&input, working());
    assert_eq!(
        verdict.state,
        TimelineState::Idle,
        "cursor → composer arrow"
    );

    input.byte_idle_ms = 300;
    input.semantic_idle_ms = 300;
    let verdict = classify_state(&input, working());
    assert_eq!(
        verdict.state,
        TimelineState::Working,
        "cursor composer arrow must not pre-empt working grace"
    );
}

#[test]
fn claude_stale_status_above_ready_prompt_returns_idle_after_grace() {
    let screen = lines(&[
        "  Done with the requested changes.",
        "",
        "❯ Summarize everything that changed.",
        "",
        "✻ Swooping… (12m 57s · ↑ 53.9k tokens)",
        "",
        "────────────────────────────────────────────────────────",
        "❯",
        "────────────────────────────────────────────────────────",
        "  ⏵⏵ auto mode on (shift+tab to cycle) · esc to interrupt",
    ]);
    let mut input = quiescent_input(&screen);
    input.byte_idle_ms = 2000;
    input.semantic_idle_ms = 2000;

    let verdict = classify_state(&input, working());

    assert_eq!(
        verdict.state,
        TimelineState::Idle,
        "stale claude status must not pin a ready composer in Working"
    );
}

#[test]
fn claude_spinner_repaint_after_completion_does_not_flip_back_to_working() {
    let screen = lines(&[
        "  Final answer text.",
        "",
        "✻ Crunched for 7s",
        "",
        "✻ Evaporating… (7s · ↓ 165 tokens)",
        "",
        "────────────────────────────────────────────────────────",
        "❯",
        "────────────────────────────────────────────────────────",
        "  ⏵⏵ auto mode on (shift+tab to cycle) · esc to interrupt",
    ]);
    let mut input = quiescent_input(&screen);
    input.byte_idle_ms = 250;
    input.semantic_idle_ms = 2000;

    let verdict = classify_state(&input, working());

    assert_eq!(
        verdict.state,
        TimelineState::Idle,
        "status-bar animation bytes must not resurrect Working after semantic idle"
    );
}

#[test]
fn claude_recent_status_near_ready_prompt_stays_working_during_grace() {
    let screen = lines(&[
        "  Generating the answer.",
        "",
        "✢ Swooping… (12m 55s · ↓ 53.7k tokens)",
        "",
        "❯ Summarize everything that changed.",
        "────────────────────────────────────────────────────────",
        "❯ Press up to edit queued messages",
        "────────────────────────────────────────────────────────",
        "  ⏵⏵ auto mode on (shift+tab to cycle) · esc to interrupt",
    ]);
    let mut input = quiescent_input(&screen);
    input.byte_idle_ms = 900;
    input.semantic_idle_ms = 900;

    let verdict = classify_state(&input, working());

    assert_eq!(
        verdict.state,
        TimelineState::Working,
        "fresh claude status remains active during working grace"
    );
}

// bottom input region of an alt-screen ⇒ composer_ready ⇒ Idle promptly.
#[test]
fn opencode_visible_cursor_in_composer_returns_idle_promptly() {
    let screen = lines(&[
        "┃  Build · DeepSeek V4 Flash Free",
        "  9.4K (5%)  ctrl+p commands",
    ]);
    let mut input = quiescent_input(&screen);
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
fn offline_picker_above_ready_composer_is_stale_transcript() {
    let mut signals = StreamSignals::default();
    signals.rows = 40;
    signals.screen.lines = lines(&[
        "Plain prose options:",
        "1. Keep swipe-only",
        "2. Add a d-pad",
        "",
        "────────────────────────",
        "❯",
        "────────────────────────",
    ]);
    let stale = PickerDetection {
        question: "Plain prose options:".to_string(),
        options: vec!["Keep swipe-only".to_string(), "Add a d-pad".to_string()],
        selected_index: 0,
        multi_select: false,
        top_row: 1,
        bottom_row: 2,
    };
    assert!(
        !offline_picker_is_coherent(&stale, &signals),
        "a picker above a visible bottom composer is stale scrollback"
    );

    let active = PickerDetection {
        bottom_row: 5,
        ..stale
    };
    assert!(
        offline_picker_is_coherent(&active, &signals),
        "a picker reaching the composer row remains a candidate"
    );
}

#[test]
fn options_block_horizontal_only_not_vertical_or_idle_box() {
    // Horizontal buttons (bottom row, column-gapped) -> vetoed.
    assert!(super::looks_like_options_block(&lines(&[
        "Allow?",
        "❯ Yes      No      Cancel"
    ])));
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

#[test]
fn active_turn_footer_fires_on_real_active_footers() {
    assert!(super::active_turn_footer(&lines(&[
        "Working on it…",
        "⏵⏵ accept edits on · esc to interrupt",
    ])));
    assert!(super::active_turn_footer(&lines(&[
        "Thinking…",
        "✶ Cogitating (3m 54s · esc to interrupt)",
    ])));
    assert!(super::active_turn_footer(&lines(&[
        "■■■■──────  esc to interrupt",
    ])));
    assert!(super::active_turn_footer(&lines(&[
        "→ Working (ctrl+c to stop)",
    ])));
    // opencode: animated progress meter + interrupt hint.
    assert!(super::active_turn_footer(&lines(&[
        "   ⬝⬝⬝⬝⬝⬝■■  esc interrupt                                                                 10.5K (5%)  ctrl+p commands",
    ])));
}

#[test]
fn active_turn_footer_silent_on_idle_and_completed_screens() {
    // claude IDLE footer: has the ⏵⏵ marker but NO interrupt hint → not active.
    assert!(!super::active_turn_footer(&lines(&[
        "⏵⏵ bypass permissions on (shift+tab to cycle)",
    ])));
    assert!(!super::active_turn_footer(&lines(&[
        "⏵⏵ auto mode on (shift+tab to cycle) · + for agents",
    ])));
    // mode-cycle footer itself is not active turn evidence.
    assert!(!super::active_turn_footer(&lines(&[
        "⏵⏵ auto mode on (shift+tab to cycle) · esc to interrupt",
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

#[test]
fn claude_auto_mode_footer_does_not_mask_active_spinner() {
    let screen = lines(&[
        "  Updating the code.",
        "",
        "✶ Working… (3s · esc to interrupt)",
        "",
        "────────────────────────────────────────────────────────",
        "  ⏵⏵ auto mode on (shift+tab to cycle) · esc to interrupt",
    ]);
    let mut input = quiescent_input(&screen);
    input.byte_idle_ms = 2000;
    input.semantic_idle_ms = 2000;

    let verdict = classify_state(&input, working());

    assert_eq!(
        verdict.state,
        TimelineState::Working,
        "excluding the idle mode footer must not hide the real active spinner"
    );
}

#[test]
fn claude_answered_question_with_stale_picker_below_composer_is_idle() {
    let screen = lines(&[
        "❯ йойо?тест",
        "⏺ Йо-йо 😄 Тест принят, я на связи. Что дальше?",
        "✻ Baked for 3s",
        "───────────────────────────────────────────────────────",
        "❯ ",
        "───────────────────────────────────────────────────────",
        "  ⏵⏵ auto mode on (shift+tab to cycle) · ← for agents",
        "───────────────────────────────────────────────────────",
        "  5. Chat about this",
        "     Вариант B.",
        "Enter to select · ↑/↓ to navigate · Esc to cancel",
        "     Вариант C.",
        "  4. Type something.",
        "───────────────────────────────────────────────────────",
        "  5. Chat about this",
        "Enter to select · ↑/↓ to navigate · Esc to cancel",
    ]);
    let mut input = quiescent_input(&screen);
    input.byte_idle_ms = 6000;
    input.semantic_idle_ms = 6000;
    let previous = TabVerdict {
        state: TimelineState::Question,
        subtype: Some(QuestionSubtype::Unknown),
    };
    let verdict = classify_state(&input, previous);
    assert_eq!(
        verdict.state,
        TimelineState::Idle,
        "answered question + stale picker echo below the ready composer must clear to Idle"
    );
}

#[test]
fn claude_active_question_picker_still_classifies_question() {
    let screen = lines(&[
        "⏺ Выбери вариант:",
        "  ❯ 1. Вариант A",
        "    2. Вариант B",
        "    3. Вариант C",
        "    4. Type something.",
        "Enter to select · ↑/↓ to navigate · Esc to cancel",
    ]);
    let mut input = quiescent_input(&screen);
    input.cursor_hidden = true;
    input.byte_idle_ms = 6000;
    input.semantic_idle_ms = 6000;
    let verdict = classify_state(
        &input,
        TabVerdict {
            state: TimelineState::Idle,
            subtype: None,
        },
    );
    assert_eq!(
        verdict.state,
        TimelineState::Question,
        "an active, unanswered picker must remain Question"
    );
}

#[test]
fn claude_current_gen_long_think_stays_working_while_status_band_animates() {
    let screen = lines(&[
        "⏺ Write(fib.py)",
        "  ⎿  Error writing file",
        "",
        "✽ Inferring… (58s · ↓ 797 tokens · thinking with high effort)",
        "",
        "────────────────────────────────────────────────────────",
        "❯",
        "────────────────────────────────────────────────────────",
        "  ⏵⏵ auto mode on (shift+tab to cycle) · esc to interrupt             ● high · /effort",
    ]);
    let mut input = quiescent_input(&screen);
    input.semantic_idle_ms = 6000; // transcript settled (long think)
    input.byte_idle_ms = 200; // status band still animating every tick
    let verdict = classify_state(&input, working());
    assert_eq!(
        verdict.state,
        TimelineState::Working,
        "a live thinking widget must not be demoted to Idle by semantic idle alone"
    );
}

#[test]
fn status_repaint_after_adjacent_completion_is_idle_but_pure_think_is_working() {
    let live_screen = lines(&[
        "  Tool output",
        "✽ Inferring… (58s · ↓ 797 tokens · thinking)",
        "────────────────────────────────────────────────────────",
        "❯",
        "────────────────────────────────────────────────────────",
    ]);
    let mut live_input = quiescent_input(&live_screen);
    live_input.semantic_idle_ms = 2000;
    live_input.byte_idle_ms = 250;

    let completed_screen = lines(&[
        "  Final answer text.",
        "✻ Crunched for 7s",
        "✻ Evaporating… (7s · ↓ 165 tokens)",
        "────────────────────────────────────────────────────────",
        "❯",
        "────────────────────────────────────────────────────────",
    ]);
    let mut completed_input = quiescent_input(&completed_screen);
    completed_input.semantic_idle_ms = 2000;
    completed_input.byte_idle_ms = 250;

    assert_eq!(
        classify_state(&live_input, working()).state,
        TimelineState::Working,
        "an animating status without an adjacent completion summary is live"
    );
    assert_eq!(
        classify_state(&completed_input, working()).state,
        TimelineState::Idle,
        "an animating status immediately below a completion summary is a repaint"
    );
}

// recognized by structure (spinner + ellipsis + elapsed/token), not a verb list.
#[test]
fn claude_novel_spinner_verb_classifies_working_by_structure_not_dictionary() {
    let screen = lines(&[
        "⏺ Reading files",
        "",
        "✽ Xyzzying… (2s · ↓ 5 tokens · thinking)",
        "",
        "────────────────────────────────────────────────────────",
        "❯",
        "────────────────────────────────────────────────────────",
        "  ⏵⏵ auto mode on (shift+tab to cycle) · esc to interrupt",
    ]);
    let mut input = quiescent_input(&screen);
    input.semantic_idle_ms = 6000;
    input.byte_idle_ms = 200;
    let verdict = classify_state(&input, working());
    assert_eq!(verdict.state, TimelineState::Working);
}

// itself. Still working despite a visible composer + high semantic idle.
#[test]
fn claude_legacy_gen_footer_with_inline_interrupt_hint_stays_working() {
    let screen = lines(&[
        "  Generating the answer.",
        "",
        "✶ Cogitating (3m 54s · esc to interrupt)",
        "",
        "❯",
    ]);
    let mut input = quiescent_input(&screen);
    input.semantic_idle_ms = 6000;
    input.byte_idle_ms = 200;
    let verdict = classify_state(&input, working());
    assert_eq!(verdict.state, TimelineState::Working);
}

#[test]
fn codex_working_footer_stays_working_during_long_turn() {
    let screen = lines(&[
        "• Analyzing the repository.",
        "",
        "→ Working (ctrl+c to stop)",
    ]);
    let mut input = quiescent_input(&screen);
    input.semantic_idle_ms = 6000;
    input.byte_idle_ms = 200;
    let verdict = classify_state(&input, working());
    assert_eq!(verdict.state, TimelineState::Working);
}

// that the shared oracle keeps it working through a long turn.
#[test]
fn opencode_progress_meter_footer_stays_working_during_long_turn() {
    let screen = lines(&[
        "┃  Build · model",
        "   ⬝⬝⬝⬝⬝⬝■■  esc interrupt                              10.5K (5%)  ctrl+p commands",
    ]);
    let mut input = quiescent_input(&screen);
    input.semantic_idle_ms = 6000;
    input.byte_idle_ms = 200;
    let verdict = classify_state(&input, working());
    assert_eq!(verdict.state, TimelineState::Working);
}

#[test]
fn claude_settled_turn_with_summary_resolves_idle() {
    let screen = lines(&[
        "  Final answer text.",
        "",
        "✻ Crunched for 58s",
        "",
        "────────────────────────────────────────────────────────",
        "❯",
        "────────────────────────────────────────────────────────",
        "  ⏵⏵ auto mode on (shift+tab to cycle) · esc to interrupt",
    ]);
    let mut input = quiescent_input(&screen);
    input.semantic_idle_ms = 6000;
    input.byte_idle_ms = 6000; // nothing animating anymore
    let verdict = classify_state(&input, working());
    assert_eq!(verdict.state, TimelineState::Idle);
}

// Live P0 oracle (prod notification 4ae14e7f): a structurally-minted picker
// question carrying a codex skills-lint warning line must classify Unknown.
#[test]
fn skills_lint_warning_picker_question_classifies_unknown_subtype() {
    let screen = lines(&[
        r"⚠ D:\Developer\codeterm\.agents\skills\gap-analysis\SKILL.md: missing field description",
        "",
        "› ",
        r"  gpt-5.6-sol high · D:\Developer\codeterm",
    ]);
    let mut input = quiescent_input(&screen);
    input.picker_question = Some(
        r"⚠ D:\Developer\codeterm\.agents\skills\gap-analysis\SKILL.md: missing field description",
    );
    let verdict = classify_state(&input, working());
    assert_eq!(verdict.state, TimelineState::Question);
    assert_eq!(verdict.subtype, Some(QuestionSubtype::Unknown));
}

// Envelope questions are not reliably Unknown ("model=…" hits ModelPicker), so
// notification admission cannot discriminate on subtype alone.
#[test]
fn coordination_envelope_picker_question_subtype_is_not_reliably_unknown() {
    let envelope =
        "-=-codeterm:tab label=Reviewer tab=166df18d model=gpt-5.6-sol-=-Coordination note only.";
    let screen = lines(&[envelope]);
    let mut input = quiescent_input(&screen);
    input.picker_question = Some(envelope);
    let with_model = classify_state(&input, working());
    assert_eq!(with_model.state, TimelineState::Question);
    assert_eq!(with_model.subtype, Some(QuestionSubtype::ModelPicker));

    let plain_envelope = "-=-codeterm:report from=Worker tab=abc123-=-Task finished, see summary.";
    let screen = lines(&[plain_envelope]);
    let mut input = quiescent_input(&screen);
    input.picker_question = Some(plain_envelope);
    let without_model = classify_state(&input, working());
    assert_eq!(without_model.state, TimelineState::Question);
    assert_eq!(without_model.subtype, Some(QuestionSubtype::Unknown));
}
