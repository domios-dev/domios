use super::*;
use crate::service::pty_io::submit_transaction::cycle::SubmitCycleClaim;
use crate::service::pty_io::test_support::join;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct State {
    busy: bool,
    composer_ready: bool,
    dialog: bool,
    swallowed_keys: usize,
    writes: Vec<String>,
    generation: u64,
    turn_generation: u64,
    turn_started: bool,
    exact_turn_seen: bool,
    deferred_ready_seen: bool,
}

struct EventTransport {
    state: Mutex<State>,
    event_tx: Sender<()>,
    event_rx: Mutex<Receiver<()>>,
    paste_tx: Sender<()>,
    deferred_wait_tx: Sender<()>,
    event_wait_tx: Sender<()>,
}

impl EventTransport {
    fn new(
        busy: bool,
        swallowed_keys: usize,
    ) -> (Arc<Self>, Receiver<()>, Receiver<()>, Receiver<()>) {
        let (event_tx, event_rx) = mpsc::channel();
        let (paste_tx, paste_rx) = mpsc::channel();
        let (deferred_wait_tx, deferred_wait_rx) = mpsc::channel();
        let (event_wait_tx, event_wait_rx) = mpsc::channel();
        (
            Arc::new(Self {
                state: Mutex::new(State {
                    busy,
                    composer_ready: false,
                    dialog: false,
                    swallowed_keys,
                    writes: Vec::new(),
                    generation: 0,
                    turn_generation: 0,
                    turn_started: busy,
                    exact_turn_seen: false,
                    deferred_ready_seen: false,
                }),
                event_tx,
                event_rx: Mutex::new(event_rx),
                paste_tx,
                deferred_wait_tx,
                event_wait_tx,
            }),
            paste_rx,
            deferred_wait_rx,
            event_wait_rx,
        )
    }

    fn signal(&self) {
        self.event_tx.send(()).unwrap();
    }
}

impl SubmitTransport for EventTransport {
    fn dialog_guard(&self) -> Result<(), InjectionError> {
        if self.state.lock().unwrap().dialog {
            Err(InjectionError::AgentTuiPresent {
                pty_id: "test-pty".into(),
            })
        } else {
            Ok(())
        }
    }

    fn observe(&self) -> Option<Observation> {
        None
    }

    fn draft_evidence_count(&self, _text: &str) -> usize {
        0
    }

    fn generation(&self) -> u64 {
        self.state.lock().unwrap().generation
    }

    fn wait_change(&self, _generation: u64, _deadline: Instant) -> bool {
        false
    }

    fn turn_started(&self) -> bool {
        self.state.lock().unwrap().turn_started
    }

    fn has_exact_user_turn_evidence(&self) -> bool {
        true
    }

    fn exact_user_turn_present(&self, _text: &str) -> bool {
        self.state.lock().unwrap().exact_turn_seen
    }

    fn wait_for_submit_event(&self, deadline: Instant) -> bool {
        let _ = self.event_wait_tx.send(());
        let remaining = deadline.saturating_duration_since(Instant::now());
        self.event_rx
            .lock()
            .unwrap()
            .recv_timeout(remaining)
            .is_ok()
    }

    fn wait_for_deferred_submit_ready(&self, deadline: Instant) -> DeferredSubmitWait {
        if self.state.lock().unwrap().deferred_ready_seen {
            return DeferredSubmitWait::Ready;
        }
        let _ = self.deferred_wait_tx.send(());
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if self
                .event_rx
                .lock()
                .unwrap()
                .recv_timeout(remaining)
                .is_err()
            {
                return DeferredSubmitWait::Deadline;
            }
            let mut state = self.state.lock().unwrap();
            if state.composer_ready {
                state.deferred_ready_seen = true;
                return DeferredSubmitWait::Ready;
            }
        }
    }

    fn write_paste(&self, text: &str, _deadline: Instant) -> Result<usize, InjectionError> {
        self.state
            .lock()
            .unwrap()
            .writes
            .push(format!("paste:{text}"));
        let _ = self.paste_tx.send(());
        Ok(text.len())
    }

    fn write_typed_follow_up(&self, text: &str, _deadline: Instant) -> Result<(), InjectionError> {
        self.state
            .lock()
            .unwrap()
            .writes
            .push(format!("follow_up:{text}"));
        Ok(())
    }

    fn write_submit_key(&self, _key: &[u8], _deadline: Instant) -> Result<(), InjectionError> {
        let mut state = self.state.lock().unwrap();
        state.writes.push("enter".into());
        state.generation += 1;
        if state.swallowed_keys > 0 {
            state.swallowed_keys -= 1;
            return Ok(());
        }
        state.busy = true;
        state.turn_started = true;
        state.turn_generation += 1;
        state.exact_turn_seen = true;
        Ok(())
    }
}

fn opts() -> VerifyOpts {
    VerifyOpts {
        echo_timeout: Duration::ZERO,
        submit_timeout: Duration::from_millis(100),
        verify_submit: true,
        ..super::tests::opts()
    }
}

