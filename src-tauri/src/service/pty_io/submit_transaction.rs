use std::collections::HashMap;
use std::sync::{Arc, OnceLock, Weak};
use std::time::Instant;

use parking_lot::Mutex;

use crate::agent::provider::VerifySubmitStrategy;
use crate::agent::status::PaneStatus;
use crate::pty::screen_state::ScreenChangeSignal;
use crate::service::inject_verify::{
    screen_echo_evidence_count, InjectionError, InjectionResult, VerifyOpts,
};
use crate::service::ServiceContext;

const TAIL_ROWS: usize = 8;
const MAX_SUBMIT_KEY_WRITES: usize = 3;

fn composer_marker(line: &str) -> bool {
    let trimmed = line.trim_start();
    let trimmed = trimmed
        .strip_prefix('│')
        .or_else(|| trimmed.strip_prefix('┃'))
        .unwrap_or(trimmed)
        .trim_start();
    matches!(trimmed.chars().next(), Some('›' | '❯' | '→'))
}

fn composer_region_text(lines: &[String], cursor_row: usize) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let end = cursor_row.min(lines.len() - 1);
    let start = (0..=end)
        .rev()
        .find(|row| composer_marker(&lines[*row]))
        .unwrap_or_else(|| end.saturating_sub(TAIL_ROWS - 1));
    lines[start..=end].join("\n")
}

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
    fn draft_evidence_count(&self, text: &str) -> usize;
    fn generation(&self) -> u64;
    fn wait_change(&self, generation: u64, deadline: Instant) -> bool;
    /// The provider's own turn state left Idle — a real turn is running.
    fn turn_started(&self) -> bool;
    fn write_paste(&self, text: &str, deadline: Instant) -> Result<usize, InjectionError>;
    fn write_submit_key(&self, key: &[u8], deadline: Instant) -> Result<(), InjectionError>;
}

struct PtySubmitTransport<'a> {
    svc: &'a ServiceContext,
    pty_id: &'a str,
    signal: Arc<ScreenChangeSignal>,
    permit: Option<crate::pty::write_queue::GuardedInputPermit>,
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

    fn draft_evidence_count(&self, text: &str) -> usize {
        if text.is_empty() {
            return 1;
        }
        let state = self.svc.screen_state.lock();
        let lines = state
            .lines_and_tail(self.pty_id, TAIL_ROWS)
            .map(|(lines, _)| lines)
            .unwrap_or_default();
        let cursor_row = state
            .cursor_input_state(self.pty_id)
            .map(|(_, _, row, _)| row as usize)
            .unwrap_or_else(|| lines.len().saturating_sub(1));
        screen_echo_evidence_count(&composer_region_text(&lines, cursor_row), text)
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
        let session = self
            .svc
            .pty
            .lock()
            .get_session(self.pty_id)
            .ok_or_else(|| InjectionError::PtyClosed {
                pty_id: self.pty_id.to_string(),
            })?;
        let timeout = deadline.saturating_duration_since(Instant::now());
        let result = match self.permit.as_ref() {
            Some(permit) => {
                session.write_and_flush_guarded(paste.as_bytes().to_vec(), permit.clone(), timeout)
            }
            None => self
                .svc
                .pty
                .lock()
                .write_and_flush(self.pty_id, paste.as_bytes(), timeout),
        };
        result.map_err(|error| injection_write_error(self.pty_id, error))?;
        if self.permit.is_some() {
            crate::pty::capture::CaptureRegistry::global().tap_input(
                self.pty_id,
                "pty.write",
                paste.as_bytes(),
            );
        }
        Ok(paste.len())
    }

    fn write_submit_key(&self, key: &[u8], deadline: Instant) -> Result<(), InjectionError> {
        let session = self
            .svc
            .pty
            .lock()
            .get_session(self.pty_id)
            .ok_or_else(|| InjectionError::PtyClosed {
                pty_id: self.pty_id.to_string(),
            })?;
        let timeout = deadline.saturating_duration_since(Instant::now());
        let result = match self.permit.as_ref() {
            Some(permit) => session.write_and_flush_guarded(key.to_vec(), permit.clone(), timeout),
            None => self
                .svc
                .pty
                .lock()
                .write_and_flush(self.pty_id, key, timeout),
        };
        result.map_err(|error| injection_write_error(self.pty_id, error))?;
        if self.permit.is_some() {
            crate::pty::capture::CaptureRegistry::global().tap_input(self.pty_id, "pty.write", key);
        }
        Ok(())
    }
}

