use super::readiness::{enter_decision, EnterDecision};
use super::*;
use crate::agent::status::TabStatus;
use crate::service::pty_io::submit_transaction::cycle::{
    CycleWait, SubmitCycleClaim, SubmitCycleSignal,
};
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
    draft_visible: bool,
    paste_marks_working: bool,
    working_after_swallowed_key: bool,
    modal_after_swallowed_key: bool,
    modal_closes_composer_after_swallowed_key: bool,
    pause_after_paste: bool,
}

struct EventTransport {
    state: Mutex<State>,
    event_tx: Sender<()>,
    event_rx: Mutex<Receiver<()>>,
    paste_tx: Sender<()>,
    deferred_wait_tx: Sender<()>,
    event_wait_tx: Sender<()>,
    paste_release_tx: Sender<()>,
    paste_release_rx: Mutex<Receiver<()>>,
    cycle_events: Mutex<Option<(SubmitCycleClaim, Receiver<SubmitCycleSignal>)>>,
    cycle_sender: Mutex<Option<mpsc::SyncSender<SubmitCycleSignal>>>,
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
        let (paste_release_tx, paste_release_rx) = mpsc::channel();
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
                    draft_visible: false,
                    paste_marks_working: false,
                    working_after_swallowed_key: false,
                    modal_after_swallowed_key: false,
                    modal_closes_composer_after_swallowed_key: false,
                    pause_after_paste: false,
                }),
                event_tx,
                event_rx: Mutex::new(event_rx),
                paste_tx,
                deferred_wait_tx,
                event_wait_tx,
                paste_release_tx,
                paste_release_rx: Mutex::new(paste_release_rx),
                cycle_events: Mutex::new(None),
                cycle_sender: Mutex::new(None),
            }),
            paste_rx,
            deferred_wait_rx,
            event_wait_rx,
        )
    }

    fn signal(&self) {
        self.event_tx.send(()).unwrap();
    }

    fn make_paste_mark_working(&self) {
        self.state.lock().unwrap().paste_marks_working = true;
    }

    fn pause_after_paste(&self) {
        self.state.lock().unwrap().pause_after_paste = true;
    }

    fn release_paste(&self) {
        self.paste_release_tx.send(()).unwrap();
    }

    fn connect_submit_cycle(&self, pty_id: &str) {
        let claim = claim_submit_cycle(pty_id);
        self.connect_submit_cycle_claim(claim);
    }

    fn connect_submit_cycle_claim(&self, claim: SubmitCycleClaim) {
        let (sender, receiver) = mpsc::sync_channel(1);
        *self.cycle_sender.lock().unwrap() = Some(sender);
        *self.cycle_events.lock().unwrap() = Some((claim, receiver));
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
        if self.state.lock().unwrap().draft_visible {
            1
        } else {
            0
        }
    }

    fn generation(&self) -> u64 {
        self.state.lock().unwrap().generation
    }

    fn wait_change(&self, _generation: u64, _deadline: Instant) -> bool {
        false
    }

    fn wait_retry_deadline_for_turn(&self, deadline: Instant, text: &str) -> bool {
        let _ = self.event_wait_tx.send(());
        loop {
            if self.exact_user_turn_present(text) {
                return true;
            }
            let cycle_wait = {
                let cycle_events = self.cycle_events.lock().unwrap();
                cycle_events.as_ref().map(|(claim, receiver)| {
                    claim
                        .cycle
                        .wait_for_wake_until(claim.generation, deadline, receiver)
                })
            };
            match cycle_wait {
                Some(CycleWait::Event) => continue,
                Some(CycleWait::Deadline | CycleWait::Inactive) => return false,
                None => {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if self
                        .event_rx
                        .lock()
                        .unwrap()
                        .recv_timeout(remaining)
                        .is_err()
                    {
                        return self.exact_user_turn_present(text);
                    }
                }
            }
        }
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
        let cycle_events = self.cycle_events.lock().unwrap();
        if let Some((claim, receiver)) = cycle_events.as_ref() {
            return matches!(
                claim
                    .cycle
                    .wait_for_wake_until(claim.generation, deadline, receiver),
                CycleWait::Event
            );
        }
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
        let mut state = self.state.lock().unwrap();
        state.writes.push(format!("paste:{text}"));
        state.draft_visible = true;
        let pause_after_paste = state.pause_after_paste;
        if state.paste_marks_working {
            state.busy = true;
            state.turn_started = true;
        }
        drop(state);
        if let Some(sender) = self.cycle_sender.lock().unwrap().take() {
            if let Some((claim, _)) = self.cycle_events.lock().unwrap().as_ref() {
                claim.payload_enqueued(sender);
            }
        }
        let _ = self.paste_tx.send(());
        if pause_after_paste {
            self.paste_release_rx
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(2))
                .expect("test releases the paste after setting idle status");
        }
        Ok(text.len())
    }

    fn write_submit_key(&self, _key: &[u8], _deadline: Instant) -> Result<(), InjectionError> {
        let mut state = self.state.lock().unwrap();
        state.writes.push("enter".into());
        state.generation += 1;
        if state.swallowed_keys > 0 {
            state.swallowed_keys -= 1;
            if state.working_after_swallowed_key {
                state.busy = true;
                state.turn_started = true;
            }
            if state.modal_after_swallowed_key {
                state.dialog = true;
                state.composer_ready = !state.modal_closes_composer_after_swallowed_key;
            }
            return Ok(());
        }
        state.draft_visible = false;
        state.busy = true;
        state.turn_started = true;
        state.turn_generation += 1;
        state.exact_turn_seen = true;
        Ok(())
    }

    fn retry_active(&self) -> bool {
        self.cycle_events
            .lock()
            .unwrap()
            .as_ref()
            .is_none_or(|(claim, _)| claim.is_current())
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
        |_initial_key, _turn_was_started| {
            let state = transport.state.lock().unwrap();
            let allowed = enter_decision(state.dialog) == EnterDecision::Submit;
            if !allowed {
                return Err(InjectionError::AgentTuiPresent {
                    pty_id: "test-pty".into(),
                });
            }
            Ok(())
        },
    )
}

