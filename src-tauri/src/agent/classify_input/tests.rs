use std::cell::Cell;

use super::*;
use crate::agent::offline_state_classifier::{
    classify_state, composer_cursor_ready, TabVerdict, TimelineState,
};
use crate::agent::registry::AgentRegistry;
use crate::agent::signal_extractor::{SignalExtractor, StreamSignals};
use crate::agent::test_provider::MockProvider;

const ROWS: u16 = 10;
const COLS: u16 = 40;
const PROVIDER: &str = "mock";

fn registry_with(rows: u16) -> AgentRegistry {
    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(MockProvider::with_status_bar_rows(PROVIDER, rows)));
    registry
}

/// Offline replay shape: everything read off the replayed extractor.
fn offline_parts(signals: &StreamSignals) -> ClassifyParts<'_> {
    ClassifyParts {
        picker_question: None,
        screen_lines: &signals.screen.lines,
        tail: &signals.tail,
        byte_idle_ms: signals.quiescence.byte_idle_ms,
        semantic_idle_ms: signals.quiescence.semantic_idle_ms,
        motion: Motion::Still,
        cursor_hidden: signals.input_mode.cursor_hidden,
        composer_ready: composer_cursor_ready(
            signals.cursor.visible,
            signals.cursor.row,
            signals.rows,
        ),
    }
}

/// Paint one line inside the bottom band, at `row` (1-based), leaving the
/// content region untouched.
fn band_repaint(row: u16, frame: u8) -> Vec<u8> {
    format!("\x1b[{row};1H  working {frame}   ").into_bytes()
}

fn semantic_idle_after_band_repaints(builder: &ClassifyInputBuilder) -> u64 {
    let mut extractor = SignalExtractor::new(ROWS, COLS, builder.volatile_hint());
    extractor.step(b"\x1b[2J\x1b[H content line\r\n", 0);
    extractor.step(&band_repaint(ROWS, 1), 1_000);
    extractor.step(&band_repaint(ROWS, 2), 2_000);
    extractor.step(b"", 3_000).quiescence.semantic_idle_ms
}

#[test]
fn classify_input_is_identical_across_live_and_offline_call_shapes() {
    let registry = registry_with(3);
    let builder = ClassifyInputBuilder::for_provider(&registry, PROVIDER).expect("resolvable");

    let mut extractor = SignalExtractor::new(ROWS, COLS, builder.volatile_hint());
    extractor.step(b"\x1b[2J\x1b[H content line\r\n", 0);
    let signals = extractor.step(b"", 1_500);
    let offline = builder.build(offline_parts(&signals));

    // The live path reads the same observation from its own `ScreenState`
    // parser and the tracker's two clocks; only the source differs.
    let lines = signals.screen.lines.clone();
    let tail = signals.tail.clone();
    let live = builder.build(ClassifyParts {
        picker_question: None,
        screen_lines: &lines,
        tail: &tail,
        byte_idle_ms: signals.quiescence.byte_idle_ms,
        semantic_idle_ms: signals.quiescence.semantic_idle_ms,
        motion: Motion::Still,
        cursor_hidden: signals.input_mode.cursor_hidden,
        composer_ready: composer_cursor_ready(
            signals.cursor.visible,
            signals.cursor.row,
            signals.rows,
        ),
    });

    assert_eq!(live, offline, "live and offline built inputs must be equal");
    assert_eq!(
        live.status_bar_rows,
        StatusBarRows::resolve(&registry, PROVIDER).expect("resolvable"),
        "both shapes carry the band the registry reports"
    );
}

#[test]
fn classify_input_band_gates_the_semantic_clock_offline_used_to_ignore() {
    let registry = registry_with(3);
    let shared = ClassifyInputBuilder::for_provider(&registry, PROVIDER).expect("resolvable");
    // What offline did before the divergence closed: no band at all.
    let unbanded = ClassifyInputBuilder::for_rows(StatusBarRows::fixture(0));

    let with_band = semantic_idle_after_band_repaints(&shared);
    let without_band = semantic_idle_after_band_repaints(&unbanded);

    assert!(
        with_band > without_band,
        "a band-only repaint must not reset the semantic clock (banded {with_band}ms vs unbanded {without_band}ms)"
    );
}

#[test]
fn classify_input_status_bar_rows_is_required_with_no_default() {
    // The compile-time half: `ClassifyInput.status_bar_rows` is a required
    // field of a type with no `Default`, pinned by the `compile_fail` doctest
    // on `ClassifyInputBuilder::build`.
    // The runtime half: resolution of an unknown provider is a typed error and
    // never yields a value at all, least of all a zero.
    let registry = registry_with(3);
    let err = StatusBarRows::resolve(&registry, "ghost").unwrap_err();
    assert_eq!(
        err,
        StatusBarRowsError::UnresolvedProvider {
            agent_type: "ghost".to_string()
        }
    );
}

#[test]
fn classify_input_refuses_an_unresolvable_provider_without_classifying() {
    let registry = registry_with(3);
    let classified = Cell::new(false);

    let outcome = with_resolved_rows(&registry, "ghost", |builder| {
        classified.set(true);
        let lines: Vec<String> = Vec::new();
        classify_state(
            &builder.build(ClassifyParts {
                picker_question: None,
                screen_lines: &lines,
                tail: "",
                byte_idle_ms: 9_000,
                semantic_idle_ms: 9_000,
                motion: Motion::Still,
                cursor_hidden: false,
                composer_ready: false,
            }),
            TabVerdict {
                state: TimelineState::Idle,
                subtype: None,
            },
        )
    });

    assert!(outcome.is_err(), "unresolvable provider must refuse");
    assert!(
        !classified.get(),
        "no classification may occur for an unresolvable provider"
    );
}

#[test]
fn classify_input_status_bar_rows_round_trips_the_registry() {
    for declared in [0u16, 1, 2, 3, 7] {
        let registry = registry_with(declared);
        let builder = ClassifyInputBuilder::for_provider(&registry, PROVIDER).expect("resolvable");
        let reported = registry
            .try_status_bar_rows(PROVIDER)
            .expect("registry resolves a registered provider");
        assert_eq!(builder.status_bar_rows().get(), reported);
        assert_eq!(builder.volatile_hint().bottom_rows, reported);
        assert_eq!(reported, declared);
    }
}