fn injection_write_error(pty_id: &str, error: String) -> InjectionError {
    if crate::pty::write_queue::is_stale_agent_input(&error) {
        InjectionError::AgentInputUnavailable {
            pty_id: pty_id.to_string(),
        }
    } else {
        InjectionError::PtyClosed {
            pty_id: pty_id.to_string(),
        }
    }
}

fn wait_submission_acknowledged<T: SubmitTransport>(
    transport: &T,
    text: &str,
    baseline_evidence: usize,
    turn_was_started: bool,
    armed: u64,
    deadline: Instant,
) -> bool {
    let mut generation = armed;
    loop {
        if submission_acknowledged_now(transport, text, baseline_evidence, turn_was_started, armed)
        {
            return true;
        }
        if !transport.wait_change(generation, deadline) {
            return false;
        }
        generation = transport.generation();
    }
}

fn submission_acknowledged_now<T: SubmitTransport>(
    transport: &T,
    text: &str,
    baseline_evidence: usize,
    turn_was_started: bool,
    armed: u64,
) -> bool {
    let changed = transport.generation() != armed;
    let draft_left_composer = changed && transport.draft_evidence_count(text) <= baseline_evidence;
    let fresh_turn = changed && !turn_was_started && transport.turn_started();
    draft_left_composer || fresh_turn
}

#[cfg(test)]
fn run<T: SubmitTransport>(
    transport: &T,
    text: &str,
    enter: bool,
    strategy: VerifySubmitStrategy,
    opts: VerifyOpts,
) -> Result<InjectionResult, InjectionError> {
    run_with_submit_key(transport, text, enter, b"\r", strategy, opts)
}

