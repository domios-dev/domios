use super::*;

struct Composer {
    status: Option<TabStatus>,
    claim: SubmitCycleClaim,
    payloads: RefCell<Vec<String>>,
    keys: Cell<usize>,
}

impl SubmitTransport for Composer {
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
        self.payloads.borrow_mut().push(text.into());
        let (sender, _) = mpsc::sync_channel(1);
        self.claim.payload_enqueued(sender);
        Ok(text.len())
    }
    fn write_submit_key(&self, key: &[u8], _: Instant) -> Result<(), InjectionError> {
        assert_eq!(key, b"\r");
        self.keys.set(self.keys.get() + 1);
        Ok(())
    }
}

fn composer(status: Option<TabStatus>, pty: &str) -> Composer {
    Composer {
        status,
        claim: claim_submit_cycle(pty),
        payloads: RefCell::default(),
        keys: Cell::new(0),
    }
}

fn send(input: &Composer) -> Result<InjectionResult, InjectionError> {
    run(
        input,
        "opaque-owner-message",
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

#[test]
fn every_tab_state_gets_the_payload_once_and_enter() {
    for (status, pty) in [
        (Some(TabStatus::Working), "always-busy"),
        (None, "always-unknown"),
        (Some(TabStatus::Idle), "always-idle"),
    ] {
        let input = composer(status, pty);
        let result = send(&input).unwrap();
        assert_eq!(*input.payloads.borrow(), ["opaque-owner-message"]);
        assert_eq!(result.submit_key_writes, 1);
        assert!(result.submit_observed);
    }
}
