//! Status wakes re-evaluate attributable turn evidence after the composer drains.
//! Recorded generations prove irrelevant status churn cannot acknowledge or spin.
use super::tests::{opts, tui};
use super::*;
use crate::service::chat_send_transaction::{run_chat_send, ChatSendLedger, ChatSendState};
use crate::service::delivery_error::DeliveryError;
use crate::service::inject_verify::InjectionResult;
use std::cell::{Cell, RefCell};

const PAYLOAD: &str = "Reply with exactly this token and nothing else: MARKER-1";
const RESPONSE_MARKER: &str = "● MARKER-1";

/// Models a quiet screen after the composer drains, with a submit-key write
/// either starting an attributable turn or leaving only lifecycle churn.
struct QuietScreenAfterTurnTransition {
    generation: Cell<u64>,
    turn_generation: Cell<u64>,
    turn_started: Cell<bool>,
    draft_visible: Cell<bool>,
    payload_writes: Cell<usize>,
    submit_key_writes: Cell<usize>,
    screen_waits: Cell<usize>,
    turn_waits: RefCell<Vec<(u64, Instant)>>,
    status_wakes_remaining: Cell<usize>,
    starts_turn_on_submit: bool,
}

impl QuietScreenAfterTurnTransition {
    fn with_status_wakes(status_wakes: usize, starts_turn_on_submit: bool) -> Self {
        Self {
            generation: Cell::new(0),
            turn_generation: Cell::new(0),
            turn_started: Cell::new(false),
            draft_visible: Cell::new(false),
            payload_writes: Cell::new(0),
            submit_key_writes: Cell::new(0),
            screen_waits: Cell::new(0),
            turn_waits: RefCell::new(Vec::new()),
            status_wakes_remaining: Cell::new(status_wakes),
            starts_turn_on_submit,
        }
    }

    fn starting_turn() -> Self {
        Self::with_status_wakes(1, true)
    }

    fn no_status_change() -> Self {
        Self::with_status_wakes(0, false)
    }

    fn irrelevant_status_then_timeout() -> Self {
        Self::with_status_wakes(1, false)
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

    fn turn_started(&self) -> bool {
        self.turn_started.get()
    }

    fn turn_generation(&self) -> u64 {
        self.turn_generation.get()
    }

    fn wait_turn_change(&self, turn_generation: u64, deadline: Instant) -> bool {
        self.turn_waits
            .borrow_mut()
            .push((turn_generation, deadline));
        if self.status_wakes_remaining.get() == 0 {
            return false;
        }
        self.status_wakes_remaining
            .set(self.status_wakes_remaining.get() - 1);
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
        if self.starts_turn_on_submit {
            self.turn_started.set(true);
            self.turn_generation.set(self.turn_generation.get() + 1);
        }
        Ok(())
    }
}

#[test]
fn three_acceptance_axes_confirm_the_submission_through_the_turn_signal() {
    let transport = QuietScreenAfterTurnTransition::starting_turn();
    let result = submit(&transport).expect("acceptance was observable");

    assert!(result.submit_observed);
    assert_eq!(transport.payload_writes.get(), 1);
    assert_eq!(transport.submit_key_writes.get(), MAX_SUBMIT_KEY_WRITES);
    assert!(!transport.draft_visible.get(), "composer back to baseline");
    assert!(transport
        .observe()
        .is_some_and(|state| state.tail.contains(RESPONSE_MARKER)));
    assert_eq!(
        transport.turn_waits.borrow().len(),
        0,
        "screen and status waits never gate the blind submit-key burst"
    );
}

#[test]
fn a_turn_that_never_advances_stays_unconfirmed() {
    let transport = QuietScreenAfterTurnTransition::no_status_change();
    let error = submit(&transport).expect_err("a drained composer alone is not acceptance");

    assert!(matches!(error, InjectionError::SubmitNeverConfirmed { .. }));
    assert_eq!(transport.payload_writes.get(), 1);
    assert!(transport.submit_key_writes.get() <= MAX_SUBMIT_KEY_WRITES);
}

#[test]
fn irrelevant_status_transition_does_not_acknowledge_or_spin_submission() {
    let transport = QuietScreenAfterTurnTransition::irrelevant_status_then_timeout();
    let error = submit(&transport).expect_err("status churn is wake-only");
    let waits = transport.turn_waits.borrow();

    assert!(matches!(error, InjectionError::SubmitNeverConfirmed { .. }));
    assert!(waits.is_empty(), "status churn never gates key recovery");
    assert_eq!(transport.submit_key_writes.get(), MAX_SUBMIT_KEY_WRITES);
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
    let transport = if publishes_turn_transition {
        QuietScreenAfterTurnTransition::starting_turn()
    } else {
        QuietScreenAfterTurnTransition::no_status_change()
    };
    let status = run_chat_send(&ledger, pending, PAYLOAD, |_| {
        submit(&transport)
            .map(|_| ())
            .map_err(|error| DeliveryError::from_injection("tab-1", error))
    });
    assert_eq!(transport.payload_writes.get(), 1);
    if publishes_turn_transition {
        assert_eq!(
            status.state,
            ChatSendState::Unknown,
            "transport acknowledgement waits for exact turn evidence"
        );
        let confirmed = ledger
            .confirm_delivered_user_turn("tab-1", "session-1", PAYLOAD)
            .unwrap();
        return (confirmed.state, confirmed.error);
    }
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

    assert_eq!(state, ChatSendState::Unknown);
    assert_eq!(error, None);
}
