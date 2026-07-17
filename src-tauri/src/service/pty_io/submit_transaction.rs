use std::collections::HashMap;
use std::sync::{Arc, OnceLock, Weak};
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::agent::provider::VerifySubmitStrategy;
use crate::agent::status::PaneStatus;
use crate::pty::screen_state::ScreenChangeSignal;
use crate::service::inject_verify::{
    screen_contains_echo, InjectionError, InjectionResult, VerifyOpts,
};
use crate::service::ServiceContext;

const TAIL_ROWS: usize = 8;

fn transaction_locks() -> &'static Mutex<HashMap<String, Weak<Mutex<()>>>> {
    static LOCKS: OnceLock<Mutex<HashMap<String, Weak<Mutex<()>>>>> = OnceLock::new();
    LOCKS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn transaction_lock(pty_id: &str) -> Arc<Mutex<()>> {
    let mut locks = transaction_locks().lock();
    if let Some(lock) = locks.get(pty_id).and_then(Weak::upgrade) {
        return lock;
    }
    let lock = Arc::new(Mutex::new(()));
    locks.insert(pty_id.to_string(), Arc::downgrade(&lock));
    lock
}

fn retire_transaction_lock(pty_id: &str, lock: &Arc<Mutex<()>>) {
    let mut locks = transaction_locks().lock();
    let current_is_lock = locks
        .get(pty_id)
        .and_then(Weak::upgrade)
        .is_some_and(|current| Arc::ptr_eq(&current, lock));
    if current_is_lock && Arc::strong_count(lock) == 1 {
        locks.remove(pty_id);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PtySubmitEntry {
    Messenger,
    Report,
    Planner,
    Direct,
}

pub(crate) fn submit_entry<T, E>(
    _entry: PtySubmitEntry,
    text: &str,
    submit: impl FnOnce(&str) -> Result<T, E>,
) -> Result<T, E> {
    submit(text)
}

#[derive(Clone)]
struct Observation {
    tail: String,
}

/// Two-phase submit boundary. The paste and the CR are separate PTY writes; the
/// CR is emitted only after the TUI has *causally* rendered the paste (a new
/// frame satisfying the draft predicate), then the provider turn transition is
/// observed independently. Elapsed time is a failure bound only, never readiness.
trait SubmitTransport {
    fn observe(&self) -> Option<Observation>;
    /// Predicate over the FULL parsed screen: the normalized draft or a
    /// structured paste placeholder is present (paste ingested by the TUI).
    fn draft_reflected(&self, text: &str) -> bool;
    fn generation(&self) -> u64;
    fn wait_change(&self, generation: u64, deadline: Instant) -> bool;
    /// The provider's own turn state left Idle — a real turn is running.
    fn turn_started(&self) -> bool;
    fn write_paste(&self, text: &str, deadline: Instant) -> Result<usize, InjectionError>;
    fn write_enter(&self, deadline: Instant) -> Result<(), InjectionError>;
}

struct PtySubmitTransport<'a> {
    svc: &'a ServiceContext,
    pty_id: &'a str,
    signal: Arc<ScreenChangeSignal>,
}

fn bracketed_paste(text: &str) -> String {
    let leading_space = if cfg!(windows) { " " } else { "" };
    format!("\x1b[200~{leading_space}{text}\x1b[201~")
}

impl SubmitTransport for PtySubmitTransport<'_> {
    fn observe(&self) -> Option<Observation> {
        let state = self.svc.screen_state.lock();
        state
            .tail_text(self.pty_id, TAIL_ROWS)
            .map(|tail| Observation { tail })
    }

    fn draft_reflected(&self, text: &str) -> bool {
        if text.is_empty() {
            return true;
        }
        let screen = self.svc.screen_state.lock().get_screen_text(self.pty_id);
        screen.is_some_and(|screen| screen_contains_echo(&screen, text))
    }

    fn generation(&self) -> u64 {
        self.signal.generation()
    }

    fn wait_change(&self, generation: u64, deadline: Instant) -> bool {
        deadline
            .checked_duration_since(Instant::now())
            .is_some_and(|remaining| self.signal.wait_after(generation, remaining))
    }

    fn turn_started(&self) -> bool {
        matches!(
            self.svc
                .agent_tracker
                .lock()
                .get_status_if_tracked(self.pty_id),
            Some(PaneStatus::Working | PaneStatus::Waiting)
        )
    }

    fn write_paste(&self, text: &str, deadline: Instant) -> Result<usize, InjectionError> {
        if text.is_empty() {
            return Ok(0);
        }
        let paste = bracketed_paste(text);
        self.svc
            .pty
            .lock()
            .write_and_flush(
                self.pty_id,
                paste.as_bytes(),
                deadline.saturating_duration_since(Instant::now()),
            )
            .map_err(|_| InjectionError::PtyClosed {
                pty_id: self.pty_id.to_string(),
            })?;
        Ok(paste.len())
    }

    fn write_enter(&self, deadline: Instant) -> Result<(), InjectionError> {
        self.svc
            .pty
            .lock()
            .write_and_flush(
                self.pty_id,
                b"\r",
                deadline.saturating_duration_since(Instant::now()),
            )
            .map_err(|_| InjectionError::PtyClosed {
                pty_id: self.pty_id.to_string(),
            })
    }
}

