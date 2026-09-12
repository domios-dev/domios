use super::{extract_question_message, QuestionSubtype};

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

use super::{
    classify_state, composer_cursor_ready, offline_picker_is_coherent, ClassifyInput, Motion,
    StatusElapsedProgress, StatusElapsedTracker, TabVerdict, TimelineState,
};
use crate::agent::attributed_screen::PickerDetection;
use crate::agent::classify_input::{ClassifyInputBuilder, ClassifyParts, StatusBarRows};
use crate::agent::signal_extractor::{SignalExtractor, StreamSignals};

fn working() -> TabVerdict {
    TabVerdict {
        state: TimelineState::Working,
        subtype: None,
    }
}

fn quiescent_input(screen: &[String]) -> ClassifyInput<'_> {
    ClassifyInputBuilder::for_rows(StatusBarRows::fixture(0)).build(ClassifyParts {
        picker_question: None,
        screen_lines: screen,
        tail: "",
        byte_idle_ms: 6000,
        semantic_idle_ms: 6000,
        motion: Motion::Still,
        cursor_hidden: false,
        composer_ready: false,
        status_elapsed: StatusElapsedProgress::NONE,
    })
}

#[test]
fn question_detection_removed_screen_content_is_not_question() {
    let numbered = lines(&[
        "┃  › 1. Alpha",
        "┃    2. Beta",
        "┃  ↑↓ select  enter submit  esc dismiss",
    ]);
    let checkbox = lines(&[
        "│   [ ] Alpha",
        "│   [ ] Beta",
        "│   ↑/↓ option · Space select · Enter next/submit · Esc to skip",
    ]);

    for screen in [&numbered, &checkbox] {
        let mut input = quiescent_input(screen);
        input.cursor_hidden = true;
        let verdict = classify_state(&input, working());
        assert_ne!(verdict.state, TimelineState::Question);
    }
}

#[test]
fn question_detection_removed_structured_picker_still_question() {
    let screen = lines(&["Choose an option", "1. Alpha", "2. Beta"]);
    let mut input = quiescent_input(&screen);
    input.picker_question = Some("Choose an option");

    let verdict = classify_state(&input, working());

    assert_eq!(verdict.state, TimelineState::Question);
}

#[test]
fn question_detection_removed_rate_limit_still_question() {
    let screen = lines(&["Rate limit reached. Try again later."]);

    let verdict = classify_state(&quiescent_input(&screen), working());

    assert_eq!(verdict.state, TimelineState::Question);
    assert_eq!(verdict.subtype, Some(QuestionSubtype::RateLimit));
}

