//! WHAT: submit-transaction behaviour against a captured collapsed-paste composer.
//! WHY: a TUI that hides the payload behind a placeholder is the only ingestion evidence there is.
//! ORACLE: recorded writes expose both a missed submit key and a draft that never left.
use super::tests::{opts, tui};
use super::*;
use crate::agent::provider::TurnEvidenceNormalization;
use crate::service::chat_send_transaction::normalize_turn_evidence_with;
use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};

/// Captured OpenCode frame in which the payload was collapsed to a paste
/// placeholder — the only ingestion evidence that TUI ever renders.
const CAPTURED_COLLAPSED_PASTE_FRAME: &str =
    include_str!("../../fixtures/captured_collapsed_paste_composer.txt");

const EMPTY_BORDERED_COMPOSER: &str = "  ┃\n  ┃  ›\n  ┃\n";
const CODEX_COLLAPSED_PASTE_FRAME: &str = "› [Pasted Content 1169 chars]";

/// Runs the production screen sensor over whole frames so the transaction sees
/// exactly what the composer renders before and after each write.
struct ScreenBackedComposer {
    frame: RefCell<&'static str>,
    after_paste: &'static str,
    after_enter: &'static str,
    starts_turn: bool,
    generation: Cell<u64>,
    turn: Cell<bool>,
    exact_session_turn: Cell<bool>,
    stored_exact_turn: RefCell<Option<(String, TurnEvidenceNormalization)>>,
    turn_after_wait: Cell<bool>,
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
            exact_session_turn: Cell::new(false),
            stored_exact_turn: RefCell::new(None),
            turn_after_wait: Cell::new(false),
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