fn tui_strategy() -> VerifySubmitStrategy {
    VerifySubmitStrategy::TuiInputDrained {
        initial_key_delay: Duration::ZERO,
        retry_offsets: [Duration::ZERO; 6],
    }
}

fn run_with_readiness_guard(
    transport: &EventTransport,
    text: &str,
) -> Result<InjectionResult, InjectionError> {
    run_with_readiness_guard_and_follow_up(transport, text, None)
}

fn run_with_readiness_guard_and_follow_up(
    transport: &EventTransport,
    text: &str,
    typed_follow_up: Option<&str>,
) -> Result<InjectionResult, InjectionError> {
    run_with_submit_key_and_follow_up(
        transport,
        text,
        true,
        b"\r",
        typed_follow_up,
        tui_strategy(),
        opts(),
        |initial_key| {
            let state = transport.state.lock().unwrap();
            if state.dialog {
                return Err(InjectionError::AgentTuiPresent {
                    pty_id: "test-pty".into(),
                });
            }
            if state.busy && !state.composer_ready {
                return Err(InjectionError::AgentInputUnavailable {
                    pty_id: "test-pty".into(),
                });
            }
            let _ = initial_key;
            Ok(())
        },
    )
}

#[test]
fn paste_during_active_turn_defers_the_first_key_until_turn_end() {
    let (transport, paste_rx, _, event_wait_rx) = EventTransport::new(true, 0);
    let send_transport = Arc::clone(&transport);
    let send = std::thread::spawn(move || {
        run_with_readiness_guard_and_follow_up(&send_transport, "first", Some(" follow-up"))
    });

    paste_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("the payload reaches the composer before readiness");
    event_wait_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("the first key guard waits for turn end");
    assert_eq!(transport.state.lock().unwrap().writes, ["paste:first"]);
    {
        let mut state = transport.state.lock().unwrap();
        state.busy = false;
        state.turn_started = false;
    }
    transport.signal();

    let result = join(send, "deferred first key send").unwrap().unwrap();
    assert!(result.submit_observed);
    assert_eq!(
        transport.state.lock().unwrap().writes,
        ["paste:first", "follow_up: follow-up", "enter"]
    );
}

#[test]
fn active_turn_wait_expires_without_an_idle_event() {
    let (transport, paste_rx, _, event_wait_rx) = EventTransport::new(true, 0);
    let send_transport = Arc::clone(&transport);
    let send = std::thread::spawn(move || run_with_readiness_guard(&send_transport, "busy"));

    paste_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("the payload reaches the composer once");
    event_wait_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("busy readiness waits on an event with a failure deadline");

    assert!(matches!(
        join(send, "active turn wait").unwrap(),
        Err(InjectionError::SubmitNeverConfirmed { .. })
    ));
    assert_eq!(transport.state.lock().unwrap().writes, ["paste:busy"]);
}

#[test]
fn composer_ready_event_issues_one_key_after_the_bounded_schedule() {
    let (transport, paste_rx, deferred_wait_rx, _) =
        EventTransport::new(false, MAX_SUBMIT_KEY_WRITES);
    let send_transport = Arc::clone(&transport);
    let send = std::thread::spawn(move || {
        run_with_readiness_guard_and_follow_up(&send_transport, "slow-paste", Some(" follow-up"))
    });

    paste_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("the payload reaches the composer once");
    deferred_wait_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("the bounded keys finish before the readiness event");
    assert_eq!(
        transport
            .state
            .lock()
            .unwrap()
            .writes
            .iter()
            .filter(|write| write.as_str() == "enter")
            .count(),
        MAX_SUBMIT_KEY_WRITES
    );
    transport.state.lock().unwrap().composer_ready = true;
    transport.signal();

    let result = join(send, "composer ready send").unwrap().unwrap();
    assert_eq!(result.submit_key_writes, MAX_SUBMIT_KEY_WRITES + 1);
    assert!(result.submit_observed);
    let state = transport.state.lock().unwrap();
    assert_eq!(state.turn_generation, 1);
    assert_eq!(
        state
            .writes
            .iter()
            .filter(|write| write.starts_with("paste:"))
            .count(),
        1
    );
    let follow_up = state
        .writes
        .iter()
        .position(|write| write == "follow_up: follow-up")
        .expect("follow-up precedes the first submit key");
    let first_key = state
        .writes
        .iter()
        .position(|write| write == "enter")
        .expect("the submit schedule writes a key");
    assert_eq!(
        state
            .writes
            .iter()
            .filter(|write| write.starts_with("follow_up:"))
            .count(),
        1,
        "recovery and the deferred key do not repeat the typed line"
    );
    assert!(
        state
            .writes
            .iter()
            .position(|write| write == "paste:slow-paste")
            .unwrap()
            < follow_up
    );
    assert!(follow_up < first_key);
}

