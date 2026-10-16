use super::classify_input::*;
use crate::agent::offline_state_classifier::{StatusElapsedProgress, composer_cursor_ready};
use crate::agent::registry::AgentRegistry;
use crate::agent::signal_extractor::{Motion, SignalExtractor, StreamSignals};
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
        status_elapsed: StatusElapsedProgress::NONE,
    }
}

/// Paint one line inside the bottom band, at `row` (1-based), leaving the
/// content region untouched.
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
        status_elapsed: StatusElapsedProgress::NONE,
    });

    assert_eq!(live, offline, "live and offline built inputs must be equal");
    assert_eq!(
        live.status_bar_rows,
        StatusBarRows::resolve(&registry, PROVIDER).expect("resolvable"),
        "both shapes carry the band the registry reports"
    );
}
