//! Regression pins for the deleted Question machinery: the pre-guard bypass,
//! the substring confirm-footer engine, and the two-bracket Idle veto. Every
//! deletion pushes verdicts toward Idle, so the false-Idle guard below is the
//! load-bearing one.

use crate::agent::classify_input::{ClassifyInputBuilder, ClassifyParts, StatusBarRows};
use crate::agent::native_prompt::{
    pending_prompt_from_input, resolve_pending_prompt, NativePromptInput,
};
use crate::agent::offline_state_classifier::{
    classify_state, ClassifyInput, TabVerdict, TimelineState,
};
use crate::agent::prompt::PromptDetector;
use crate::agent::signal_extractor::Motion;
use crate::multiplexer::types::{PendingPrompt, PromptOption};

fn lines(rows: &[&str]) -> Vec<String> {
    rows.iter().map(|row| (*row).to_string()).collect()
}

fn idle() -> TabVerdict {
    TabVerdict {
        state: TimelineState::Idle,
        subtype: None,
    }
}

fn working() -> TabVerdict {
    TabVerdict {
        state: TimelineState::Working,
        subtype: None,
    }
}

fn settled(screen: &[String], cursor_hidden: bool) -> ClassifyInput<'_> {
    ClassifyInputBuilder::for_rows(StatusBarRows::fixture(0)).build(ClassifyParts {
        picker_question: None,
        screen_lines: screen,
        tail: "",
        byte_idle_ms: 6000,
        semantic_idle_ms: 6000,
        motion: Motion::Still,
        cursor_hidden,
        composer_ready: false,
    })
}

/// Verbatim from `testbench/replays/cursor/systems_design_long_chat_idle_v1
/// -20260621-systems-cursor-v2-cursor-...-120x32`, frame t_ms=342375 — a real
/// cursor follow-ups composer over numbered prose. `confirm_style_footer`
/// matched `↑` + "enter" + "esc" on the composer hint bar, and the deleted
/// pre-guard rung turned that into a Question with the cursor visible.
fn cursor_follow_ups_composer_frame() -> Vec<String> {
    lines(&[
        "",
        "  7.2 Compaction algorithm",
        "",
        "  1. Compute gc_watermark = min(peer.vv) over active peers",
        "  2. Collect tombstones T where op(T).seq < gc_watermark[site(T)]",
        "  3. For each removed T, rewrite pointers:",
        "       after == T  \u{2192}  after := visible_predecessor(T)",
        "  4. Physically delete T from map",
        "  5. Optionally merge adjacent runs with same (origin, site) metadata",
        "",
        " \u{250c}\u{2500} follow-ups \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2510}",
        " \u{2502} \u{25cb} \u{439}\u{43e}                                        \u{2502}",
        " \u{2502} enter send now \u{b7} \u{2191} select/edit \u{b7} esc cancel   \u{2502}",
        " \u{2514}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
        "",
    ])
}

#[test]
fn question_removal_corpus_bypass_frame_is_no_longer_question() {
    let screen = cursor_follow_ups_composer_frame();
    for cursor_hidden in [true, false] {
        let verdict = classify_state(&settled(&screen, cursor_hidden), idle());
        assert_ne!(
            verdict.state,
            TimelineState::Question,
            "a real composer hint bar over numbered prose must not be a Question \
             (cursor_hidden={cursor_hidden})"
        );
    }
}

/// The deleted engine matched by SUBSTRING: "esc" inside *describe*/*escape*/
/// *rescue*, "select" inside *selected*, "space" inside *namespace*.
#[test]
fn question_removal_substring_words_do_not_produce_question() {
    let cases: [(&str, &str); 5] = [
        (
            "describe",
            "  Please describe the change and press enter; the selected item is saved.",
        ),
        (
            "escape",
            "  Escape hatches let you enter a selected branch.",
        ),
        ("rescue", "  The rescue job will enter the selected queue."),
        (
            "selected",
            "  The selected namespace will enter a rescue window.",
        ),
        (
            "namespace",
            "  The namespace will enter a rescue window shortly.",
        ),
    ];
    for (word, footer) in cases {
        let screen = lines(&[
            "Summary of the plan:",
            "1. Alpha step",
            "2. Beta step",
            "3. Gamma step",
            footer,
        ]);
        for cursor_hidden in [true, false] {
            let verdict = classify_state(&settled(&screen, cursor_hidden), idle());
            assert_ne!(
                verdict.state,
                TimelineState::Question,
                "{word}: prose containing the substring must not read as a choice footer \
                 (cursor_hidden={cursor_hidden})"
            );
        }
    }
}

/// The deleted veto fired on any bottom line holding two `[` characters, so a
/// bracketed log line stranded an otherwise-idle composer.
#[test]
fn question_removal_bracketed_log_line_does_not_veto_idle() {
    let bar = "\u{2500}".repeat(72);
    let screen = lines(&[
        "\u{276f} Write the notes file, then say done.",
        "  Nothing else is pending.",
        bar.as_str(),
        "\u{276f}",
        bar.as_str(),
        "[INFO] [main] build finished in 2.1s",
    ]);
    let input = ClassifyInputBuilder::for_rows(StatusBarRows::fixture(0)).build(ClassifyParts {
        picker_question: None,
        screen_lines: &screen,
        tail: "",
        byte_idle_ms: 3000,
        semantic_idle_ms: 3000,
        motion: Motion::Still,
        cursor_hidden: false,
        composer_ready: false,
    });
    let verdict = classify_state(&input, working());
    assert_eq!(
        verdict.state,
        TimelineState::Idle,
        "a bracketed log line under a ready composer must not veto Idle"
    );
}

