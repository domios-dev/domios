//! WHAT: dropped-Enter recovery at the transport and ledger seams.
//! WHY: a provider that drops one CR while the draft stays parked in the
//! composer must be recovered key-only within the turn deadline; a genuine
//! loss keeps its honest submit_unconfirmed classification.
//! ORACLE: recorded writes, ledger state, and the exact error variant.
use super::tests::{opts, tui};
use super::*;
use std::cell::Cell;
use std::sync::{Condvar, Mutex};
use std::time::Duration;

/// Models the captured production trace (prod log 2026-07-30, tab 166df18d):
/// paste ingested and rendered, Enter written and dropped by the TUI, draft
/// still parked in the composer at the turn deadline. `wait_change` honors the
/// production contract of `PtySubmitTransport`: with no new frame it returns
/// false only once the passed failure deadline has expired.
struct ParkedDraft {
    drops_remaining: Cell<usize>,
    starts_turn: bool,
    draft_visible: Cell<bool>,
    turn: Cell<bool>,
    generation: Cell<u64>,
    turn_generation: Cell<u64>,
    payload_writes: Cell<usize>,
    submit_key_writes: Cell<usize>,
    idle: Mutex<()>,
    never_signaled: Condvar,
}

impl ParkedDraft {
    fn dropping(drops: usize) -> Self {
        Self {
            drops_remaining: Cell::new(drops),
            starts_turn: true,
            draft_visible: Cell::new(false),
            turn: Cell::new(false),
            generation: Cell::new(0),
            turn_generation: Cell::new(0),
            payload_writes: Cell::new(0),
            submit_key_writes: Cell::new(0),
            idle: Mutex::new(()),
            never_signaled: Condvar::new(),
        }
    }

    fn lifecycle_only_drain() -> Self {
        Self {
            starts_turn: false,
            ..Self::dropping(0)
        }
    }
}

impl SubmitTransport for ParkedDraft {
    fn observe(&self) -> Option<Observation> {
        let tail = if self.turn.get() {
            "• Working (5s • esc to interrupt)".to_string()
        } else if self.draft_visible.get() {
            "› run /review on my current changes".to_string()
        } else {
            "› ".to_string()
        };
        Some(Observation { tail })
    }

    fn draft_evidence_count(&self, _text: &str) -> usize {
        usize::from(self.draft_visible.get())
    }

    fn generation(&self) -> u64 {
        self.generation.get()
    }

    fn wait_change(&self, generation: u64, deadline: Instant) -> bool {
        if self.generation.get() != generation {
            return true;
        }
        let mut guard = self.idle.lock().unwrap();
        loop {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return self.generation.get() != generation;
            };
            let (next, timeout) = self.never_signaled.wait_timeout(guard, remaining).unwrap();
            guard = next;
            if self.generation.get() != generation {
                return true;
            }
            if timeout.timed_out() {
                return false;
            }
        }
    }

    fn turn_started(&self) -> bool {
        self.turn.get()
    }

    fn turn_generation(&self) -> u64 {
        self.turn_generation.get()
    }

    fn write_paste(&self, text: &str, _deadline: Instant) -> Result<usize, InjectionError> {
        self.payload_writes.set(self.payload_writes.get() + 1);
        self.draft_visible.set(true);
        self.generation.set(self.generation.get() + 1);
        Ok(text.len())
    }

    fn write_submit_key(&self, _key: &[u8], _deadline: Instant) -> Result<(), InjectionError> {
        self.submit_key_writes.set(self.submit_key_writes.get() + 1);
        if self.drops_remaining.get() > 0 {
            self.drops_remaining.set(self.drops_remaining.get() - 1);
            return Ok(());
        }
        self.draft_visible.set(false);
        if self.starts_turn {
            self.turn.set(true);
            self.turn_generation.set(self.turn_generation.get() + 1);
        } else {
            self.turn_generation.set(self.turn_generation.get() + 2);
        }
        self.generation.set(self.generation.get() + 1);
        Ok(())
    }
}

fn short_opts() -> VerifyOpts {
    VerifyOpts {
        echo_timeout: Duration::ZERO,
        submit_timeout: Duration::from_millis(250),
        ..opts()
    }
}