/// Block until a post-paste frame (`generation` advanced past `armed`) satisfies
/// the draft predicate. Returns `false` on the deadline WITHOUT writing the CR —
/// a truthful failure, never a timer that fires Enter into a mid-ingestion TUI.
fn wait_paste_ingested<T: SubmitTransport>(
    transport: &T,
    text: &str,
    armed: u64,
    deadline: Instant,
) -> bool {
    if text.is_empty() {
        return true;
    }
    let mut generation = armed;
    loop {
        if transport.generation() != armed && transport.draft_reflected(text) {
            return true;
        }
        if !transport.wait_change(generation, deadline) {
            return false;
        }
        generation = transport.generation();
    }
}

/// Observe the provider turn transition after the CR. Wakes on screen frames,
/// tests the provider's discrete turn state — not a screen substring or a poll.
fn wait_turn_started<T: SubmitTransport>(transport: &T, armed: u64, deadline: Instant) -> bool {
    let mut generation = armed;
    loop {
        if transport.turn_started() {
            return true;
        }
        if !transport.wait_change(generation, deadline) {
            return transport.turn_started();
        }
        generation = transport.generation();
    }
}

fn run<T: SubmitTransport>(
    transport: &T,
    text: &str,
    enter: bool,
    strategy: VerifySubmitStrategy,
    opts: VerifyOpts,
) -> Result<InjectionResult, InjectionError> {
    let started = Instant::now();
    let ingest_deadline = started + opts.echo_timeout;
    let turn_deadline = started + opts.echo_timeout.saturating_add(opts.submit_timeout);
    // Two-phase acknowledgement only where the provider exposes a live TUI turn.
    let two_phase = matches!(strategy, VerifySubmitStrategy::TuiInputDrained { .. });

    let armed = transport.generation();
    let bytes_written = transport.write_paste(text, turn_deadline)?;

    // Primary path: the CR fires the instant the paste render event arrives.
    // Bounded deadline is NOT readiness — it only authorizes ONE recovery Enter,
    // flagged as an anomaly, and success still requires the turn transition.
    let mut submit_anomaly = false;
    if enter
        && two_phase
        && !text.is_empty()
        && !wait_paste_ingested(transport, text, armed, ingest_deadline)
    {
        submit_anomaly = true;
        log::warn!(
            "[submit-anomaly] paste render event not observed within {:?}; issuing one recovery Enter, turn acknowledgement still required",
            opts.echo_timeout
        );
    }

    let echo_observed = transport.draft_reflected(text);

    if !enter {
        return Ok(InjectionResult {
            bytes_written,
            echo_observed,
            submit_observed: false,
            submit_anomaly,
            elapsed: started.elapsed(),
        });
    }

    let enter_armed = transport.generation();
    transport.write_enter(turn_deadline)?;
    let submit_observed = if two_phase {
        wait_turn_started(transport, enter_armed, turn_deadline)
    } else {
        true
    };

    Ok(InjectionResult {
        bytes_written,
        echo_observed,
        submit_observed,
        submit_anomaly,
        elapsed: started.elapsed(),
    })
}

pub(crate) fn submit_text(
    svc: &ServiceContext,
    pty_id: &str,
    text: &str,
    enter: bool,
    strategy: VerifySubmitStrategy,
    opts: VerifyOpts,
) -> Result<InjectionResult, InjectionError> {
    let lock = transaction_lock(pty_id);
    let transaction = lock.lock();
    let signal = svc.screen_state.lock().arm_screen_change_signal(pty_id);
    svc.feed_input_parser(pty_id, text, enter);
    let transport = PtySubmitTransport {
        svc,
        pty_id,
        signal,
    };
    let required = enter && opts.verify_submit;
    let result = run(&transport, text, enter, strategy, opts).and_then(|result| {
        let elapsed = result.elapsed;
        let tail = transport.observe().map(|o| o.tail).unwrap_or_default();
        result.require_submission(required, elapsed, tail)
    });
    drop(transaction);
    retire_transaction_lock(pty_id, &lock);
    result
}