    fn set_stored_exact_turn(&self, turn: &str, normalization: TurnEvidenceNormalization) {
        *self.stored_exact_turn.borrow_mut() = Some((turn.to_string(), normalization));
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

    fn has_exact_user_turn_evidence(&self) -> bool {
        true
    }

    fn exact_user_turn_present(&self, text: &str) -> bool {
        if !self.exact_session_turn.get() {
            return false;
        }
        self.stored_exact_turn
            .borrow()
            .as_ref()
            .is_none_or(|(stored, normalization)| {
                normalize_turn_evidence_with(text, *normalization)
                    == normalize_turn_evidence_with(stored, *normalization)
            })
    }

    fn wait_retry_deadline_for_turn(&self, _deadline: Instant, _text: &str) -> bool {
        if self.turn_after_wait.replace(false) {
            self.turn.set(true);
            self.exact_session_turn.set(true);
        }
        self.exact_session_turn.get()
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
        self.exact_session_turn.set(self.starts_turn);
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
    assert_eq!(
        io.enters(),
        1,
        "turn evidence stops the remaining key schedule"
    );
}

#[test]
fn codex_collapsed_pasted_content_is_submitted_without_rewriting_payload() {
    let io = ScreenBackedComposer::new(
        EMPTY_BORDERED_COMPOSER,
        CODEX_COLLAPSED_PASTE_FRAME,
        EMPTY_BORDERED_COMPOSER,
        true,
    );
    let result = run(&io, COLLAPSED_PASTE_PAYLOAD, true, tui(), opts())
        .expect("provider turn evidence confirms the collapsed paste");

    assert!(result.submit_observed);
    assert_eq!(io.pastes(), 1, "the payload is written at most once");
    assert_eq!(
        io.enters(),
        1,
        "no recovery key follows exact turn evidence"
    );
}

#[test]
fn claude_wrapped_multiline_turn_with_tabs_stops_the_key_schedule() {
    let io = ScreenBackedComposer::new(
        EMPTY_BORDERED_COMPOSER,
        "› [Pasted text #8 +3 lines]",
        EMPTY_BORDERED_COMPOSER,
        true,
    );
    io.set_stored_exact_turn(
        "<pasted_content id=\"8\">\nname       value\n        alpha    beta\nlast\n</pasted_content id=\"8\">",
        TurnEvidenceNormalization {
            unwrap_pasted_content_wrapper: true,
            collapse_horizontal_whitespace: true,
            drop_supplementary_characters: false,
        },
    );
    let payload = "name\tvalue\n\talpha\tbeta\nlast";
    let result = run(&io, payload, true, tui(), opts())
        .expect("the normalized exact Claude transcript turn confirms submission");

    assert!(result.submit_observed);
    assert_eq!(io.pastes(), 1);
    assert_eq!(io.enters(), 1);
}

#[test]
fn codex_astral_characters_dropped_from_turn_still_stop_the_key_schedule() {
    let io = ScreenBackedComposer::new(
        EMPTY_BORDERED_COMPOSER,
        CODEX_COLLAPSED_PASTE_FRAME,
        EMPTY_BORDERED_COMPOSER,
        true,
    );
    io.set_stored_exact_turn(
        "inspect  logs\nrepair  pipeline\n✅ BMP survives ",
        TurnEvidenceNormalization {
            drop_supplementary_characters: true,
            ..Default::default()
        },
    );
    let payload = "inspect 🚀 logs\nrepair 🧪 pipeline\n✅ BMP survives 🔥";
    let result = run(&io, payload, true, tui(), opts())
        .expect("Codex's provider-normalized exact turn confirms submission");

    assert!(result.submit_observed);
    assert_eq!(io.pastes(), 1);
    assert_eq!(io.enters(), 1);
}

#[test]
fn submit_stops_on_provider_turn_evidence() {
    let io = ScreenBackedComposer::new(
        EMPTY_BORDERED_COMPOSER,
        CODEX_COLLAPSED_PASTE_FRAME,
        CODEX_COLLAPSED_PASTE_FRAME,
        false,
    );
    io.turn_after_wait.set(true);
    let result = run(&io, COLLAPSED_PASTE_PAYLOAD, true, tui(), opts())
        .expect("exact provider turn evidence arrives during the key schedule");

    assert!(result.submit_observed);
    assert_eq!(io.pastes(), 1);
    assert_eq!(
        io.enters(),
        1,
        "the wait wakeup suppresses later Enter keys"
    );
}

#[test]
fn codex_bounded_schedule_reaches_the_observed_slow_ingestion_window() {
    let strategy = crate::agent::provider::VerifySubmitStrategy::TuiInputDrained {
        initial_key_delay: Duration::from_millis(300),
        retry_offsets: [200, 500, 1200, 2200, 4000, 5800].map(Duration::from_millis),
    };

    let final_key_offset = strategy.initial_key_delay() + strategy.retry_offsets()[5];
    assert!(
        final_key_offset >= Duration::from_millis(5900),
        "the seventh key attempt is scheduled after the observed 5.9s ingestion case"
    );
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
    assert_eq!(io.enters(), 1);
}

#[test]
fn non_tui_shell_send_keeps_one_command_write_and_stops_after_echo_drains() {
    let io = ScreenBackedComposer::new(
        EMPTY_BORDERED_COMPOSER,
        CODEX_COLLAPSED_PASTE_FRAME,
        EMPTY_BORDERED_COMPOSER,
        true,
    );
    let result = run(
        &io,
        "git status --short",
        true,
        VerifySubmitStrategy::None,
        opts(),
    )
    .expect("the shell send follows the existing submit contract");

    assert!(result.submit_observed);
    assert_eq!(io.pastes(), 1);
    assert_eq!(io.enters(), 1);
}

#[test]
fn opencode_collapsed_paste_keeps_its_existing_single_submit_path() {
    let io = ScreenBackedComposer::new(
        EMPTY_BORDERED_COMPOSER,
        CAPTURED_COLLAPSED_PASTE_FRAME,
        EMPTY_BORDERED_COMPOSER,
        true,
    );
    let result = run(&io, COLLAPSED_PASTE_PAYLOAD, true, tui(), opts())
        .expect("OpenCode's exact structured turn evidence stops the retry schedule");

    assert!(result.submit_observed);
    assert_eq!(io.pastes(), 1);
    assert_eq!(io.enters(), 1);
}

#[test]
fn claude_capability_keeps_collapsed_paste_single_write() {
    let io = ScreenBackedComposer::new(
        EMPTY_BORDERED_COMPOSER,
        CAPTURED_COLLAPSED_PASTE_FRAME,
        EMPTY_BORDERED_COMPOSER,
        true,
    );
    let claude_strategy = VerifySubmitStrategy::TuiInputDrained {
        initial_key_delay: Duration::from_millis(100),
        retry_offsets: crate::agent::provider::DEFAULT_SUBMIT_RETRY_OFFSETS,
    };
    let result = run(&io, COLLAPSED_PASTE_PAYLOAD, true, claude_strategy, opts())
        .expect("Claude's existing TUI submission contract still submits once");

    assert!(result.submit_observed);
    assert_eq!(io.pastes(), 1);
    assert_eq!(io.enters(), 1);
}