/// THE HARM-RANKING GUARD. A false Idle silently closes a worker turn, and
/// every deletion in this change pushes verdicts toward Idle. Verbatim from
/// `testbench/replays/claude/snake_incremental_build_browser_v1-20260621-085359`,
/// frame t_ms=3375, recorded Working with a live status widget.
#[test]
fn question_removal_working_corpus_frame_does_not_become_idle() {
    let bar = "\u{2500}".repeat(120);
    let screen = lines(&[
        "\u{276f} Create a minimal but complete browser Snake game in a single self-contained snake.html file (inline CSS + JS, canvas,",
        "  no external assets or libraries). Just create the file.",
        "\u{2500}\u{2500}",
        "\u{b7} Jitterbugging\u{2026} (2s \u{b7} thinking with high effort)",
        "",
        bar.as_str(),
        "\u{276f}",
        bar.as_str(),
        "  \u{23f5}\u{23f5} auto mode on (shift+tab to cycle) \u{b7} esc to interrupt             Bypass permissions mode was disabled by settings",
    ]);
    let input = ClassifyInputBuilder::for_rows(StatusBarRows::fixture(0)).build(ClassifyParts {
        picker_question: None,
        screen_lines: &screen,
        tail: "",
        byte_idle_ms: 200,
        semantic_idle_ms: 200,
        motion: Motion::Still,
        cursor_hidden: false,
        composer_ready: false,
    });
    let verdict = classify_state(&input, working());
    assert_eq!(
        verdict.state,
        TimelineState::Working,
        "a genuinely working corpus frame must not fall to Idle after the deletions"
    );
}

fn structured_prompt(question: &str) -> PendingPrompt {
    PendingPrompt {
        question: question.to_string(),
        options: vec![
            PromptOption {
                label: "Yes".to_string(),
                value: "1".to_string(),
            },
            PromptOption {
                label: "No".to_string(),
                value: "2".to_string(),
            },
        ],
        sections: None,
        multi_select: None,
        submit_available: None,
        selected_values: None,
    }
}

/// The live scrape surface, built the way the live path builds it.
fn scraped_picker_input() -> NativePromptInput {
    let mut parser = vt100::Parser::new(24, 80, 0);
    for line in [
        "Select an option",
        "",
        "\u{276f} 1. first choice",
        "  2. second choice",
        "  3. third choice",
        "",
        "  enter to select \u{b7} esc to cancel",
    ] {
        parser.process(line.as_bytes());
        parser.process(b"\r\n");
    }
    NativePromptInput {
        screen: codeterm_pty::attributed_screen::AttributedScreen::from_vt100(parser.screen()),
        cursor_hidden: true,
        alt_screen: false,
    }
}

#[test]
fn question_removal_scrape_without_structured_handle_resolves_to_nothing() {
    let input = scraped_picker_input();
    assert!(
        pending_prompt_from_input(&input).is_some(),
        "precondition: the structural detector accepts this screen"
    );
    assert!(
        resolve_pending_prompt(None, Some(&input)).is_none(),
        "a captured picker without a structured handle must not resolve"
    );

    let structured = structured_prompt("structured handle");
    let resolved = resolve_pending_prompt(Some(structured.clone()), Some(&input))
        .expect("the structured handle resolves");
    assert!(resolved.explicit, "a structured handle stays structured");
    assert_eq!(resolved.prompt, structured, "the structured handle wins");
}

#[test]
fn question_removal_scrape_does_not_create_detector_prompt() {
    let input = scraped_picker_input();

    let mut detector = PromptDetector::new();
    let scraped = resolve_pending_prompt(None, Some(&input));
    assert!(
        scraped.is_none(),
        "a captured picker must not resolve from scraping"
    );
    detector.evaluate_pty_output_parsed("tab-scrape", true, true, scraped);
    assert!(
        detector.active_prompt("tab-scrape").is_none(),
        "a scrape-derived prompt must not reach the detector"
    );

    let mut detector = PromptDetector::new();
    detector.hook_detected_explicit("tab-hook", structured_prompt("hook body"));
    assert_eq!(
        detector.active_prompt_explicit("tab-hook"),
        Some(true),
        "a hook-derived prompt still reports structured provenance"
    );

    let mut detector = PromptDetector::new();
    let structured = resolve_pending_prompt(Some(structured_prompt("handle")), Some(&input))
        .expect("structured resolves");
    detector.evaluate_pty_output_parsed("tab-structured", true, true, Some(structured));
    assert_eq!(
        detector.active_prompt_explicit("tab-structured"),
        Some(true),
        "a structured handle keeps its provenance through the live path"
    );
}

/// The hook screen-parse fallback is a scrape wearing a hook's clothes: it must
/// keep its no-clear ownership without claiming structured provenance.
#[test]
fn question_removal_hook_screen_parse_is_not_structured() {
    let mut detector = PromptDetector::new();
    detector.hook_detected_parse("tab-1", "proceed", |_| Some(structured_prompt("scraped")));
    assert_eq!(
        detector.active_prompt_explicit("tab-1"),
        Some(false),
        "a hook fallback parsed off the screen is a scrape, not a structured handle"
    );
    assert!(
        detector.active_prompt("tab-1").is_some(),
        "it still owns the card: a routine empty chunk must not clear it"
    );
}