#[test]
fn question_detection_removed_order_skips_parked_question() {
    let menu = lines(&[
        "┃  › 1. Alpha",
        "┃    2. Beta",
        "┃  ↑↓ select  enter submit  esc dismiss",
    ]);
    let mut working_input = quiescent_input(&menu);
    working_input.motion = Motion::Streaming;
    assert_eq!(
        classify_state(
            &working_input,
            TabVerdict {
                state: TimelineState::Idle,
                subtype: None,
            },
        )
        .state,
        TimelineState::Working
    );

    let idle_screen = lines(&["› ready"]);
    let mut idle_input = quiescent_input(&idle_screen);
    idle_input.cursor_hidden = true;
    assert_eq!(
        classify_state(
            &idle_input,
            TabVerdict {
                state: TimelineState::Working,
                subtype: None,
            },
        )
        .state,
        TimelineState::Idle
    );

    let previous = TabVerdict {
        state: TimelineState::Question,
        subtype: Some(QuestionSubtype::Unknown),
    };
    assert_eq!(classify_state(&quiescent_input(&menu), previous), previous);
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


// ── status-widget elapsed as structural turn progress ────────────────

const COMPOSER_DIVIDER: &str = "────────────────────────────────────────";
const COMBINED_FOOTER: &str = "  ⏵⏵ auto mode on (shift+tab to cycle) · esc to interrupt";

/// The reported flicker screen: a live Claude status widget with a visible ready
/// composer and the COMBINED mode-cycle/interrupt footer, which is deliberately
/// not active-turn evidence. Everything except the widget's own counter reads as
/// settled, so that counter is the only structural progress signal left.
fn claude_quiet_working_screen(elapsed: &str) -> Vec<String> {
    let mut screen = lines(&["⏺ Read(src/main.rs)", ""]);
    screen.push(format!(
        "✽ Inferring… ({elapsed} · ↓ 797 tokens · thinking)"
    ));
    screen.extend(lines(&[
        "",
        COMPOSER_DIVIDER,
        "❯",
        COMPOSER_DIVIDER,
        COMBINED_FOOTER,
    ]));
    screen
}

/// Same screen, widget still spinning but carrying no counter at all.
fn claude_quiet_working_screen_without_elapsed() -> Vec<String> {
    lines(&[
        "⏺ Read(src/main.rs)",
        "",
        "✽ Inferring…",
        "",
        COMPOSER_DIVIDER,
        "❯",
        COMPOSER_DIVIDER,
        COMBINED_FOOTER,
    ])
}

/// Turn over: the widget is gone and only the completion summary remains.
fn claude_settled_screen() -> Vec<String> {
    lines(&[
        "  Final answer text.",
        "",
        "✻ Crunched for 20s",
        "",
        COMPOSER_DIVIDER,
        "❯",
        COMPOSER_DIVIDER,
        COMBINED_FOOTER,
    ])
}

fn paint(screen: &[String]) -> Vec<u8> {
    let mut out = String::from("\x1b[2J\x1b[H");
    for line in screen {
        out.push_str(line);
        out.push_str("\r\n");
    }
    out.into_bytes()
}

#[test]
fn claude_status_elapsed_advance_keeps_working_after_bytes_go_quiet() {
    let first = claude_quiet_working_screen("14s");
    let second = claude_quiet_working_screen("20s");

    // The combined footer stays exactly what it was: NOT active-turn evidence.
    assert!(!super::active_turn_footer(&second));

    // Pre-fix arm: the identical screen with no progress evidence resolves Idle,
    // which is the flicker an advancing counter has to answer.
    let mut without_progress = quiescent_input(&second);
    without_progress.status_elapsed = StatusElapsedProgress::NONE;
    assert_eq!(
        classify_state(&without_progress, working()).state,
        TimelineState::Idle,
        "no progress evidence keeps the pre-existing idle verdict"
    );

    let mut tracker = StatusElapsedTracker::new();
    tracker.observe(super::observed_status_elapsed_ms(&first));

    let mut input = quiescent_input(&second);
    input.status_elapsed = tracker.observe(super::observed_status_elapsed_ms(&second));

    assert_eq!(input.byte_idle_ms, 6000);
    assert_eq!(input.semantic_idle_ms, 6000);
    assert_eq!(input.status_elapsed.previous_ms, Some(14_000));
    assert_eq!(input.status_elapsed.current_ms, Some(20_000));
    assert_eq!(
        classify_state(&input, working()).state,
        TimelineState::Working,
        "an advancing status counter is live work even after the repaint went quiet"
    );
}

#[test]
fn claude_status_elapsed_freeze_does_not_latch_working() {
    let screen = claude_quiet_working_screen("14s");
    let mut tracker = StatusElapsedTracker::new();
    tracker.observe(super::observed_status_elapsed_ms(&screen));

    let mut frozen = quiescent_input(&screen);
    frozen.status_elapsed = tracker.observe(super::observed_status_elapsed_ms(&screen));
    assert_eq!(frozen.status_elapsed.previous_ms, Some(14_000));
    assert!(!frozen.status_elapsed.advancing());
    assert_eq!(
        classify_state(&frozen, working()).state,
        TimelineState::Idle,
        "a frozen counter must not latch Working"
    );

    let absent_screen = claude_quiet_working_screen_without_elapsed();
    assert!(super::extract_status_line(&absent_screen).is_some());
    let mut absent = quiescent_input(&absent_screen);
    absent.status_elapsed = tracker.observe(super::observed_status_elapsed_ms(&absent_screen));
    assert_eq!(absent.status_elapsed, StatusElapsedProgress::NONE);
    assert_eq!(
        classify_state(&absent, working()).state,
        TimelineState::Idle,
        "a widget with no counter keeps the pre-existing idle verdict"
    );

    // A counter that restarts lower is a new turn, never inherited progress.
    tracker.observe(Some(30_000));
    let restarted_screen = claude_quiet_working_screen("2s");
    let mut restarted = quiescent_input(&restarted_screen);
    restarted.status_elapsed = tracker.observe(super::observed_status_elapsed_ms(&restarted_screen));
    assert_eq!(restarted.status_elapsed.previous_ms, None);
    assert_eq!(
        classify_state(&restarted, working()).state,
        TimelineState::Idle
    );
}

/// The live path derives progress from `TabStatusTracker::observe_status_elapsed`
/// over `extract_status_line(...).elapsed_ms`; the offline replay adapter derives
/// it from `StatusElapsedTracker::observe` over `observed_status_elapsed_ms`. Both
/// run here against one observation sequence covering advance, freeze, reset and
/// disappearance, and must agree on every pair and every verdict.
#[test]
fn live_and_offline_status_elapsed_sequences_match() {
    use crate::agent::status::TabStatusTracker;

    let screens = vec![
        claude_quiet_working_screen("14s"),
        claude_quiet_working_screen("20s"),
        claude_quiet_working_screen("20s"),
        claude_quiet_working_screen("2s"),
        claude_settled_screen(),
        claude_quiet_working_screen("3s"),
    ];

    let mut live_tracker = TabStatusTracker::new();
    live_tracker.update_output("pty-status-elapsed", [7u8; 32]);
    let mut offline_tracker = StatusElapsedTracker::new();

    let mut live_verdicts = Vec::new();
    let mut offline_verdicts = Vec::new();

    for screen in &screens {
        let live_progress = live_tracker.observe_status_elapsed(
            "pty-status-elapsed",
            super::extract_status_line(screen).and_then(|status| status.elapsed_ms),
        );
        let offline_progress = offline_tracker.observe(super::observed_status_elapsed_ms(screen));
        assert_eq!(
            live_progress, offline_progress,
            "live and offline must read the same elapsed pair"
        );

        let mut live_input = quiescent_input(screen);
        live_input.status_elapsed = live_progress;
        live_verdicts.push(classify_state(&live_input, working()));

        let mut offline_input = quiescent_input(screen);
        offline_input.status_elapsed = offline_progress;
        offline_verdicts.push(classify_state(&offline_input, working()));
    }

    assert_eq!(live_verdicts, offline_verdicts);
    assert_eq!(
        live_verdicts
            .iter()
            .map(|verdict| verdict.state)
            .collect::<Vec<_>>(),
        vec![
            TimelineState::Idle,
            TimelineState::Working,
            TimelineState::Idle,
            TimelineState::Idle,
            TimelineState::Idle,
            TimelineState::Idle,
        ],
        "only the advancing observation is Working"
    );
}

/// The same sequence through the REAL replay adapter: a painted grid, the
/// extractor's own quiescence clocks, and `classify_signals` threading the
/// shared tracker.
#[test]
fn cast_replay_status_elapsed_advance_keeps_working_after_bytes_go_quiet() {
    let builder = ClassifyInputBuilder::for_rows(StatusBarRows::fixture(0));
    let mut extractor = SignalExtractor::new(24, 80, builder.volatile_hint());
    let mut status_elapsed = StatusElapsedTracker::new();
    let mut previous = working();
    let mut states = Vec::new();

    for (index, elapsed) in ["14s", "20s", "20s"].iter().enumerate() {
        let painted_ms = index as u64 * 10_000;
        extractor.step(&paint(&claude_quiet_working_screen(elapsed)), painted_ms);
        let signals = extractor.step(b"", painted_ms + 6_000);
        assert_eq!(signals.quiescence.byte_idle_ms, 6_000);
        assert!(signals.quiescence.semantic_idle_ms >= 6_000);
        previous =
            super::classify_signals(&extractor, &signals, previous, &builder, &mut status_elapsed);
        states.push(previous.state);
    }

    assert_eq!(
        states,
        vec![
            TimelineState::Idle,
            TimelineState::Working,
            TimelineState::Idle,
        ],
        "replay agrees with the live oracle: only the advancing frame is Working"
    );
}
