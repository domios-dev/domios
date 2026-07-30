//! WHAT: the turn transition lands on the status signal, not on a repaint.
//! WHY: three acceptance axes were observable live and confirmation still refused.
//! ORACLE: the recorded waits name which signal the acknowledgement path sampled.
use super::tests::{opts, tui};
use super::*;
use crate::service::chat_send_transaction::{run_chat_send, ChatSendLedger, ChatSendState};
use crate::service::delivery_error::DeliveryError;
use crate::service::inject_verify::InjectionResult;
use std::cell::Cell;

const PAYLOAD: &str = "Reply with exactly this token and nothing else: MARKER-1";
const RESPONSE_MARKER: &str = "● MARKER-1";

/// The live trace: the payload lands as one turn, the response marker renders,
/// the composer returns to baseline, and the screen then goes quiet. The turn
/// transition is published afterwards, on the status signal only.
struct QuietScreenAfterTurnTransition {
    generation: Cell<u64>,
    turn_generation: Cell<u64>,
    draft_visible: Cell<bool>,
    payload_writes: Cell<usize>,
    submit_key_writes: Cell<usize>,
    screen_waits: Cell<usize>,
    turn_waits: Cell<usize>,
    publishes_turn_transition: bool,
}

impl QuietScreenAfterTurnTransition {
    fn new(publishes_turn_transition: bool) -> Self {
        Self {
            generation: Cell::new(0),
            turn_generation: Cell::new(0),
            draft_visible: Cell::new(false),
            payload_writes: Cell::new(0),
            submit_key_writes: Cell::new(0),
            screen_waits: Cell::new(0),
            turn_waits: Cell::new(0),
            publishes_turn_transition,
        }
    }
}

impl SubmitTransport for QuietScreenAfterTurnTransition {
    fn observe(&self) -> Option<Observation> {
        Some(Observation {
            tail: if self.draft_visible.get() {
                format!("❯ {PAYLOAD}")
            } else {
                format!("{RESPONSE_MARKER}\n\n❯\u{a0}")
            },
        })
    }

    fn draft_evidence_count(&self, _text: &str) -> usize {
        usize::from(self.draft_visible.get())
    }

    fn generation(&self) -> u64 {
        self.generation.get()
    }

    fn wait_change(&self, _generation: u64, _deadline: Instant) -> bool {
        self.screen_waits.set(self.screen_waits.get() + 1);
        false
    }

    /// The paste repaint already marked the tab as working, exactly as the live
    /// tracker does, so a fresh-turn transition can never be the evidence.
    fn turn_started(&self) -> bool {
        true
    }

    fn turn_generation(&self) -> u64 {
        self.turn_generation.get()
    }

    fn wait_turn_change(&self, turn_generation: u64, _deadline: Instant) -> bool {
        self.turn_waits.set(self.turn_waits.get() + 1);
        if !self.publishes_turn_transition {
            return false;
        }
        if self.turn_generation.get() == turn_generation {
            self.turn_generation.set(turn_generation + 1);
        }
        true
    }

    fn write_paste(&self, text: &str, _deadline: Instant) -> Result<usize, InjectionError> {
        self.payload_writes.set(self.payload_writes.get() + 1);
        self.draft_visible.set(true);
        self.generation.set(self.generation.get() + 1);
        Ok(text.len())
    }

    fn write_submit_key(&self, _key: &[u8], _deadline: Instant) -> Result<(), InjectionError> {
        self.submit_key_writes.set(self.submit_key_writes.get() + 1);
        if !self.draft_visible.get() {
            return Ok(());
        }
        self.draft_visible.set(false);
        self.generation.set(self.generation.get() + 1);
        Ok(())
    }
}

#[test]
fn three_acceptance_axes_reach_confirmation_through_the_turn_signal() {
    let transport = QuietScreenAfterTurnTransition::new(true);
    let result = submit(&transport).expect("acceptance was observable");

    assert!(result.submit_observed);
    assert_eq!(transport.payload_writes.get(), 1);
    assert_eq!(transport.submit_key_writes.get(), 1);
    assert!(!transport.draft_visible.get(), "composer back to baseline");
    assert!(transport
        .observe()
        .is_some_and(|state| state.tail.contains(RESPONSE_MARKER)));
    assert_eq!(
        transport.turn_waits.get(),
        1,
        "the turn transition must be sampled on its own signal"
    );
}

#[test]
fn a_turn_that_never_advances_stays_unconfirmed() {
    let transport = QuietScreenAfterTurnTransition::new(false);
    let error = submit(&transport).expect_err("a drained composer alone is not acceptance");

    assert!(matches!(error, InjectionError::SubmitNeverConfirmed { .. }));
    assert_eq!(transport.payload_writes.get(), 1);
    assert!(transport.submit_key_writes.get() <= MAX_SUBMIT_KEY_WRITES);
}

/// Mirrors the production boundary: a submit that required acknowledgement and
/// did not observe it is rendered as uncertainty, never as success.
fn submit(transport: &QuietScreenAfterTurnTransition) -> Result<InjectionResult, InjectionError> {
    run(transport, PAYLOAD, true, tui(), opts()).and_then(|result| {
        let elapsed = result.elapsed;
        let tail = transport.observe().map(|state| state.tail).unwrap_or_default();
        result.require_submission(true, elapsed, tail)
    })
}

fn ledger_send(publishes_turn_transition: bool) -> (ChatSendState, Option<String>) {
    let ledger = ChatSendLedger::default();
    let pending = ledger.try_start("tab-1", PAYLOAD, None).unwrap();
    let transport = QuietScreenAfterTurnTransition::new(publishes_turn_transition);
    let status = run_chat_send(&ledger, pending, PAYLOAD, |_| {
        submit(&transport)
            .map(|_| ())
            .map_err(|error| DeliveryError::from_injection("tab-1", error))
    });
    assert_eq!(transport.payload_writes.get(), 1);
    (status.state, status.error)
}

#[test]
fn observable_acceptance_confirms_at_the_ledger_seam() {
    let (state, error) = ledger_send(true);

    assert_eq!(state, ChatSendState::Sent);
    assert_eq!(error, None);
}

#[test]
fn unacknowledged_submit_still_reaches_the_ledger_as_unconfirmed() {
    let (state, error) = ledger_send(false);

    assert_eq!(state, ChatSendState::Failed);
    assert!(error.is_some_and(|error| error.starts_with("submit_unconfirmed:")));
}