#[test]
fn first_key_readiness_allows_post_ready_working_without_a_modal() {
    assert_eq!(enter_decision(false), EnterDecision::Submit);
}

#[test]
fn first_key_readiness_withholds_enter_for_a_confident_prompt() {
    assert_eq!(enter_decision(true), EnterDecision::WithholdForPrompt);
}

#[test]
fn busy_after_first_readiness_submits_long_payload_and_follow_up_without_an_idle_event() {
    let (transport, _, _, event_wait_rx) = EventTransport::new(true, 0);
    let text = "x".repeat(9439);
    let result =
        run_with_readiness_guard_and_follow_up(&transport, &text, Some(" follow-up")).unwrap();
    assert!(result.submit_observed);
    assert_eq!(result.submit_key_writes, 1);
    let state = transport.state.lock().unwrap();
    assert!(state.busy);
    assert!(!state.composer_ready);
    assert_eq!(
        state.writes,
        [format!("paste:{text} follow-up"), "enter".into()]
    );
    assert!(event_wait_rx.try_recv().is_err());
}

#[test]
fn idle_paste_echo_then_idle_poll_keeps_first_key_immediate() {
    let (transport, paste_rx, _, event_wait_rx) = EventTransport::new(false, 0);
    transport.make_paste_mark_working();
    transport.pause_after_paste();
    let pty_id = "idle-paste-echo-idle-poll";
    transport.connect_submit_cycle(pty_id);
    let send_transport = Arc::clone(&transport);
    let send = std::thread::spawn(move || run_with_readiness_guard(&send_transport, "idle"));

    paste_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("the payload reaches the composer once");
    {
        let mut state = transport.state.lock().unwrap();
        assert!(state.busy, "paste echo marks the tracker Working");
        state.busy = false;
        state.turn_started = false;
    }
    notify_submit_cycle_status_transition(pty_id, TabStatus::Working, TabStatus::Idle);
    transport.release_paste();
    let result = join(send, "idle paste submit").unwrap().unwrap();

    assert!(result.submit_observed);
    assert_eq!(result.submit_key_writes, 1);
    assert_eq!(
        transport.state.lock().unwrap().writes,
        ["paste:idle", "enter"]
    );
    assert!(event_wait_rx.try_recv().is_err());
}

