use super::*;
use crate::service::chat_send_transaction::{ChatSendLedger, ChatSendState, SendLiveBinding};
use std::cell::Cell;

struct Contention {
    claim: SubmitCycleClaim,
    writes: Cell<usize>,
    keys: Cell<usize>,
    evidence: Cell<bool>,
}

struct Refused;

impl SubmitTransport for Refused {
    fn prewrite_guard(&self) -> Result<(), InjectionError> {
        Err(InjectionError::AgentInputUnavailable {
            pty_id: "contention".into(),
        })
    }
    fn observe(&self) -> Option<Observation> {
        None
    }
    fn draft_evidence_count(&self, _: &str) -> usize {
        0
    }
    fn generation(&self) -> u64 {
        0
    }
    fn wait_change(&self, _: u64, _: Instant) -> bool {
        false
    }
    fn turn_started(&self) -> bool {
        false
    }
    fn write_paste(&self, _: &str, _: Instant) -> Result<usize, InjectionError> {
        panic!("refused delivery cannot enqueue its payload")
    }
    fn write_submit_key(&self, _: &[u8], _: Instant) -> Result<(), InjectionError> {
        panic!("refused delivery cannot issue keys")
    }
}

impl SubmitTransport for Contention {
    fn observe(&self) -> Option<Observation> {
        None
    }
    fn draft_evidence_count(&self, _: &str) -> usize {
        self.writes.get()
    }
    fn generation(&self) -> u64 {
        0
    }
    fn wait_change(&self, _: u64, _: Instant) -> bool {
        false
    }
    fn turn_started(&self) -> bool {
        false
    }
    fn retry_active(&self) -> bool {
        self.claim.is_current()
    }
    fn exact_user_turn_present(&self, text: &str) -> bool {
        text == "opaque-owner-message" && self.evidence.get()
    }
    fn write_paste(&self, text: &str, _: Instant) -> Result<usize, InjectionError> {
        self.writes.set(self.writes.get() + 1);
        let (sender, _) = mpsc::sync_channel(1);
        self.claim.payload_enqueued(sender);
        Ok(text.len())
    }
    fn write_submit_key(&self, key: &[u8], _: Instant) -> Result<(), InjectionError> {
        assert_eq!(key, b"\r");
        self.keys.set(self.keys.get() + 1);
        if self.keys.get() == 1 {
            let refused = claim_submit_cycle(self.claim.pty_id());
            assert!(run(&Refused, "routed-report", true, strategy(), options()).is_err());
            drop(refused);
            assert!(self.claim.is_current());
        }
        if self.keys.get() == 3 {
            self.evidence.set(true);
        }
        Ok(())
    }
}

fn strategy() -> VerifySubmitStrategy {
    VerifySubmitStrategy::TuiInputDrained {
        initial_key_delay: Duration::ZERO,
        retry_offsets: [Duration::ZERO; 6],
    }
}

fn options() -> VerifyOpts {
    VerifyOpts {
        require_prompt_first: false,
        verify_echo: false,
        verify_submit: true,
        ..Default::default()
    }
}

#[test]
fn refused_concurrent_delivery_preserves_remaining_enters_and_exact_delivery_evidence() {
    let ledger = ChatSendLedger::default();
    let send = ledger
        .try_start_scoped(
            "owner-tab",
            "opaque-owner-message",
            None,
            Some(SendLiveBinding {
                session_id: Some("own-session".into()),
                binding_nonce: Some("launch".into()),
            }),
        )
        .unwrap();
    let transport = Contention {
        claim: claim_submit_cycle("refused-concurrent-delivery"),
        writes: Cell::new(0),
        keys: Cell::new(0),
        evidence: Cell::new(false),
    };
    let result = run(
        &transport,
        "opaque-owner-message",
        true,
        strategy(),
        options(),
    )
    .unwrap();
    assert_eq!(transport.writes.get(), 1);
    assert_eq!(result.submit_key_writes, 3);
    assert!(result.submit_observed);
    ledger.finish(&send.send_id, Err("submit_unconfirmed: late turn".into()));
    assert_eq!(
        ledger.status(&send.send_id).unwrap().state,
        ChatSendState::Unknown
    );
    assert!(ledger
        .confirm_delivered_user_turns_scoped(
            "owner-tab",
            "foreign-session",
            Some("launch"),
            "opaque-owner-message",
        )
        .is_empty());
    let confirmed = ledger.confirm_delivered_user_turns_scoped(
        "owner-tab",
        "own-session",
        Some("launch"),
        "opaque-owner-message",
    );
    assert_eq!(confirmed.len(), 1);
    assert_eq!(confirmed[0].state, ChatSendState::Sent);
    ledger.finish(
        &send.send_id,
        Err("submit_unconfirmed: transport finished later".into()),
    );
    assert_eq!(
        ledger.status(&send.send_id).unwrap().state,
        ChatSendState::Sent
    );
}
