//! WHAT: submit-transaction behaviour against a captured collapsed-paste composer.
//! WHY: a TUI that hides the payload behind a placeholder is the only ingestion evidence there is.
//! ORACLE: recorded writes expose both a missed submit key and a draft that never left.
use super::tests::{opts, tui};
use super::*;
use std::cell::{Cell, RefCell};
use std::time::Duration;

/// Captured OpenCode frame in which the payload was collapsed to a paste
/// placeholder — the only ingestion evidence that TUI ever renders.
const CAPTURED_COLLAPSED_PASTE_FRAME: &str =
    include_str!("../../fixtures/captured_collapsed_paste_composer.txt");

const EMPTY_BORDERED_COMPOSER: &str = "  ┃\n  ┃  ›\n  ┃\n";

/// Runs the production screen sensor over whole frames so the transaction sees
/// exactly what the composer renders before and after each write.
struct ScreenBackedComposer {
    frame: RefCell<&'static str>,
    after_paste: &'static str,
    after_enter: &'static str,
    starts_turn: bool,
    generation: Cell<u64>,
    turn: Cell<bool>,
    writes: RefCell<Vec<String>>,
}

impl ScreenBackedComposer {
    fn new(
        frame: &'static str,
        after_paste: &'static str,
        after_enter: &'static str,
        starts_turn: bool,
    ) -> Self {
        Self {
            frame: RefCell::new(frame),
            after_paste,
            after_enter,
            starts_turn,
            generation: Cell::new(0),
            turn: Cell::new(false),
            writes: RefCell::new(Vec::new()),
        }
    }

    fn enters(&self) -> usize {
        self.writes
            .borrow()
            .iter()
            .filter(|write| write.as_str() == "enter")
            .count()
    }

    fn pastes(&self) -> usize {
        self.writes
            .borrow()
            .iter()
            .filter(|write| write.starts_with("paste:"))
            .count()
    }
}

impl SubmitTransport for ScreenBackedComposer {
    fn observe(&self) -> Option<Observation> {
        Some(Observation {
            tail: self.frame.borrow().to_string(),
        })
    }

    fn draft_evidence_count(&self, text: &str) -> usize {
        if text.is_empty() {
            return 1;
        }
        let lines = self
            .frame
            .borrow()
            .lines()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let cursor_row = lines.len().saturating_sub(1);
        screen_echo_evidence_count(&composer_region_text(&lines, cursor_row), text)
    }

    fn generation(&self) -> u64 {
        self.generation.get()
    }

    fn wait_change(&self, _generation: u64, _deadline: Instant) -> bool {
        false
    }

    fn turn_started(&self) -> bool {
        self.turn.get()
    }

    fn write_paste(&self, text: &str, _deadline: Instant) -> Result<usize, InjectionError> {
        if text.is_empty() {
            return Ok(0);
        }
        *self.frame.borrow_mut() = self.after_paste;
        self.generation.set(self.generation.get() + 1);
        self.writes.borrow_mut().push(format!("paste:{text}"));
        Ok(text.len())
    }

    fn write_submit_key(&self, key: &[u8], _deadline: Instant) -> Result<(), InjectionError> {
        *self.frame.borrow_mut() = self.after_enter;
        self.generation.set(self.generation.get() + 1);
        self.turn.set(self.starts_turn);
        let name = if key == b"\t" { "tab" } else { "enter" };
        self.writes.borrow_mut().push(name.to_string());
        Ok(())
    }
}

/// The live log preserved only the first 120 chars of the delivered payload;
/// the rest is filler that keeps it in the same size class OpenCode collapsed.
const COLLAPSED_PASTE_PAYLOAD: &str = concat!(
    "Please read this deliberately long wrapped composer payload without ",
    "altering it: alpha beta gamma delta epsilon zeta eta theta iota kappa ",
    "lambda mu nu xi omicron pi rho sigma tau upsilon phi chi psi omega",
);

#[test]
fn collapsed_paste_frame_releases_the_submit_key_exactly_once() {
    let io = ScreenBackedComposer::new(
        EMPTY_BORDERED_COMPOSER,
        CAPTURED_COLLAPSED_PASTE_FRAME,
        EMPTY_BORDERED_COMPOSER,
        true,
    );
    let result = run(&io, COLLAPSED_PASTE_PAYLOAD, true, tui(), opts())
        .expect("collapsed paste is ingestion evidence");

    assert!(result.echo_observed);
    assert!(result.submit_observed);
    assert_eq!(io.pastes(), 1);
    assert_eq!(io.enters(), MAX_SUBMIT_KEY_WRITES);
}

#[test]
fn collapsed_draft_that_never_leaves_the_composer_is_never_a_confirmed_submit() {
    let io = ScreenBackedComposer::new(
        EMPTY_BORDERED_COMPOSER,
        CAPTURED_COLLAPSED_PASTE_FRAME,
        CAPTURED_COLLAPSED_PASTE_FRAME,
        false,
    );
    let result = run(&io, COLLAPSED_PASTE_PAYLOAD, true, tui(), opts())
        .expect("collapsed paste is ingestion evidence");
    let result = result.require_submission(true, Duration::ZERO, "> parked".into());

    assert!(
        matches!(&result, Err(InjectionError::SubmitNeverConfirmed { .. })),
        "{result:?}"
    );
    assert_eq!(io.pastes(), 1, "the payload is written exactly once");
    assert_eq!(io.enters(), MAX_SUBMIT_KEY_WRITES);
}

#[test]
fn key_only_submit_never_verifies_an_empty_payload_echo() {
    let io = ScreenBackedComposer::new(
        CAPTURED_COLLAPSED_PASTE_FRAME,
        CAPTURED_COLLAPSED_PASTE_FRAME,
        EMPTY_BORDERED_COMPOSER,
        true,
    );
    let result = run(&io, "", true, tui(), opts()).expect("a key-only submit has no payload echo");

    assert_eq!(result.bytes_written, 0);
    assert!(result.submit_observed);
    assert_eq!(io.pastes(), 0, "a key-only submit never rewrites a payload");
    assert_eq!(io.enters(), MAX_SUBMIT_KEY_WRITES);
}