#[test]
fn modal_after_busy_payload_handoff_holds_first_key_without_replaying_payload() {
    for composer_ready in [false, true] {
        let (transport, paste_rx, _, _) = EventTransport::new(true, 0);
        transport.pause_after_paste();
        let send_transport = Arc::clone(&transport);
        let send = std::thread::spawn(move || run_with_readiness_guard(&send_transport, "busy"));
        paste_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        {
            let mut state = transport.state.lock().unwrap();
            state.dialog = true;
            state.composer_ready = composer_ready;
        }
        transport.release_paste();

        let result = join(send, "active turn modal guard").unwrap().unwrap_err();
        assert!(matches!(
            result,
            InjectionError::SubmitWithheldForPrompt { .. }
        ));
        assert_eq!(transport.state.lock().unwrap().writes, ["paste:busy"]);
    }
}

#[test]
fn busy_ready_composer_hands_off_payload_once_before_dialog_stops_recovery_keys() {
    let (transport, paste_rx, _, _) = EventTransport::new(true, 1);
    {
        let mut state = transport.state.lock().unwrap();
        state.composer_ready = true;
        state.modal_after_swallowed_key = true;
        state.modal_closes_composer_after_swallowed_key = true;
    }
    let send_transport = Arc::clone(&transport);
    let send = std::thread::spawn(move || run_with_readiness_guard(&send_transport, "follow-up"));

    paste_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("the ready busy composer receives the payload before key recovery");
    match join(send, "busy ready payload handoff").unwrap() {
        Err(error) => assert!(matches!(error, InjectionError::SubmitNeverConfirmed { .. })),
        Ok(result) => assert!(
            !result.submit_observed && result.submit_anomaly && result.submit_key_writes == 1,
            "a dialog after the first key must leave the send unconfirmed without recovery keys: {result:?}"
        ),
    }
    assert_eq!(
        transport.state.lock().unwrap().writes,
        ["paste:follow-up", "enter"]
    );
}

#[test]
fn swallowed_enter_retries_after_working_status_without_turn_evidence() {
    let (transport, paste_rx, _, _) = EventTransport::new(false, 1);
    transport.state.lock().unwrap().working_after_swallowed_key = true;
    let send_transport = Arc::clone(&transport);
    let send = std::thread::spawn(move || run_with_readiness_guard(&send_transport, "message"));

    paste_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("the payload reaches the composer once");
    let result = join(send, "working status submit retry").unwrap().unwrap();

    assert!(result.submit_observed);
    assert!(!result.submit_anomaly);
    assert_eq!(result.submit_key_writes, 2);
    let state = transport.state.lock().unwrap();
    assert!(state.exact_turn_seen);
    assert!(!state.draft_visible);
    assert_eq!(state.writes, ["paste:message", "enter", "enter"]);
}

#[test]
fn swallowed_enter_retries_past_a_codex_prompt_lookalike_in_the_ready_composer() {
    const CODEX_COMPOSER_FIXTURE: &str =
        include_str!("../../../../../plugins/provider-codex/tests/fixtures/tab-state/recovery-dialog-lookalike.txt");

    let (transport, paste_rx, _, _) = EventTransport::new(false, 1);
    {
        let mut state = transport.state.lock().unwrap();
        state.working_after_swallowed_key = true;
        state.composer_ready = true;
    }
    let send_transport = Arc::clone(&transport);
    let send = std::thread::spawn(move || {
        run_with_readiness_guard(&send_transport, CODEX_COMPOSER_FIXTURE)
    });

    paste_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("the Codex prompt-shaped payload reaches the composer once");
    let result = join(send, "Codex composer modal lookalike retry")
        .unwrap()
        .unwrap();

    assert!(result.submit_observed);
    assert!(!result.submit_anomaly);
    assert_eq!(result.submit_key_writes, 2);
    let state = transport.state.lock().unwrap();
    assert!(state.exact_turn_seen);
    assert_eq!(
        state.writes,
        [
            format!("paste:{CODEX_COMPOSER_FIXTURE}"),
            "enter".into(),
            "enter".into()
        ]
    );
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
    assert!(state
        .writes
        .contains(&"paste:slow-paste follow-up".to_string()));
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
        let _ = claim.is_current();
        let result = run_with_readiness_guard(&transport, text);
        drop(claim);
        result
    })
}