#[test]
fn parked_draft_after_dropped_enter_recovers_before_turn_deadline() {
    let io = ParkedDraft::dropping(1);

    let result = run(
        &io,
        "run /review on my current changes",
        true,
        tui(),
        short_opts(),
    )
    .unwrap();

    assert_eq!(io.payload_writes.get(), 1, "the payload is written once");
    assert!(
        result.submit_observed,
        "a dropped Enter with the draft parked in the composer must be \
         recovered key-only within the turn deadline; writes={} anomaly={}",
        io.submit_key_writes.get(),
        result.submit_anomaly
    );
    assert!(io.turn.get(), "the recovered Enter starts a real turn");
    assert_eq!(io.submit_key_writes.get(), MAX_SUBMIT_KEY_WRITES);
}

#[test]
fn restart_status_transitions_without_a_turn_do_not_acknowledge_submission() {
    let mut tracker = codeterm_types::pty::TabStatusTracker::new();
    tracker.update_output("pty-restart", [1; 32]);
    tracker.set_idle("pty-restart");
    let signal = tracker.arm_status_change_signal("pty-restart");
    let before_restart = signal.generation();
    tracker.set_dead("pty-restart");
    tracker.set_waiting("pty-restart");

    assert_eq!(
        signal.generation(),
        before_restart + 2,
        "restart lifecycle transitions advance the generic status signal"
    );
    let io = ParkedDraft::lifecycle_only_drain();

    let result = run(&io, "post-restart payload", true, tui(), short_opts()).unwrap();

    assert_eq!(io.payload_writes.get(), 1);
    assert!(io.turn_generation.get() >= 2);
    assert!(!result.submit_observed);
    assert!(result.submit_anomaly);
}

#[test]
fn recovered_submit_reaches_sent_only_after_exact_turn_evidence() {
    use crate::service::chat_send_transaction::{run_chat_send, ChatSendLedger, ChatSendState};
    use crate::service::delivery_error::DeliveryError;

    let ledger = ChatSendLedger::default();
    let pending = ledger
        .try_start("tab-1", "run /review on my current changes", None)
        .unwrap();
    let io = ParkedDraft::dropping(1);

    let status = run_chat_send(
        &ledger,
        pending,
        "run /review on my current changes",
        |text| {
            run(&io, text, true, tui(), short_opts())
                .and_then(|result| {
                    let elapsed = result.elapsed;
                    let tail = io.observe().map(|o| o.tail).unwrap_or_default();
                    result.require_submission(true, elapsed, tail)
                })
                .map(|_| ())
                .map_err(|error| DeliveryError::from_injection("tab-1", error))
        },
    );

    assert_eq!(io.payload_writes.get(), 1);
    assert_eq!(
        status.state,
        ChatSendState::Unknown,
        "transport success stays unknown without transcript evidence, got {:?}",
        status.error
    );
    let confirmed = ledger
        .confirm_delivered_user_turn("tab-1", "session-1", "run /review on my current changes")
        .unwrap();
    assert_eq!(confirmed.state, ChatSendState::Sent);
}

#[test]
fn unrecoverable_dropped_enter_stays_submit_unconfirmed_with_single_payload() {
    let io = ParkedDraft::dropping(usize::MAX);

    let result = run(&io, "held message", true, tui(), short_opts()).unwrap();

    assert_eq!(
        io.payload_writes.get(),
        1,
        "recovery is key-only; the payload is never re-pasted"
    );
    assert!(io.submit_key_writes.get() <= MAX_SUBMIT_KEY_WRITES);
    assert!(!result.submit_observed);
    assert!(result.submit_anomaly);

    let surfaced = result
        .require_submission(true, Duration::ZERO, "› held message".into())
        .unwrap_err();
    assert!(
        matches!(surfaced, InjectionError::SubmitNeverConfirmed { .. }),
        "a genuinely lost Enter keeps its honest classification, got {surfaced:?}"
    );
    let delivery = crate::service::delivery_error::DeliveryError::from_injection("tab-1", surfaced);
    assert_eq!(delivery.code(), "submit_unconfirmed");
    assert!(delivery.bytes_written());
}
