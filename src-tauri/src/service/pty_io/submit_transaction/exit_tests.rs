use super::*;
use std::sync::mpsc::sync_channel;

struct ExitingProvider {
    claim: SubmitCycleClaim,
    receiver: Receiver<SubmitCycleSignal>,
    writes: RefCell<Vec<String>>,
}

impl SubmitTransport for ExitingProvider {
    fn observe(&self) -> Option<Observation> {
        None
    }

    fn draft_evidence_count(&self, _text: &str) -> usize {
        usize::from(!self.writes.borrow().is_empty())
    }

    fn generation(&self) -> u64 {
        0
    }

    fn wait_change(&self, _generation: u64, _deadline: Instant) -> bool {
        false
    }

    fn turn_started(&self) -> bool {
        false
    }

    fn retry_active(&self) -> bool {
        self.claim.is_current()
    }

    fn wait_retry_deadline_for_turn(&self, deadline: Instant, _text: &str) -> bool {
        assert!(matches!(
            self.claim
                .cycle
                .wait_for_wake_until(self.claim.generation, deadline, &self.receiver),
            CycleWait::Inactive
        ));
        false
    }

    fn write_paste(&self, text: &str, _deadline: Instant) -> Result<usize, InjectionError> {
        self.writes.borrow_mut().push(text.into());
        Ok(text.len())
    }

    fn write_submit_key(&self, _key: &[u8], _deadline: Instant) -> Result<(), InjectionError> {
        self.writes.borrow_mut().push("submit-key".into());
        let pty = self.claim.pty_id().to_owned();
        std::thread::spawn(move || notify_submit_cycle_provider_exit(&pty));
        Ok(())
    }
}

#[test]
fn agent_restart_provider_exit_ends_submit_without_confirmation_timeout() {
    let claim = claim_exit_submit_cycle("restart-provider-exit");
    let (sender, receiver) = sync_channel(1);
    claim.cycle.install_waiter(claim.generation, sender);
    let provider = ExitingProvider {
        claim,
        receiver,
        writes: RefCell::new(Vec::new()),
    };
    let started = Instant::now();
    let result = run(
        &provider,
        "/exit",
        true,
        VerifySubmitStrategy::TuiInputDrained {
            initial_key_delay: Duration::ZERO,
            retry_offsets: [Duration::from_secs(30); 6],
        },
        VerifyOpts {
            require_prompt_first: false,
            verify_echo: false,
            verify_submit: true,
            submit_timeout: Duration::from_secs(30),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(provider.claim.provider_exited());
    assert!(!result.submit_observed);
    assert_eq!(result.bytes_written, 5);
    assert_eq!(result.submit_key_writes, 1);
    assert_eq!(*provider.writes.borrow(), ["/exit", "submit-key"]);
}

#[test]
fn agent_restart_provider_exit_before_wait_registration_is_retained() {
    let claim = claim_exit_submit_cycle("restart-exit-before-wait");
    notify_submit_cycle_provider_exit(claim.pty_id());
    let (sender, receiver) = sync_channel(1);
    claim.cycle.install_waiter(claim.generation, sender);
    assert!(claim.provider_exited());
    assert!(matches!(
        claim.cycle.wait_for_wake_until(
            claim.generation,
            Instant::now() + Duration::from_secs(30),
            &receiver
        ),
        CycleWait::Inactive
    ));
}

#[test]
fn agent_restart_exit_observer_does_not_cancel_a_newer_chat_send() {
    let exit = claim_exit_submit_cycle("restart-exit-superseded");
    let chat = claim_submit_cycle(exit.pty_id());
    notify_submit_cycle_provider_exit(exit.pty_id());
    assert!(!exit.provider_exited());
    assert!(chat.is_current());
    assert!(!chat.provider_exited());
    let foreign = claim_exit_submit_cycle("restart-other-pty");
    notify_submit_cycle_provider_exit(exit.pty_id());
    assert!(foreign.is_current());
    assert!(!foreign.provider_exited());
}