fn start_claimed_send_with_recovery_window(
    claim: SubmitCycleClaim,
    transport: Arc<EventTransport>,
    text: &'static str,
) -> std::thread::JoinHandle<Result<InjectionResult, InjectionError>> {
    std::thread::spawn(move || {
        let mut options = opts();
        options.submit_timeout = Duration::from_secs(2);
        let result = run_with_submit_key_and_follow_up(
            transport.as_ref(),
            text,
            true,
            b"\r",
            None,
            VerifySubmitStrategy::TuiInputDrained {
                initial_key_delay: Duration::ZERO,
                retry_offsets: [Duration::from_secs(1); 6],
            },
            options,
            |_initial_key, _turn_was_started| {
                let state = transport.state.lock().unwrap();
                let allowed = enter_decision(state.dialog) == EnterDecision::Submit;
                if !allowed {
                    return Err(InjectionError::AgentTuiPresent {
                        pty_id: "test-pty".into(),
                    });
                }
                Ok(())
            },
        );
        drop(claim);
        result
    })
}

#[test]
fn a_new_send_cancels_remaining_keys_and_writes_while_the_prior_cycle_is_open() {
    let pty_id = "two-messages-one-pty-cycle";
    let first_claim = claim_submit_cycle(pty_id);
    assert!(first_claim.is_current());

    let (first, first_paste_rx, _, first_retry_wait_rx) =
        EventTransport::new(true, MAX_SUBMIT_KEY_WRITES);
    first.state.lock().unwrap().composer_ready = true;
    first.connect_submit_cycle_claim(first_claim.clone());
    let (second, second_paste_rx, _, _) = EventTransport::new(true, 0);
    second.state.lock().unwrap().composer_ready = true;
    let first_send =
        start_claimed_send_with_recovery_window(first_claim.clone(), Arc::clone(&first), "first");
    first_paste_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("first payload is written once");
    first_retry_wait_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("the first cycle reaches its remaining-key wait");
    let second_claim = claim_submit_cycle(pty_id);
    assert!(first_claim.is_current());
    assert!(second_claim.is_current());
    second.connect_submit_cycle_claim(second_claim.clone());
    let second_send = start_claimed_send(second_claim, Arc::clone(&second), "second", None);
    second_paste_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("the second payload writes without waiting for the first cycle");
    assert!(!first_claim.is_current());
    let first_result = join(first_send, "cancelled prior cycle send")
        .unwrap()
        .unwrap();
    assert!(!first_result.submit_observed);
    {
        let state = first.state.lock().unwrap();
        assert_eq!(state.turn_generation, 0);
        assert_eq!(
            state
                .writes
                .iter()
                .filter(|write| write.starts_with("paste:"))
                .count(),
            1
        );
        assert_eq!(
            state
                .writes
                .iter()
                .filter(|write| write.as_str() == "enter")
                .count(),
            1,
        );
    }
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
fn superseding_a_written_payload_preserves_its_initial_enter() {
    let pty_id = "superseded-written-initial-enter";
    let first_claim = claim_submit_cycle(pty_id);
    let (transport, pasted, _, _) = EventTransport::new(true, 0);
    transport.state.lock().unwrap().composer_ready = true;
    transport.pause_after_paste();
    transport.connect_submit_cycle_claim(first_claim.clone());
    let sender = Arc::clone(&transport);
    let send = std::thread::spawn(move || run_with_readiness_guard(&sender, "first"));
    pasted.recv_timeout(Duration::from_secs(2)).unwrap();
    let newer = claim_submit_cycle(pty_id);
    let (sender, _) = mpsc::sync_channel(1);
    newer.payload_enqueued(sender);
    assert!(!first_claim.is_current());
    transport.release_paste();
    let result = join(send, "superseded initial Enter").unwrap().unwrap();
    assert!(result.submit_observed);
    assert_eq!(
        transport.state.lock().unwrap().writes,
        ["paste:first", "enter"]
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
        state.composer_ready = false;
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
fn provider_dialog_before_paste_still_writes_the_payload_once_and_withholds_enter() {
    let (transport, paste_rx, _, _) = EventTransport::new(false, 0);
    transport.state.lock().unwrap().dialog = true;
    let error = run_with_readiness_guard(&transport, "payload").unwrap_err();
    assert!(matches!(
        error,
        InjectionError::SubmitWithheldForPrompt { .. }
    ));
    assert!(paste_rx.try_recv().is_ok());
    assert_eq!(transport.state.lock().unwrap().writes, ["paste:payload"]);
}
