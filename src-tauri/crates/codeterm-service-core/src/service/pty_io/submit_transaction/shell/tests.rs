use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::ShellActivity;
use crate::service::test_fixture::ServiceFixture;

struct ShellTrace {
    fixture: ServiceFixture,
    pty: String,
    tab: String,
    claim: SubmitCycleClaim,
    waiter: RefCell<Option<ExecWaiter>>,
    writes: RefCell<Vec<String>>,
    keys: Cell<usize>,
    start_after_key: Option<usize>,
    cancel_after_key: Option<usize>,
    finish_after_start: bool,
    replacement: RefCell<Option<SubmitCycleClaim>>,
}

impl ShellTrace {
    fn new(pty: &str, start_after_key: Option<usize>) -> Self {
        let (mut mux, tab) = MultiplexerState::new();
        mux.link_pty(&tab, pty.into());
        let shell = mux.get_tab_mut(&tab).unwrap();
        shell.role = None;
        shell.role_profile = None;
        shell.shell_integration_nonce = Some("nonce".into());
        Self {
            fixture: ServiceFixture::new(mux),
            pty: pty.into(),
            tab,
            claim: claim_submit_cycle(pty),
            waiter: RefCell::new(None),
            writes: RefCell::new(Vec::new()),
            keys: Cell::new(0),
            start_after_key,
            cancel_after_key: None,
            finish_after_start: true,
            replacement: RefCell::new(None),
        }
    }
    fn start(&self, pty: &str, command: &str, nonce: &str) {
        self.fixture.service.process_pty_output(
            pty,
            format!("\x1b]633;A\x07\x1b]633;E;{command};{nonce}\x07\x1b]633;C\x07").as_bytes(),
        );
    }
}

impl SubmitTransport for ShellTrace {
    fn observe(&self) -> Option<Observation> {
        Some(Observation {
            tail: "PS> opaque".into(),
        })
    }
    fn draft_evidence_count(&self, _: &str) -> usize {
        1
    }
    fn generation(&self) -> u64 {
        self.keys.get() as u64
    }
    fn wait_change(&self, _: u64, _: Instant) -> bool {
        false
    }
    fn turn_started(&self) -> bool {
        true
    }
    fn exact_user_turn_present(&self, _: &str) -> bool {
        self.waiter
            .borrow()
            .as_ref()
            .is_some_and(ExecWaiter::started)
    }
    fn max_submit_keys(&self) -> usize {
        4
    }
    fn retry_active(&self) -> bool {
        self.claim.is_current()
    }
    fn write_paste(&self, text: &str, _: Instant) -> Result<usize, InjectionError> {
        *self.waiter.borrow_mut() = Some(ExecWaiter::for_command(&self.pty, text));
        self.claim.enqueue_test_payload();
        self.writes.borrow_mut().push(text.into());
        Ok(text.len())
    }
    fn write_submit_key(&self, key: &[u8], _: Instant) -> Result<(), InjectionError> {
        assert_eq!(key, b"\r");
        let count = self.keys.get() + 1;
        self.keys.set(count);
        self.writes.borrow_mut().push("enter".into());
        if self.start_after_key == Some(count) {
            self.start(&self.pty, "opaque", "nonce");
            if self.finish_after_start {
                self.fixture
                    .service
                    .process_pty_output(&self.pty, b"\x1b]633;D;0\x07");
            }
        } else {
            self.start(&format!("{}-foreign", self.pty), "opaque", "nonce");
            self.start(&self.pty, "leftover-opaque", "nonce");
        }
        if self.cancel_after_key == Some(count) {
            let next = claim_submit_cycle(&self.pty);
            next.enqueue_test_payload();
            *self.replacement.borrow_mut() = Some(next);
        }
        Ok(())
    }
}

#[test]
fn busy_shell_dropped_enter_recovers_only_keys_and_keeps_fast_start_evidence() {
    let trace = ShellTrace::new("shell-dropped-enter", Some(2));
    trace.start(&trace.pty, "opaque", "nonce");
    let result = run_shell(&trace, "opaque").unwrap();
    assert!(result.submit_observed);
    assert!(!result.submit_anomaly);
    assert_eq!(result.submit_key_writes, 2);
    assert_eq!(*trace.writes.borrow(), ["opaque", "enter", "enter"]);
}

#[test]
fn matching_shell_start_stops_recovery_without_waiting_for_output_or_completion() {
    let mut trace = ShellTrace::new("shell-silent-start", Some(1));
    trace.finish_after_start = false;
    let result = run_shell(&trace, "opaque").unwrap();
    assert!(result.submit_observed);
    assert_eq!(*trace.writes.borrow(), ["opaque", "enter"]);
}

#[test]
fn leftover_input_foreign_and_stale_marks_never_confirm_the_shell_send() {
    let trace = ShellTrace::new("shell-leftover-input", None);
    trace.start(&trace.pty, "opaque", "nonce");
    let result = run_shell(&trace, "opaque").unwrap();
    assert!(!result.submit_observed);
    assert!(result.submit_anomaly);
    assert_eq!(result.submit_key_writes, 4);
    assert_eq!(
        *trace.writes.borrow(),
        ["opaque", "enter", "enter", "enter", "enter"]
    );
    let elapsed = result.elapsed;
    assert!(matches!(
        result.require_submission(true, elapsed, String::new()),
        Err(InjectionError::SubmitNeverConfirmed { .. })
    ));
}

#[test]
fn newer_shell_payload_cancels_prior_recovery_without_rewriting_it() {
    let mut trace = ShellTrace::new("shell-newer-payload", None);
    trace.cancel_after_key = Some(1);
    let result = run_shell(&trace, "opaque").unwrap();
    assert!(!result.submit_observed);
    assert_eq!(*trace.writes.borrow(), ["opaque", "enter"]);
}

#[test]
fn echoed_unsubmitted_line_never_claims_running_until_authenticated_command_start() {
    let trace = ShellTrace::new("shell-unsubmitted-state", None);
    trace
        .fixture
        .service
        .prepare_utility_command(&trace.tab, "opaque");
    trace.fixture.service.process_pty_output(
        &trace.pty,
        b"\x1b]633;A\x07PS> opaque\x1b]633;E;opaque;nonce\x07",
    );
    assert_eq!(
        trace
            .fixture
            .service
            .mux
            .read()
            .get_tab(&trace.tab)
            .unwrap()
            .shell_command_state
            .activity,
        ShellActivity::Unknown
    );
    trace
        .fixture
        .service
        .process_pty_output(&trace.pty, b"\x1b]633;C\x07");
    assert_eq!(
        trace
            .fixture
            .service
            .mux
            .read()
            .get_tab(&trace.tab)
            .unwrap()
            .shell_command_state
            .activity,
        ShellActivity::Running
    );
}

#[test]
fn utility_dispatch_separates_submit_key_from_payload_and_keeps_exact_command() {
    for (encoded, payload, expected) in [
        (
            "\x1b[200~ opaque\x1b[201~",
            "\x1b[200~ opaque\x1b[201~",
            " opaque",
        ),
        (
            "/bin/bash /tmp/opaque.sh\r",
            "/bin/bash /tmp/opaque.sh",
            "/bin/bash /tmp/opaque.sh",
        ),
    ] {
        assert_eq!(
            shell_dispatch(encoded.as_bytes()),
            (payload.as_bytes().to_vec(), expected.into())
        );
    }
}