pub(crate) fn write_terminal_reply(
    svc: &ServiceContext,
    pty_id: &str,
    reply: &[u8],
) -> Result<(), String> {
    let lock = transaction_lock(pty_id);
    let transaction = lock.lock();
    let result = svc.pty.lock().write(pty_id, reply);
    drop(transaction);
    retire_transaction_lock(pty_id, &lock);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    /// Deterministic model of a live TUI (codex/opencode/claude) at the PTY seam.
    ///
    /// RED/GREEN contract this encodes about the real race:
    ///   * A CR glued to the paste-end (written before any post-paste frame) is
    ///     consumed by the paste handler — the turn never starts. This is the
    ///     bf1278870 regression: `run()` wrote paste+CR back-to-back and marked
    ///     `submit_observed` on the pipe flush.
    ///   * The CR is accepted ONLY once the paste is ingested (a post-paste frame
    ///     satisfies the draft predicate). Success is causally triggered by that
    ///     frame — the deadline is a truthful failure, it never fires the CR.
    struct TuiModel {
        generation: Cell<u64>,
        draft_pending: Cell<bool>,
        draft_visible: Cell<bool>,
        turn: Cell<bool>,
        deliver_ingest_frame: bool,
        accept_enter: bool,
        writes: RefCell<Vec<String>>,
    }

    impl TuiModel {
        /// Normal causal path: the paste render event is observable; the CR that
        /// follows it starts a turn.
        fn ingesting() -> Self {
            Self::new(true, true)
        }

        /// Anomaly that recovers: no observable render event, but the paste did
        /// land, so the one forced recovery Enter starts a turn.
        fn recovers_after_unobserved_ingest() -> Self {
            Self::new(false, true)
        }

        /// Anomaly that fails honestly: no render event and the CR is swallowed —
        /// the turn never starts, so success must not be claimed.
        fn never_ingests() -> Self {
            Self::new(false, false)
        }

        fn new(deliver_ingest_frame: bool, accept_enter: bool) -> Self {
            Self {
                generation: Cell::new(0),
                draft_pending: Cell::new(false),
                draft_visible: Cell::new(false),
                turn: Cell::new(false),
                deliver_ingest_frame,
                accept_enter,
                writes: RefCell::new(Vec::new()),
            }
        }
    }

    impl SubmitTransport for TuiModel {
        fn observe(&self) -> Option<Observation> {
            let tail = if self.turn.get() {
                "working".to_string()
            } else if self.draft_visible.get() {
                "> [Pasted Content 42 chars]".to_string()
            } else {
                "> ".to_string()
            };
            Some(Observation { tail })
        }

        fn draft_reflected(&self, _text: &str) -> bool {
            self.draft_visible.get()
        }

        fn generation(&self) -> u64 {
            self.generation.get()
        }

        fn wait_change(&self, generation: u64, _deadline: Instant) -> bool {
            // Deliver the ingestion frame exactly once after the paste.
            if self.deliver_ingest_frame && self.draft_pending.get() && !self.draft_visible.get() {
                self.draft_visible.set(true);
                self.generation.set(self.generation.get() + 1);
                return true;
            }
            // A started turn keeps rendering; a swallowed submit produces nothing.
            if self.turn.get() && self.generation.get() == generation {
                self.generation.set(self.generation.get() + 1);
                return true;
            }
            false
        }

        fn turn_started(&self) -> bool {
            self.turn.get()
        }

        fn write_paste(&self, text: &str, _deadline: Instant) -> Result<usize, InjectionError> {
            self.draft_pending.set(true);
            self.writes.borrow_mut().push(format!("paste:{text}"));
            Ok(text.len())
        }

        fn write_enter(&self, _deadline: Instant) -> Result<(), InjectionError> {
            self.writes.borrow_mut().push("enter".to_string());
            // The CR starts a turn only if the paste actually landed. On the normal
            // path that is proven by the observed render frame; on the recovery path
            // the paste landed even though no frame was observed.
            let paste_landed = self.draft_visible.get() || !self.deliver_ingest_frame;
            if self.accept_enter && paste_landed {
                self.turn.set(true);
            }
            Ok(())
        }
    }

    /// Scripted transport for the paste-then-enter ordering contract. Enter is
    /// always accepted (turn starts) so ordering assertions stay focused on the
    /// write sequence, not the ingestion race (covered by `TuiModel`).
    struct ControlledTransport {
        tail: &'static str,
        generation: Cell<u64>,
        turn: Cell<bool>,
        writes: RefCell<Vec<String>>,
    }

    impl ControlledTransport {
        fn new(tail: &'static str) -> Self {
            Self {
                tail,
                generation: Cell::new(0),
                turn: Cell::new(false),
                writes: RefCell::new(Vec::new()),
            }
        }
    }

    impl SubmitTransport for ControlledTransport {
        fn observe(&self) -> Option<Observation> {
            Some(Observation {
                tail: self.tail.to_string(),
            })
        }

        // Ordering model: the paste render frame is delivered synchronously by
        // `write_paste`, so ingestion always resolves and Enter follows the paste.
        fn draft_reflected(&self, _text: &str) -> bool {
            self.generation.get() > 0
        }

        fn generation(&self) -> u64 {
            self.generation.get()
        }

        fn wait_change(&self, _generation: u64, _deadline: Instant) -> bool {
            false
        }

        fn turn_started(&self) -> bool {
            self.turn.get()
        }

        fn write_paste(&self, text: &str, _deadline: Instant) -> Result<usize, InjectionError> {
            self.generation.set(self.generation.get() + 1);
            self.writes.borrow_mut().push(format!("paste:{text}"));
            Ok(4)
        }

        fn write_enter(&self, _deadline: Instant) -> Result<(), InjectionError> {
            self.writes
                .borrow_mut()
                .push(format!("enter:{}", self.tail));
            self.turn.set(true);
            Ok(())
        }
    }

    fn opts() -> VerifyOpts {
        VerifyOpts {
            require_prompt_first: false,
            verify_echo: true,
            verify_submit: true,
            echo_timeout: Duration::ZERO,
            submit_timeout: Duration::ZERO,
            prompt_timeout: Duration::ZERO,
        }
    }

    fn tui() -> VerifySubmitStrategy {
        VerifySubmitStrategy::TuiInputDrained {
            timeout: Duration::ZERO,
            idle_after_drain: Duration::ZERO,
        }
    }

    #[test]
    fn normal_path_fires_cr_on_the_ingestion_event_with_no_anomaly() {
        // GREEN with the two-phase machine: the CR lands only after the ingestion
        // frame, a real turn starts, `submit_observed` reflects it, and no recovery
        // was needed (`submit_anomaly` is false).
        //
        // RED against bf1278870: that `run()` wrote paste+CR back-to-back (no
        // ingestion frame between them) and set `submit_observed = true` on the
        // pipe flush. This same assertion fails there because `turn_started()`
        // stays false — the CR was swallowed. Success is causally triggered by the
        // ingestion frame; the deadline never fires the CR on this path.
        let io = TuiModel::ingesting();
        let result = run(&io, "unique-marker-42", true, tui(), opts()).unwrap();

        assert!(io.turn_started(), "the CR must start a real provider turn");
        assert!(
            result.submit_observed,
            "submit_observed must reflect the provider turn, not a ConPTY flush"
        );
        assert!(
            !result.submit_anomaly,
            "the causal path must not be flagged as a recovery"
        );
        assert_eq!(
            &*io.writes.borrow(),
            &["paste:unique-marker-42", "enter"],
            "the CR must be written strictly after the paste, never glued to it"
        );
    }

    #[test]
    fn bounded_deadline_recovers_with_one_forced_enter_flagged_anomalous() {
        // The render event is never observed. The bounded deadline authorizes ONE
        // forced Enter (not a loop), the submit is flagged anomalous, and success
        // is STILL gated on the provider turn transition — never on elapsed time.
        let io = TuiModel::recovers_after_unobserved_ingest();
        let result = run(&io, "unique-marker-42", true, tui(), opts()).unwrap();

        assert!(
            result.submit_anomaly,
            "the recovery path must be observably anomalous"
        );
        assert!(
            result.submit_observed,
            "recovery still requires the turn transition to claim success"
        );
        assert_eq!(
            &*io.writes.borrow(),
            &["paste:unique-marker-42", "enter"],
            "exactly one recovery Enter — no retry loop"
        );
    }

    #[test]
    fn forced_recovery_without_a_turn_stays_unconfirmed() {
        // Recovery fired one Enter but the turn never started: no silent success.
        // The anomaly is recorded and the submit surfaces as unconfirmed.
        let io = TuiModel::never_ingests();
        let result = run(&io, "marker", true, tui(), opts()).unwrap();

        assert!(result.submit_anomaly);
        assert!(!result.submit_observed, "no turn => not confirmed");
        assert!(!io.turn_started());
        assert_eq!(
            &*io.writes.borrow(),
            &["paste:marker", "enter"],
            "exactly one forced Enter, then honest failure — no retry loop"
        );

        let surfaced = result.require_submission(true, Duration::ZERO, "> marker".into());
        assert!(matches!(
            surfaced,
            Err(InjectionError::SubmitNeverConfirmed { .. })
        ));
    }

    #[test]
    fn every_pty_provider_strategy_preserves_paste_then_enter_order() {
        let strategies = [
            VerifySubmitStrategy::TuiInputDrained {
                timeout: Duration::ZERO,
                idle_after_drain: Duration::ZERO,
            },
            VerifySubmitStrategy::ShellPrompt {
                timeout: Duration::ZERO,
            },
            VerifySubmitStrategy::None,
        ];

        for strategy in strategies {
            let io = ControlledTransport::new("> provider-neutral prompt");
            let result = run(&io, "provider-neutral prompt", true, strategy, opts()).unwrap();
            assert!(result.submit_observed);
            assert_eq!(
                &*io.writes.borrow(),
                &[
                    "paste:provider-neutral prompt",
                    "enter:> provider-neutral prompt"
                ]
            );
        }
    }

    #[test]
    fn long_wrapped_chat_message_enters_after_paste_placeholder_echo() {
        let body = "continuation ".repeat(200);
        let io = ControlledTransport::new("> [Pasted Content 2600 chars]");
        let result = run(&io, &body, true, tui(), opts()).unwrap();

        assert!(result.submit_observed);
        assert!(result.echo_observed);
        assert_eq!(
            &*io.writes.borrow(),
            &[
                format!("paste:{body}"),
                "enter:> [Pasted Content 2600 chars]".to_string()
            ]
        );
    }

    #[test]
    fn complete_echo_is_diagnostic_but_not_required_for_enter() {
        let io = ControlledTransport::new("> hello");
        let result = run(&io, "hello", true, tui(), opts()).unwrap();
        assert!(result.echo_observed);
        assert!(result.submit_observed);
    }

    #[test]
    fn spawn_introduction_uses_the_same_paste_then_enter_contract() {
        let intro = "-=-codeterm:pane label=CodeTerm pane=system-=-You are the Orchestrator of this team. Your Planner counterpart is Rodin.";
        let io = ControlledTransport::new("> [Pasted Content 128 chars]");

        let result = submit_entry(PtySubmitEntry::Messenger, intro, |body| {
            run(&io, body, true, tui(), opts())
        })
        .unwrap();

        assert!(result.submit_observed);
        assert_eq!(
            &*io.writes.borrow(),
            &[
                format!("paste:{intro}"),
                "enter:> [Pasted Content 128 chars]".to_string()
            ]
        );
    }

    #[test]
    fn messenger_report_planner_send_share_submit_transaction() {
        for entry in [
            PtySubmitEntry::Messenger,
            PtySubmitEntry::Report,
            PtySubmitEntry::Planner,
            PtySubmitEntry::Direct,
        ] {
            let mut submit_calls = 0;
            let result = submit_entry(entry, "shared body", |text| {
                submit_calls += 1;
                assert_eq!(text, "shared body");
                Err::<(), _>(InjectionError::SubmitNeverConfirmed {
                    waited: Duration::ZERO,
                    screen_tail: "> shared body".into(),
                })
            });
            assert!(matches!(
                result,
                Err(InjectionError::SubmitNeverConfirmed { .. })
            ));
            assert_eq!(submit_calls, 1);
        }
    }

    #[test]
    fn transaction_lock_registry_retires_closed_ptys() {
        let lock = transaction_lock("closed-pty");
        assert!(transaction_locks().lock().contains_key("closed-pty"));
        retire_transaction_lock("closed-pty", &lock);
        assert!(!transaction_locks().lock().contains_key("closed-pty"));
    }

    #[test]
    fn terminal_replies_wait_for_an_active_submit_transaction() {
        let lock = transaction_lock("reply-lock");
        let submit = lock.lock();
        let (entered, receiver) = std::sync::mpsc::sync_channel(1);
        let reply_lock = lock.clone();
        let worker = std::thread::spawn(move || {
            let _reply = reply_lock.lock();
            let _ = entered.send(());
        });

        assert!(receiver.recv_timeout(Duration::from_millis(1)).is_err());
        drop(submit);
        receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("terminal reply must run after submit completion");
        worker.join().unwrap();
        retire_transaction_lock("reply-lock", &lock);
    }
}
