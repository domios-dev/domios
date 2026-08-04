//! WHAT: post-paste classification invariants at the transport seam.
//! WHY: once the payload reached the PTY, no later failure may claim a
//! zero-write terminal (`pty_unavailable`) — the dev false-Retry incident.
//! ORACLE: recorded writes plus the exact InjectionError variant returned.
use super::tests::{opts, tui};
use super::*;
use std::cell::{Cell, RefCell};
use std::time::Duration;

struct RevokedAfterPaste {
    generation: Cell<u64>,
    writes: RefCell<Vec<String>>,
}

impl SubmitTransport for RevokedAfterPaste {
    fn observe(&self) -> Option<Observation> {
        Some(Observation { tail: "> ".into() })
    }

    fn draft_evidence_count(&self, _text: &str) -> usize {
        usize::from(self.generation.get() > 0)
    }

    fn generation(&self) -> u64 {
        self.generation.get()
    }

    fn wait_change(&self, _generation: u64, _deadline: Instant) -> bool {
        false
    }

    fn turn_started(&self) -> bool {
        false
    }

    fn write_paste(&self, text: &str, _deadline: Instant) -> Result<usize, InjectionError> {
        self.generation.set(1);
        self.writes.borrow_mut().push(format!("paste:{text}"));
        Ok(text.len())
    }

    fn write_submit_key(&self, _key: &[u8], _deadline: Instant) -> Result<(), InjectionError> {
        Err(InjectionError::AgentInputUnavailable {
            pty_id: "guarded-pty".into(),
        })
    }
}

#[test]
fn revoked_after_paste_is_submit_uncertainty_with_no_recovery_writes() {
    let io = RevokedAfterPaste {
        generation: Cell::new(0),
        writes: RefCell::new(Vec::new()),
    };
    let error = run(&io, "envelope", true, tui(), opts()).unwrap_err();
    // The paste already reached the PTY: revocation afterwards is unconfirmed
    // submission, never a zero-write terminal claim.
    assert!(matches!(error, InjectionError::SubmitNeverConfirmed { .. }));
    assert_eq!(&*io.writes.borrow(), &["paste:envelope"]);
}

struct PtyDiesAfterPaste {
    generation: Cell<u64>,
    payload_writes: Cell<usize>,
    submit_key_attempts: Cell<usize>,
}

impl SubmitTransport for PtyDiesAfterPaste {
    fn observe(&self) -> Option<Observation> {
        Some(Observation { tail: "> ".into() })
    }

    fn draft_evidence_count(&self, _text: &str) -> usize {
        usize::from(self.generation.get() > 0)
    }

    fn generation(&self) -> u64 {
        self.generation.get()
    }

    fn wait_change(&self, _generation: u64, _deadline: Instant) -> bool {
        false
    }

    fn turn_started(&self) -> bool {
        false
    }

    fn write_paste(&self, text: &str, _deadline: Instant) -> Result<usize, InjectionError> {
        self.payload_writes.set(self.payload_writes.get() + 1);
        self.generation.set(1);
        Ok(text.len())
    }

    fn write_submit_key(&self, _key: &[u8], _deadline: Instant) -> Result<(), InjectionError> {
        self.submit_key_attempts.set(self.submit_key_attempts.get() + 1);
        Err(InjectionError::PtyClosed {
            pty_id: "died-after-paste".into(),
        })
    }
}

/// The dev incident shape: the payload reached the PTY and the provider kept
/// running, yet a later submit-key write error was reported as "pty closed" and
/// rendered as pty_unavailable with Retry after successful delivery.
#[test]
fn post_paste_submit_key_failure_never_claims_pty_unavailable() {
    let io = PtyDiesAfterPaste {
        generation: Cell::new(0),
        payload_writes: Cell::new(0),
        submit_key_attempts: Cell::new(0),
    };
    let error = run(&io, "delivered turn", true, tui(), opts()).unwrap_err();

    assert!(
        matches!(error, InjectionError::SubmitNeverConfirmed { .. }),
        "post-paste write failures must classify as submit uncertainty, got {error:?}"
    );
    assert_eq!(
        io.payload_writes.get(),
        1,
        "the payload is written exactly once"
    );
    assert_eq!(
        io.submit_key_attempts.get(),
        1,
        "no blind submit-key retries after a write failure"
    );

    let delivery = crate::service::delivery_error::DeliveryError::from_injection("tab-1", error);
    assert_eq!(delivery.code(), "submit_unconfirmed");
    assert!(delivery.bytes_written());
}

#[test]
fn write_ack_timeout_is_uncertainty_and_structural_errors_stay_terminal() {
    assert!(matches!(
        injection_write_error("p1", "PTY write acknowledgement timed out".into()),
        InjectionError::SubmitNeverConfirmed { .. }
    ));
    assert!(matches!(
        injection_write_error("p1", "stale agent input permit".into()),
        InjectionError::AgentInputUnavailable { .. }
    ));
    assert!(matches!(
        injection_write_error("p1", "PTY not found: p1".into()),
        InjectionError::PtyClosed { .. }
    ));
    assert!(matches!(
        injection_write_error("p1", "PTY write channel closed".into()),
        InjectionError::PtyClosed { .. }
    ));
}

struct SwallowsEveryEnter {
    generation: Cell<u64>,
    draft_visible: Cell<bool>,
    payload_writes: Cell<usize>,
    submit_key_writes: Cell<usize>,
}

impl SubmitTransport for SwallowsEveryEnter {
    fn observe(&self) -> Option<Observation> {
        Some(Observation { tail: "> ".into() })
    }

    fn draft_evidence_count(&self, _text: &str) -> usize {
        usize::from(self.draft_visible.get())
    }

    fn generation(&self) -> u64 {
        self.generation.get()
    }

    fn wait_change(&self, _generation: u64, _deadline: Instant) -> bool {
        false
    }

    fn turn_started(&self) -> bool {
        false
    }

    fn write_paste(&self, text: &str, _deadline: Instant) -> Result<usize, InjectionError> {
        self.payload_writes.set(self.payload_writes.get() + 1);
        self.draft_visible.set(true);
        self.generation.set(self.generation.get() + 1);
        Ok(text.len())
    }

    fn write_submit_key(&self, _key: &[u8], _deadline: Instant) -> Result<(), InjectionError> {
        self.submit_key_writes.set(self.submit_key_writes.get() + 1);
        Ok(())
    }
}

/// The dev incident's second defect: with the shared phase deadline already
/// expired, retry iterations still enqueued submit-key writes whose ~0 ack
/// waits produced spurious terminal errors. A retry write may never begin
/// past the deadline; the one delivery CR still goes out.
#[test]
fn expired_phase_deadline_stops_retry_writes_before_enqueue() {
    let io = SwallowsEveryEnter {
        generation: Cell::new(0),
        draft_visible: Cell::new(false),
        payload_writes: Cell::new(0),
        submit_key_writes: Cell::new(0),
    };
    let result = run(
        &io,
        "one delivery keypress",
        true,
        tui(),
        VerifyOpts {
            submit_timeout: Duration::ZERO,
            ..opts()
        },
    )
    .unwrap();

    assert_eq!(io.payload_writes.get(), 1);
    assert_eq!(
        io.submit_key_writes.get(),
        1,
        "no retry write may begin with an expired deadline"
    );
    assert!(!result.submit_observed);
    assert!(result.submit_anomaly, "uncertainty stays honest, not fatal");
}