fn run_with_submit_key<T: SubmitTransport>(
    transport: &T,
    text: &str,
    enter: bool,
    submit_key: &[u8],
    _strategy: VerifySubmitStrategy,
    opts: VerifyOpts,
) -> Result<InjectionResult, InjectionError> {
    let started = Instant::now();
    let turn_deadline = started + opts.echo_timeout.saturating_add(opts.submit_timeout);
    let two_phase = enter && opts.verify_submit;

    let baseline_evidence = transport.draft_evidence_count(text);
    let bytes_written = transport.write_paste(text, turn_deadline)?;

    let echo_observed = transport.draft_evidence_count(text) > baseline_evidence;

    if !enter {
        return Ok(InjectionResult {
            bytes_written,
            echo_observed,
            submit_observed: false,
            submit_anomaly: false,
            elapsed: started.elapsed(),
        });
    }

    let turn_was_started = transport.turn_started();
    let enter_armed = transport.generation();
    for index in 0..MAX_SUBMIT_KEY_WRITES {
        if index > 0 {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        transport.write_submit_key(submit_key, turn_deadline)?;
    }
    let submit_observed = !two_phase
        || wait_submission_acknowledged(
            transport,
            text,
            baseline_evidence,
            turn_was_started,
            enter_armed,
            turn_deadline,
        );

    Ok(InjectionResult {
        bytes_written,
        echo_observed,
        submit_observed,
        submit_anomaly: two_phase && !submit_observed,
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
    submit_text_with_key(svc, pty_id, text, enter, b"\r", strategy, opts, None)
}

pub(crate) fn submit_text_with_key(
    svc: &ServiceContext,
    pty_id: &str,
    text: &str,
    enter: bool,
    submit_key: &[u8],
    strategy: VerifySubmitStrategy,
    opts: VerifyOpts,
    permit: Option<crate::pty::write_queue::GuardedInputPermit>,
) -> Result<InjectionResult, InjectionError> {
    let lock = transaction_lock(pty_id);
    let transaction = lock.lock();
    let signal = svc.screen_state.lock().arm_screen_change_signal(pty_id);
    svc.feed_input_parser(pty_id, text, enter);
    let transport = PtySubmitTransport {
        svc,
        pty_id,
        signal,
        permit,
    };
    let required = enter && opts.verify_submit;
    let result = run_with_submit_key(&transport, text, enter, submit_key, strategy, opts).and_then(
        |result| {
            if result.submit_anomaly {
                log::warn!(
                    "[submit-recovery] pty={} recovered={} payload_repeated=false elapsed_ms={}",
                    pty_id,
                    result.submit_observed,
                    result.elapsed.as_millis()
                );
            }
            let elapsed = result.elapsed;
            let tail = transport.observe().map(|o| o.tail).unwrap_or_default();
            result.require_submission(required, elapsed, tail)
        },
    );
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
    use std::time::Duration;

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
        stale_frames_before_ingest: Cell<usize>,
        deliver_ingest_frame: bool,
        accept_enter: bool,
        swallowed_enters: Cell<usize>,
        newline_enters: Cell<usize>,
        writes: RefCell<Vec<String>>,
    }

    impl TuiModel {
        /// Normal causal path: the paste render event is observable; the CR that
        /// follows it starts a turn.
        fn ingesting() -> Self {
            Self::new(0, true, true)
        }

        fn stale_frame_then_ingesting() -> Self {
            Self::new(1, true, true)
        }

        /// The paste may land without exposing a screen event.
        fn recovers_after_unobserved_ingest() -> Self {
            Self::new(0, false, true)
        }

        /// Anomaly that fails honestly: no render event and the CR is swallowed —
        /// the turn never starts, so success must not be claimed.
        fn never_ingests() -> Self {
            Self::new(0, false, false)
        }

        fn new(
            stale_frames_before_ingest: usize,
            deliver_ingest_frame: bool,
            accept_enter: bool,
        ) -> Self {
            Self {
                generation: Cell::new(0),
                draft_pending: Cell::new(false),
                draft_visible: Cell::new(false),
                turn: Cell::new(false),
                stale_frames_before_ingest: Cell::new(stale_frames_before_ingest),
                deliver_ingest_frame,
                accept_enter,
                swallowed_enters: Cell::new(0),
                newline_enters: Cell::new(0),
                writes: RefCell::new(Vec::new()),
            }
        }

        fn swallows_first_enter() -> Self {
            Self::swallows_enters(1)
        }

        fn swallows_enters(count: usize) -> Self {
            let model = Self::new(0, true, true);
            model.swallowed_enters.set(count);
            model
        }

        fn already_working_and_swallows_first_enter() -> Self {
            let model = Self::swallows_first_enter();
            model.turn.set(true);
            model
        }

        fn first_enter_inserts_newline() -> Self {
            let model = Self::new(0, true, true);
            model.newline_enters.set(1);
            model
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

        fn draft_evidence_count(&self, _text: &str) -> usize {
            usize::from(self.draft_visible.get())
        }

        fn generation(&self) -> u64 {
            self.generation.get()
        }

        fn wait_change(&self, _generation: u64, _deadline: Instant) -> bool {
            if self.stale_frames_before_ingest.get() > 0 {
                self.stale_frames_before_ingest
                    .set(self.stale_frames_before_ingest.get() - 1);
                self.generation.set(self.generation.get() + 1);
                return true;
            }
            // Deliver the ingestion frame exactly once after the paste.
            if self.deliver_ingest_frame && self.draft_pending.get() && !self.draft_visible.get() {
                self.draft_visible.set(true);
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
            if !self.deliver_ingest_frame {
                self.draft_visible.set(true);
            }
            self.writes.borrow_mut().push(format!("paste:{text}"));
            Ok(text.len())
        }

        fn write_submit_key(&self, key: &[u8], _deadline: Instant) -> Result<(), InjectionError> {
            let name = if key == b"\t" { "tab" } else { "enter" };
            self.writes.borrow_mut().push(name.to_string());
            if self.deliver_ingest_frame && self.draft_pending.get() && !self.draft_visible.get() {
                self.draft_visible.set(true);
                self.generation.set(self.generation.get() + 1);
            }
            if self.swallowed_enters.get() > 0 {
                self.swallowed_enters.set(self.swallowed_enters.get() - 1);
                return Ok(());
            }
            if self.newline_enters.get() > 0 {
                self.newline_enters.set(self.newline_enters.get() - 1);
                self.generation.set(self.generation.get() + 1);
                return Ok(());
            }
            let paste_landed = self.draft_visible.get() || !self.deliver_ingest_frame;
            if self.accept_enter && paste_landed {
                self.turn.set(key == b"\r");
                self.draft_visible.set(false);
                self.generation.set(self.generation.get() + 1);
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
            self.turn.get()
        }

        fn write_paste(&self, text: &str, _deadline: Instant) -> Result<usize, InjectionError> {
            self.generation.set(self.generation.get() + 1);
            self.writes.borrow_mut().push(format!("paste:{text}"));
            Ok(4)
        }

        fn write_submit_key(&self, key: &[u8], _deadline: Instant) -> Result<(), InjectionError> {
            let name = if key == b"\t" { "tab" } else { "enter" };
            self.writes
                .borrow_mut()
                .push(format!("{name}:{}", self.tail));
            self.turn.set(true);
            self.generation.set(self.generation.get() + 1);
            Ok(())
        }
    }

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
        VerifySubmitStrategy::TuiInputDrained
    }

    #[test]
    fn composer_region_excludes_matching_history_after_submission() {
        let lines = vec![
            "› routed payload".to_string(),
            "• processed routed payload".to_string(),
            "› Find and fix a bug in @filename".to_string(),
            "gpt-5.6-sol".to_string(),
        ];

        let region = composer_region_text(&lines, 2);

        assert!(!region.contains("routed payload"));
        assert!(region.contains("Find and fix a bug"));
    }

    #[test]
    fn composer_region_keeps_wrapped_draft_through_cursor_row() {
        let lines = vec![
            "• prior output".to_string(),
            "› first draft line".to_string(),
            "  second draft line".to_string(),
            "  third draft line".to_string(),
            "gpt-5.6-sol".to_string(),
        ];

        let region = composer_region_text(&lines, 3);

        assert!(region.contains("first draft line"));
        assert!(region.contains("third draft line"));
        assert!(!region.contains("prior output"));
    }

    #[test]
    fn normal_path_fires_cr_on_the_ingestion_event_with_no_anomaly() {
        // The ingestion frame causally releases Enter; a pipe flush alone is insufficient.
        // This fails against bf1278870, where paste+CR were written back-to-back.
        let io = TuiModel::ingesting();
        let result = run(&io, "unique-marker-42", true, tui(), opts()).unwrap();

        assert!(io.turn_started(), "the CR must start a real provider turn");
        assert!(
            result.submit_observed,
            "submit_observed must reflect the provider turn, not a ConPTY flush"
        );
        assert!(!result.submit_anomaly);
        assert_eq!(
            &*io.writes.borrow(),
            &["paste:unique-marker-42", "enter", "enter", "enter"]
        );
    }

    #[test]
    fn stale_repaint_cannot_release_enter_before_new_fuzzy_echo() {
        let io = TuiModel::stale_frame_then_ingesting();
        let result = run(&io, "pairing intro", true, tui(), opts()).unwrap();

        assert!(result.submit_observed);
        assert!(!result.submit_anomaly);
        assert_eq!(
            &*io.writes.borrow(),
            &["paste:pairing intro", "enter", "enter", "enter"]
        );
    }

    #[test]
    fn visible_paste_without_generation_still_submits() {
        let io = TuiModel::recovers_after_unobserved_ingest();
        let result = run(&io, "unique-marker-42", true, tui(), opts()).unwrap();

        assert!(!result.submit_anomaly);
        assert!(result.submit_observed);
        assert_eq!(
            &*io.writes.borrow(),
            &["paste:unique-marker-42", "enter", "enter", "enter"]
        );
    }

    #[test]
    fn missing_paste_evidence_surfaces_actionable_failure() {
        let io = TuiModel::never_ingests();
        let result = run(&io, "marker", true, tui(), opts()).unwrap();

        assert!(result.submit_anomaly);
        assert!(!result.submit_observed, "no turn => not confirmed");
        assert!(!io.turn_started());
        assert_eq!(
            &*io.writes.borrow(),
            &["paste:marker", "enter", "enter", "enter"],
            "the payload remains singular while the submit key is retried"
        );

        let surfaced = result.require_submission(true, Duration::ZERO, "> marker".into());
        assert!(matches!(
            surfaced,
            Err(InjectionError::SubmitNeverConfirmed { .. })
        ));
    }

    #[test]
    fn paste_accepted_then_enter_swallowed_recovers_without_duplicate_payload() {
        let io = TuiModel::swallows_first_enter();
        let result = run(&io, "team bootstrap", true, tui(), opts()).unwrap();

        assert!(!result.submit_anomaly);
        assert!(result.submit_observed);
        assert!(io.turn_started());
        assert_eq!(
            &*io.writes.borrow(),
            &["paste:team bootstrap", "enter", "enter", "enter"]
        );
        result
            .require_submission(true, Duration::ZERO, "> team bootstrap".into())
            .unwrap();
    }

    #[test]
    fn enter_that_inserts_newline_retries_until_the_turn_starts() {
        let io = TuiModel::first_enter_inserts_newline();
        let result = run(&io, "multiline payload", true, tui(), opts()).unwrap();

        assert!(!result.submit_anomaly);
        assert!(result.submit_observed);
        assert!(io.turn_started());
        assert_eq!(
            &*io.writes.borrow(),
            &["paste:multiline payload", "enter", "enter", "enter"]
        );
    }

    #[test]
    fn repeated_swallowed_enters_use_bounded_submit_key_recovery() {
        let io = TuiModel::swallows_enters(5);
        let result = run(&io, "delivery-marker", true, tui(), opts()).unwrap();

        assert!(result.submit_anomaly);
        assert!(!result.submit_observed);
        assert!(!io.turn_started());
        assert_eq!(
            io.writes
                .borrow()
                .iter()
                .filter(|write| write.starts_with("paste:"))
                .count(),
            1,
            "recovery must never paste the message twice"
        );
        assert_eq!(
            io.writes
                .borrow()
                .iter()
                .filter(|write| write.as_str() == "enter")
                .count(),
            MAX_SUBMIT_KEY_WRITES,
            "a pending transaction retries only the submit key within its fixed budget"
        );
    }

    #[test]
    fn working_provider_recovers_swallowed_enter_when_draft_leaves_composer() {
        let io = TuiModel::already_working_and_swallows_first_enter();
        let result = run(&io, "team bootstrap", true, tui(), opts()).unwrap();

        assert!(!result.submit_anomaly);
        assert!(result.submit_observed);
        assert_eq!(
            &*io.writes.borrow(),
            &["paste:team bootstrap", "enter", "enter", "enter"]
        );
    }

    #[test]
    fn every_pty_provider_strategy_preserves_paste_then_enter_order() {
        let strategies = [
            VerifySubmitStrategy::TuiInputDrained,
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
                    "enter:> provider-neutral prompt",
                    "enter:> provider-neutral prompt",
                    "enter:> provider-neutral prompt"
                ]
            );
        }
    }

    #[test]
    fn revoked_after_paste_writes_no_enter_or_recovery() {
        let io = RevokedAfterPaste {
            generation: Cell::new(0),
            writes: RefCell::new(Vec::new()),
        };
        let error = run(&io, "envelope", true, tui(), opts()).unwrap_err();
        assert!(matches!(
            error,
            InjectionError::AgentInputUnavailable { .. }
        ));
        assert_eq!(&*io.writes.borrow(), &["paste:envelope"]);
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
                "enter:> [Pasted Content 2600 chars]".to_string(),
                "enter:> [Pasted Content 2600 chars]".to_string(),
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
                "enter:> [Pasted Content 128 chars]".to_string(),
                "enter:> [Pasted Content 128 chars]".to_string(),
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
