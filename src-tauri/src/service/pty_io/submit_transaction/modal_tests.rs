use super::readiness::{payload_decision, PayloadDecision, DELIVERY_MODAL_OPEN};
use super::*;
use crate::service::chat_send_transaction::{ChatSendLedger, ChatSendState, SendLiveBinding};
use crate::service::output_feed::delivery_readiness::{DeliveryReadiness, DeliveryReadinessEvents};

struct ModalInput {
    status: Option<TabStatus>,
    modal: Cell<bool>,
    open_during_preparation: Cell<bool>,
    claim: SubmitCycleClaim,
    payloads: RefCell<Vec<String>>,
    keys: Cell<usize>,
}

impl ModalInput {
    fn guard(&self) -> Result<(), InjectionError> {
        match payload_decision(self.modal.get()) {
            PayloadDecision::Write => Ok(()),
            PayloadDecision::HoldModal => Err(InjectionError::AgentTuiPresent {
                pty_id: self.claim.pty_id().into(),
            }),
        }
    }
}

impl SubmitTransport for ModalInput {
    fn dialog_guard(&self) -> Result<(), InjectionError> {
        self.guard()
    }
    fn prepare_paste(&self, _: &str, _: bool) {
        if self.open_during_preparation.replace(false) {
            self.modal.set(true);
        }
    }
    fn observe(&self) -> Option<Observation> {
        None
    }
    fn draft_evidence_count(&self, _: &str) -> usize {
        self.payloads.borrow().len()
    }
    fn generation(&self) -> u64 {
        0
    }
    fn wait_change(&self, _: u64, _: Instant) -> bool {
        false
    }
    fn turn_started(&self) -> bool {
        self.status.is_some_and(TabStatus::has_active_turn)
    }
    fn exact_user_turn_present(&self, text: &str) -> bool {
        self.keys.get() > 0 && self.payloads.borrow().iter().any(|payload| payload == text)
    }
    fn retry_active(&self) -> bool {
        self.claim.is_current()
    }
    fn write_paste(&self, text: &str, _: Instant) -> Result<usize, InjectionError> {
        self.guard()?;
        self.payloads.borrow_mut().push(text.into());
        let (sender, _) = mpsc::sync_channel(1);
        self.claim.payload_enqueued(sender);
        Ok(text.len())
    }
    fn write_submit_key(&self, key: &[u8], _: Instant) -> Result<(), InjectionError> {
        self.guard()?;
        assert_eq!(key, b"\r");
        self.keys.set(self.keys.get() + 1);
        Ok(())
    }
}

fn send(input: &ModalInput, text: &str) -> Result<InjectionResult, InjectionError> {
    run(
        input,
        text,
        true,
        VerifySubmitStrategy::TuiInputDrained {
            initial_key_delay: Duration::ZERO,
            retry_offsets: [Duration::ZERO; 6],
        },
        VerifyOpts {
            require_prompt_first: false,
            verify_echo: false,
            verify_submit: true,
            ..Default::default()
        },
    )
}

fn input(status: Option<TabStatus>, pty: &str, modal: bool) -> ModalInput {
    ModalInput {
        status,
        modal: Cell::new(modal),
        open_during_preparation: Cell::new(false),
        claim: claim_submit_cycle(pty),
        payloads: RefCell::default(),
        keys: Cell::new(0),
    }
}

fn input_with_modal(status: Option<TabStatus>, pty: &str) -> ModalInput {
    input(status, pty, true)
}

#[test]
fn busy_unknown_and_idle_stuck_without_modal_write_immediately_with_enter() {
    for (status, pty) in [
        (Some(TabStatus::Working), "modal-free-busy"),
        (None, "modal-free-unknown"),
        (Some(TabStatus::Idle), "modal-free-idle-stuck"),
    ] {
        let input = input(status, pty, false);
        let result = send(&input, "opaque-owner-message").unwrap();
        assert_eq!(*input.payloads.borrow(), ["opaque-owner-message"]);
        assert_eq!(result.submit_key_writes, 1);
        assert!(result.submit_observed);
        let held = input_with_modal(status, pty);
        assert!(matches!(
            send(&held, "queued"),
            Err(InjectionError::AgentTuiPresent { .. })
        ));
        assert!(held.payloads.borrow().is_empty());
        assert_eq!(held.keys.get(), 0);
    }
}

#[test]
fn modal_holds_user_messages_without_reset_then_close_writes_in_order_once_with_enter() {
    let ledger = ChatSendLedger::default();
    let first = ledger
        .try_start_scoped(
            "tab",
            "first",
            None,
            Some(SendLiveBinding {
                session_id: Some("own".into()),
                binding_nonce: Some("launch".into()),
            }),
        )
        .unwrap();
    let second = ledger
        .try_start_scoped(
            "tab",
            "second",
            None,
            Some(SendLiveBinding {
                session_id: Some("own".into()),
                binding_nonce: Some("launch".into()),
            }),
        )
        .unwrap();
    let first_input = input(Some(TabStatus::Working), "modal-held-owner", true);
    let second_input = input(None, "modal-held-owner", true);
    let prior = claim_submit_cycle("modal-held-owner");
    let (sender, _) = mpsc::sync_channel(1);
    prior.payload_enqueued(sender);
    let events = DeliveryReadinessEvents::default();
    let state = |modal_visible| DeliveryReadiness {
        pty_id: "modal-held-owner".into(),
        generation: Some(1),
        modal_visible,
    };
    events.prime("tab", state(true));
    for (pending, input) in [(&first, &first_input), (&second, &second_input)] {
        assert!(matches!(
            send(
                input,
                if pending.send_id == first.send_id {
                    "first"
                } else {
                    "second"
                }
            ),
            Err(InjectionError::AgentTuiPresent { .. })
        ));
        ledger.set_user_pending_reason(&pending.send_id, Some(DELIVERY_MODAL_OPEN));
        let queued = ledger.status(&pending.send_id).unwrap();
        assert_eq!(queued.state, ChatSendState::Pending);
        assert_eq!(queued.pending_reason.as_deref(), Some(DELIVERY_MODAL_OPEN));
        assert!(input.payloads.borrow().is_empty());
        assert_eq!(input.keys.get(), 0);
        assert!(prior.is_current());
        assert!(!events.observe("tab", state(true)));
    }
    assert!(events.observe("tab", state(false)));
    for (pending, input, text) in [
        (&first, &first_input, "first"),
        (&second, &second_input, "second"),
    ] {
        input.modal.set(false);
        ledger.set_user_pending_reason(&pending.send_id, None);
        let result = send(input, text).unwrap();
        assert_eq!(input.payloads.borrow().as_slice(), [text]);
        assert_eq!(result.submit_key_writes, 1);
        ledger.finish(&pending.send_id, Ok(()));
        let delivered =
            ledger.confirm_delivered_user_turns_scoped("tab", "own", Some("launch"), text);
        assert_eq!(delivered.len(), 1);
        assert_eq!(delivered[0].state, ChatSendState::Sent);
    }
    assert!(!events.observe("tab", state(false)));
}

#[test]
fn modal_opening_after_initial_guard_is_refused_at_final_payload_write() {
    let input = input(Some(TabStatus::Working), "modal-open-before-enqueue", false);
    input.open_during_preparation.set(true);
    assert!(matches!(
        send(&input, "opaque"),
        Err(InjectionError::AgentTuiPresent { .. })
    ));
    assert!(input.payloads.borrow().is_empty());
    assert_eq!(input.keys.get(), 0);
    assert!(!input.claim.cycle.is_current(input.claim.generation));
}