#[test]
fn deferred_key_wait_expires_without_a_readiness_event() {
    let (transport, paste_rx, deferred_wait_rx, _) =
        EventTransport::new(false, MAX_SUBMIT_KEY_WRITES);
    let send_transport = Arc::clone(&transport);
    let send =
        std::thread::spawn(move || run_with_readiness_guard(&send_transport, "readiness-timeout"));

    paste_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("the payload reaches the composer once");
    deferred_wait_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("the deferred event wait starts after bounded keys finish");

    let result = join(send, "readiness timeout send").unwrap().unwrap();
    assert!(!result.submit_observed);
    assert!(result.submit_anomaly);
    assert_eq!(
        transport
            .state
            .lock()
            .unwrap()
            .writes
            .iter()
            .filter(|write| write.as_str() == "enter")
            .count(),
        MAX_SUBMIT_KEY_WRITES
    );
}

fn start_claimed_send(
    claim: SubmitCycleClaim,
    transport: Arc<EventTransport>,
    text: &'static str,
    started_tx: Option<Sender<()>>,
) -> std::thread::JoinHandle<Result<InjectionResult, InjectionError>> {
    std::thread::spawn(move || {
        if let Some(started_tx) = started_tx {
            let _ = started_tx.send(());
        }
        claim.wait_until_active(Instant::now() + Duration::from_secs(2));
        let result = run_with_readiness_guard(&transport, text);
        drop(claim);
        result
    })
}

#[test]
fn a_second_message_waits_for_the_open_cycle_and_keeps_its_own_turn() {
    let pty_id = "two-messages-one-pty-cycle";
    let first_claim = claim_submit_cycle(pty_id);
    let second_claim = claim_submit_cycle(pty_id);
    assert!(first_claim.is_current());
    assert!(!second_claim.is_current());

    let (first, first_paste_rx, first_deferred_rx, _) =
        EventTransport::new(false, MAX_SUBMIT_KEY_WRITES);
    let (second, second_paste_rx, _, _) = EventTransport::new(false, 0);
    let first_send = start_claimed_send(first_claim, Arc::clone(&first), "first", None);
    first_paste_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("first payload is written once");

    let (second_started_tx, second_started_rx) = mpsc::channel();
    let second_send = start_claimed_send(
        second_claim,
        Arc::clone(&second),
        "second",
        Some(second_started_tx),
    );
    second_started_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("the second send is waiting for the active PTY cycle");
    assert!(second_paste_rx.try_recv().is_err());
    first_deferred_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("the first send exhausts its bounded schedule");
    first.state.lock().unwrap().composer_ready = true;
    first.signal();

    assert!(
        join(first_send, "first active cycle send")
            .unwrap()
            .unwrap()
            .submit_observed
    );
    {
        let state = first.state.lock().unwrap();
        assert_eq!(state.turn_generation, 1);
        assert_eq!(
            state
                .writes
                .iter()
                .filter(|write| write.starts_with("paste:"))
                .count(),
            1
        );
    }
    second_paste_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("second payload starts only after the first cycle finishes");
    assert!(
        join(second_send, "second active cycle send")
            .unwrap()
            .unwrap()
            .submit_observed
    );
    assert_eq!(second.state.lock().unwrap().turn_generation, 1);
    assert_eq!(
        second.state.lock().unwrap().writes,
        ["paste:second", "enter"]
    );
}

#[test]
fn deferred_key_is_not_written_when_the_readiness_event_is_a_dialog() {
    let (transport, paste_rx, deferred_wait_rx, _) =
        EventTransport::new(false, MAX_SUBMIT_KEY_WRITES);
    let send_transport = Arc::clone(&transport);
    let send = std::thread::spawn(move || run_with_readiness_guard(&send_transport, "payload"));
    paste_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("paste reaches the composer once");
    deferred_wait_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("the bounded key schedule is exhausted");
    {
        let mut state = transport.state.lock().unwrap();
        state.dialog = true;
        state.composer_ready = true;
    }
    transport.signal();
    let result = join(send, "dialog readiness send").unwrap().unwrap();
    assert!(!result.submit_observed);
    assert!(result.submit_anomaly);
    assert_eq!(
        transport
            .state
            .lock()
            .unwrap()
            .writes
            .iter()
            .filter(|write| write.as_str() == "enter")
            .count(),
        MAX_SUBMIT_KEY_WRITES
    );
}

#[test]
fn provider_dialog_before_paste_is_refused_without_writing() {
    let (transport, paste_rx, _, event_wait_rx) = EventTransport::new(false, 0);
    transport.state.lock().unwrap().dialog = true;
    let error = run_with_readiness_guard(&transport, "payload").unwrap_err();
    assert!(matches!(error, InjectionError::AgentTuiPresent { .. }));
    assert!(paste_rx.try_recv().is_err());
    assert!(transport.state.lock().unwrap().writes.is_empty());
    assert!(event_wait_rx.try_recv().is_err());
}
