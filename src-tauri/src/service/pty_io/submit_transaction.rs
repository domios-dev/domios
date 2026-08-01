use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, Weak};
use std::thread::Thread;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::agent::provider::VerifySubmitStrategy;
use crate::agent::status::TabStatus;
use crate::pty::screen_state::ScreenChangeSignal;
use crate::service::inject_verify::{
    screen_echo_evidence_count, InjectionError, InjectionResult, VerifyOpts,
};
use crate::service::ServiceContext;

const TAIL_ROWS: usize = 8;
const MAX_SUBMIT_KEY_WRITES: usize = 3;

struct SubmitCycle {
    pty_id: String,
    generation: AtomicU64,
    waiter: Mutex<Option<Thread>>,
    write_gate: Mutex<()>,
    #[cfg(test)]
    boundary_hook: Mutex<Option<(u64, Box<dyn FnOnce() + Send>)>>,
}

impl SubmitCycle {
    fn new(pty_id: String) -> Self {
        Self {
            pty_id,
            generation: AtomicU64::new(0),
            waiter: Mutex::new(None),
            write_gate: Mutex::new(()),
            #[cfg(test)]
            boundary_hook: Mutex::new(None),
        }
    }

    fn reset(&self) -> u64 {
        let _write_gate = self.write_gate.lock();
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        if let Some(waiter) = self.waiter.lock().take() {
            waiter.unpark();
        }
        generation
    }

    fn is_current(&self, generation: u64) -> bool {
        self.generation.load(Ordering::Acquire) == generation
    }

    fn current_generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    fn while_current<T>(&self, generation: u64, write: impl FnOnce() -> T) -> Option<T> {
        self.while_current_at_boundary(generation, || {}, write)
    }

    fn while_current_at_boundary<T>(
        &self,
        generation: u64,
        at_boundary: impl FnOnce(),
        write: impl FnOnce() -> T,
    ) -> Option<T> {
        at_boundary();
        #[cfg(test)]
        self.run_boundary_hook(generation);
        let _write_gate = self.write_gate.lock();
        self.is_current(generation).then(write)
    }

    #[cfg(test)]
    fn run_boundary_hook(&self, generation: u64) {
        let hook = {
            let mut hook = self.boundary_hook.lock();
            hook.as_ref()
                .is_some_and(|(hook_generation, _)| *hook_generation == generation)
                .then(|| hook.take().map(|(_, hook)| hook))
                .flatten()
        };
        if let Some(hook) = hook {
            hook();
        }
    }

    fn wait_until(&self, generation: u64, deadline: Instant) -> bool {
        if !self.is_current(generation) {
            return false;
        }
        *self.waiter.lock() = Some(std::thread::current());
        while self.is_current(generation) {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                break;
            };
            std::thread::park_timeout(remaining);
        }
        self.waiter.lock().take();
        self.is_current(generation)
    }
}

fn submit_cycles() -> &'static Mutex<HashMap<String, Weak<SubmitCycle>>> {
    static CYCLES: OnceLock<Mutex<HashMap<String, Weak<SubmitCycle>>>> = OnceLock::new();
    CYCLES.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(test)]
fn submit_cycle_registered(pty_id: &str) -> bool {
    submit_cycles().lock().contains_key(pty_id)
}

impl Drop for SubmitCycle {
    fn drop(&mut self) {
        let mut cycles = submit_cycles().lock();
        if cycles
            .get(&self.pty_id)
            .is_some_and(|cycle| std::ptr::eq(cycle.as_ptr(), self))
        {
            cycles.remove(&self.pty_id);
        }
    }
}

#[derive(Clone)]
pub(crate) struct SubmitCycleClaim {
    pty_id: String,
    cycle: Arc<SubmitCycle>,
    generation: u64,
}

impl SubmitCycleClaim {
    pub(crate) fn pty_id(&self) -> &str {
        &self.pty_id
    }

    pub(crate) fn is_current(&self) -> bool {
        self.cycle.is_current(self.generation)
    }

    #[cfg(test)]
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    #[cfg(test)]
    pub(crate) fn same_cycle(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.cycle, &other.cycle)
    }

    pub(crate) fn enqueue_if_current<T>(&self, enqueue: impl FnOnce() -> T) -> Option<T> {
        self.cycle.while_current(self.generation, enqueue)
    }

    #[cfg(test)]
    fn set_boundary_hook(&self, hook: impl FnOnce() + Send + 'static) {
        *self.cycle.boundary_hook.lock() = Some((self.generation, Box::new(hook)));
    }
}

pub(crate) fn claim_submit_cycle(pty_id: &str) -> SubmitCycleClaim {
    let cycle = {
        let mut cycles = submit_cycles().lock();
        if let Some(cycle) = cycles.get(pty_id).and_then(Weak::upgrade) {
            cycle
        } else {
            let cycle = Arc::new(SubmitCycle::new(pty_id.to_string()));
            cycles.insert(pty_id.to_string(), Arc::downgrade(&cycle));
            cycle
        }
    };
    let generation = cycle.reset();
    log::info!(
        "[submit-cycle] claim pty={} cycle={:p} generation={}",
        pty_id,
        Arc::as_ptr(&cycle),
        generation
    );
    SubmitCycleClaim {
        pty_id: pty_id.to_string(),
        cycle,
        generation,
    }
}

fn framed_composer_row(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let content = trimmed
        .strip_prefix('│')
        .or_else(|| trimmed.strip_prefix('┃'))?
        .trim_start();
    let content = content.trim_end();
    let content = content
        .strip_suffix('│')
        .or_else(|| content.strip_suffix('┃'))
        .unwrap_or(content);
    Some(content.trim_end())
}

fn composer_marker(line: &str) -> bool {
    let trimmed = framed_composer_row(line).unwrap_or_else(|| line.trim_start());
    matches!(trimmed.chars().next(), Some('›' | '❯' | '→'))
}

pub(crate) fn composer_region_text(lines: &[String], cursor_row: usize) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let end = cursor_row.min(lines.len() - 1);
    let start = (0..=end)
        .rev()
        .find(|row| composer_marker(&lines[*row]))
        .unwrap_or_else(|| end.saturating_sub(TAIL_ROWS - 1));
    lines[start..=end]
        .iter()
        .enumerate()
        .map(|(offset, line)| {
            framed_composer_row(line).unwrap_or_else(|| {
                if offset == 0 {
                    line
                } else {
                    line.trim_start()
                }
            })
        })
        .collect::<Vec<_>>()
        .join("\n")
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
    fn turn_generation(&self) -> u64 {
        0
    }
    /// Wait until the status generation changes. A wake triggers predicate
    /// re-evaluation and is never acknowledgement evidence itself.
    fn wait_turn_change(&self, turn_generation: u64, deadline: Instant) -> bool {
        let _ = (turn_generation, deadline);
        false
    }
    fn wait_retry_deadline(&self, deadline: Instant) {
        let _ = deadline;
    }
    fn retry_active(&self) -> bool {
        true
    }
    fn write_paste(&self, text: &str, deadline: Instant) -> Result<usize, InjectionError>;
    fn write_paste_if_active(
        &self,
        text: &str,
        deadline: Instant,
    ) -> Result<Option<usize>, InjectionError> {
        self.write_paste(text, deadline).map(Some)
    }
    fn write_submit_key(&self, key: &[u8], deadline: Instant) -> Result<(), InjectionError>;
    fn write_submit_key_if_active(
        &self,
        key: &[u8],
        deadline: Instant,
    ) -> Result<bool, InjectionError> {
        self.write_submit_key(key, deadline).map(|()| true)
    }
}

struct PtySubmitTransport<'a> {
    svc: &'a ServiceContext,
    pty_id: &'a str,
    signals: Mutex<Option<PtySubmitSignals>>,
    permit: Option<crate::pty::write_queue::GuardedInputPermit>,
    submit_cycle: Arc<SubmitCycle>,
    submit_generation: u64,
}

struct PtySubmitSignals {
    screen: Arc<ScreenChangeSignal>,
    status: Arc<codeterm_types::pty::TabStatusSignal>,
}

impl PtySubmitTransport<'_> {
    fn signals(&self) -> Option<PtySubmitSignals> {
        self.signals
            .lock()
            .as_ref()
            .map(|signals| PtySubmitSignals {
                screen: Arc::clone(&signals.screen),
                status: Arc::clone(&signals.status),
            })
    }

    fn write_submit_key_current(
        &self,
        key: &[u8],
        deadline: Instant,
    ) -> Result<bool, InjectionError> {
        let composer_generation = { self.svc.screen_state.lock().pty_generation(self.pty_id) };
        let Some(enqueued) = self.submit_cycle.while_current(self.submit_generation, || {
            let acknowledgment = match self.permit.as_ref() {
                Some(permit) => self.svc.pty.lock().enqueue_write_and_flush_guarded(
                    self.pty_id,
                    key,
                    permit.clone(),
                ),
                None => self
                    .svc
                    .pty
                    .lock()
                    .enqueue_write_and_flush(self.pty_id, key),
            }
            .map_err(|error| injection_write_error(self.pty_id, error))?;
            self.svc.feed_input_parser(self.pty_id, "", true);
            Ok::<_, InjectionError>(acknowledgment)
        }) else {
            log::info!(
                "[submit-cycle] key-write-skipped pty={} cycle={:p} generation={} current_generation={}",
                self.pty_id,
                Arc::as_ptr(&self.submit_cycle),
                self.submit_generation,
                self.submit_cycle.current_generation()
            );
            return Ok(false);
        };
        let acknowledgment = enqueued?;
        let session_generation = acknowledgment.generation;
        let timeout = deadline.saturating_duration_since(Instant::now());
        let result = acknowledgment.wait(timeout);
        log::info!(
            "[submit-audit] key-write pty={} cycle={:p} submit_generation={} session_gen={} composer_gen={:?} enqueue_timeout_ms={} zero_timeout={} queue_result={}",
            self.pty_id,
            Arc::as_ptr(&self.submit_cycle),
            self.submit_generation,
            session_generation,
            composer_generation,
            timeout.as_millis(),
            timeout.is_zero(),
            match &result {
                Ok(()) => "ok",
                Err(error) => error.as_str(),
            }
        );
        result.map_err(|error| injection_write_error(self.pty_id, error))?;
        Ok(true)
    }
}

fn xterm_paste(text: &str, bracketed_paste_mode: bool) -> String {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    if bracketed_paste_mode {
        format!("\x1b[200~{normalized}\x1b[201~")
    } else {
        normalized.replace('\n', "\r")
    }
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
        self.signals()
            .map(|signals| signals.screen.generation())
            .unwrap_or(0)
    }

    fn wait_change(&self, generation: u64, deadline: Instant) -> bool {
        self.signals().is_some_and(|signals| {
            deadline
                .checked_duration_since(Instant::now())
                .is_some_and(|remaining| signals.screen.wait_after(generation, remaining))
        })
    }

    fn turn_started(&self) -> bool {
        matches!(
            self.svc
                .agent_tracker
                .lock()
                .get_status_if_tracked(self.pty_id),
            Some(TabStatus::Working | TabStatus::Waiting)
        )
    }

    fn turn_generation(&self) -> u64 {
        self.signals()
            .map(|signals| signals.status.generation())
            .unwrap_or(0)
    }

    fn wait_turn_change(&self, turn_generation: u64, deadline: Instant) -> bool {
        self.signals().is_some_and(|signals| {
            deadline
                .checked_duration_since(Instant::now())
                .is_some_and(|remaining| signals.status.wait_after(turn_generation, remaining))
        })
    }

    fn wait_retry_deadline(&self, deadline: Instant) {
        self.submit_cycle
            .wait_until(self.submit_generation, deadline);
    }

    fn retry_active(&self) -> bool {
        let current_generation = self.submit_cycle.current_generation();
        let active = current_generation == self.submit_generation;
        log::info!(
            "[submit-cycle] retry-check pty={} cycle={:p} generation={} current_generation={} active={}",
            self.pty_id,
            Arc::as_ptr(&self.submit_cycle),
            self.submit_generation,
            current_generation,
            active
        );
        active
    }

    fn write_paste(&self, text: &str, deadline: Instant) -> Result<usize, InjectionError> {
        if text.is_empty() {
            return Ok(0);
        }
        let bracketed_paste_mode = self
            .svc
            .screen_state
            .lock()
            .bracketed_paste_mode(self.pty_id)
            .unwrap_or(false);
        let paste = xterm_paste(text, bracketed_paste_mode);
        log::info!(
            "[submit-audit] paste-encoding pty={} bracketed_paste_mode={} source_bytes={} encoded_bytes={}",
            self.pty_id,
            bracketed_paste_mode,
            text.len(),
            paste.len()
        );
        let Some(enqueued) = self.submit_cycle.while_current(self.submit_generation, || {
            let signals = PtySubmitSignals {
                screen: self
                    .svc
                    .screen_state
                    .lock()
                    .arm_screen_change_signal(self.pty_id),
                status: self
                    .svc
                    .agent_tracker
                    .lock()
                    .arm_status_change_signal(self.pty_id),
            };
            let acknowledgment = match self.permit.as_ref() {
                Some(permit) => self.svc.pty.lock().enqueue_write_and_flush_guarded(
                    self.pty_id,
                    paste.as_bytes(),
                    permit.clone(),
                ),
                None => self
                    .svc
                    .pty
                    .lock()
                    .enqueue_write_and_flush(self.pty_id, paste.as_bytes()),
            }
            .map_err(|error| injection_write_error(self.pty_id, error))?;
            self.svc.feed_input_parser(self.pty_id, text, false);
            Ok::<_, InjectionError>((acknowledgment, signals))
        }) else {
            return Err(InjectionError::AgentInputUnavailable {
                pty_id: self.pty_id.to_string(),
            });
        };
        let (acknowledgment, signals) = enqueued?;
        *self.signals.lock() = Some(signals);
        acknowledgment
            .wait(deadline.saturating_duration_since(Instant::now()))
            .map_err(|error| injection_write_error(self.pty_id, error))?;
        Ok(paste.len())
    }

    fn write_paste_if_active(
        &self,
        text: &str,
        deadline: Instant,
    ) -> Result<Option<usize>, InjectionError> {
        match self.write_paste(text, deadline) {
            Err(InjectionError::AgentInputUnavailable { .. }) => Ok(None),
            result => result.map(Some),
        }
    }

    fn write_submit_key(&self, key: &[u8], deadline: Instant) -> Result<(), InjectionError> {
        self.write_submit_key_current(key, deadline)
            .and_then(|written| {
                written
                    .then_some(())
                    .ok_or_else(|| InjectionError::AgentInputUnavailable {
                        pty_id: self.pty_id.to_string(),
                    })
            })
    }

    fn write_submit_key_if_active(
        &self,
        key: &[u8],
        deadline: Instant,
    ) -> Result<bool, InjectionError> {
        self.write_submit_key_current(key, deadline)
    }
}

fn injection_write_error(pty_id: &str, error: String) -> InjectionError {
    if crate::pty::write_queue::is_stale_agent_input(&error) {
        InjectionError::AgentInputUnavailable {
            pty_id: pty_id.to_string(),
        }
    } else if crate::pty::write_queue::is_write_ack_timeout(&error) {
        // The bytes are queued on the serialized writer and may still land, so
        // this is submit uncertainty — never a zero-write "PTY closed" claim.
        InjectionError::SubmitNeverConfirmed {
            waited: std::time::Duration::ZERO,
            screen_tail: String::new(),
        }
    } else {
        InjectionError::PtyClosed {
            pty_id: pty_id.to_string(),
        }
    }
}

fn draft_left_composer<T: SubmitTransport>(
    transport: &T,
    text: &str,
    baseline_evidence: usize,
    echo_observed: bool,
    armed: u64,
) -> bool {
    echo_observed
        && transport.generation() != armed
        && transport.draft_evidence_count(text) <= baseline_evidence
}

fn submission_acknowledged_now<T: SubmitTransport>(
    transport: &T,
    text: &str,
    baseline_evidence: usize,
    echo_observed: bool,
    turn_was_started: bool,
    armed: u64,
    strategy: &VerifySubmitStrategy,
) -> bool {
    let changed = transport.generation() != armed;
    let fresh_turn = changed && !turn_was_started && transport.turn_started();
    let drained = draft_left_composer(transport, text, baseline_evidence, echo_observed, armed);
    if matches!(strategy, VerifySubmitStrategy::TuiInputDrained) {
        return fresh_turn;
    }
    drained || fresh_turn
}

#[cfg(test)]
fn run<T: SubmitTransport>(
    transport: &T,
    text: &str,
    enter: bool,
    strategy: VerifySubmitStrategy,
    opts: VerifyOpts,
) -> Result<InjectionResult, InjectionError> {
    run_with_submit_key(
        transport,
        "test-transport",
        text,
        enter,
        b"\r",
        strategy,
        opts,
    )
}

fn run_with_submit_key<T: SubmitTransport>(
    transport: &T,
    pty_id: &str,
    text: &str,
    enter: bool,
    submit_key: &[u8],
    strategy: VerifySubmitStrategy,
    opts: VerifyOpts,
) -> Result<InjectionResult, InjectionError> {
    let started = Instant::now();
    let turn_deadline = started + opts.submit_timeout;
    let two_phase = enter && opts.verify_submit;

    let baseline_evidence = transport.draft_evidence_count(text);
    let Some(bytes_written) = transport.write_paste_if_active(text, turn_deadline)? else {
        return Err(InjectionError::AgentInputUnavailable {
            pty_id: pty_id.to_string(),
        });
    };
    let mut paste_observed =
        text.is_empty() || transport.draft_evidence_count(text) > baseline_evidence;

    if !enter {
        return Ok(InjectionResult {
            bytes_written,
            echo_observed: paste_observed,
            submit_observed: false,
            submit_anomaly: false,
            submit_key_writes: 0,
            elapsed: started.elapsed(),
        });
    }

    let turn_was_started = transport.turn_started();
    let mut submit_observed = !two_phase;
    let mut submit_key_writes = 0usize;
    for _ in 0..MAX_SUBMIT_KEY_WRITES {
        if !transport.retry_active() {
            break;
        }
        if submit_key_writes > 0 {
            let retry_deadline = started + Duration::from_secs(submit_key_writes as u64);
            transport.wait_retry_deadline(retry_deadline.min(turn_deadline));
            if !transport.retry_active() || Instant::now() >= turn_deadline {
                break;
            }
        }
        let enter_armed = transport.generation();
        // The paste already reached the PTY: a submit-key write failure can
        // only mean unconfirmed submission, never a zero-write terminal error.
        let submit_key_written = match transport
            .write_submit_key_if_active(submit_key, turn_deadline)
        {
            Ok(written) => written,
            Err(error) => {
                if text.is_empty() {
                    return Err(error);
                }
                log::warn!(
                "[submit-transaction] post-paste submit-key write failed ({error}); classified unconfirmed"
            );
                let screen_tail = transport
                    .observe()
                    .map(|state| state.tail)
                    .unwrap_or_default();
                return Err(InjectionError::SubmitNeverConfirmed {
                    waited: started.elapsed(),
                    screen_tail,
                });
            }
        };
        if !submit_key_written {
            break;
        }
        submit_key_writes += 1;
        if !two_phase {
            break;
        }
        paste_observed |= transport.draft_evidence_count(text) > baseline_evidence;
        submit_observed |= submission_acknowledged_now(
            transport,
            text,
            baseline_evidence,
            paste_observed,
            turn_was_started,
            enter_armed,
            &strategy,
        );
    }

    Ok(InjectionResult {
        bytes_written,
        echo_observed: paste_observed,
        submit_observed,
        submit_anomaly: two_phase && !submit_observed,
        submit_key_writes,
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
    let submit_claim = claim_submit_cycle(pty_id);
    submit_text_with_key_claimed(
        svc,
        pty_id,
        text,
        enter,
        submit_key,
        strategy,
        opts,
        permit,
        submit_claim,
    )
}

pub(crate) fn submit_text_with_key_claimed(
    svc: &ServiceContext,
    pty_id: &str,
    text: &str,
    enter: bool,
    submit_key: &[u8],
    strategy: VerifySubmitStrategy,
    opts: VerifyOpts,
    permit: Option<crate::pty::write_queue::GuardedInputPermit>,
    submit_claim: SubmitCycleClaim,
) -> Result<InjectionResult, InjectionError> {
    if submit_claim.pty_id() != pty_id || !submit_claim.is_current() {
        return Err(InjectionError::AgentInputUnavailable {
            pty_id: pty_id.to_string(),
        });
    }
    let submit_cycle = Arc::clone(&submit_claim.cycle);
    let submit_generation = submit_claim.generation;
    let lock = transaction_lock(pty_id);
    let transaction = lock.lock();
    let transport = PtySubmitTransport {
        svc,
        pty_id,
        signals: Mutex::new(None),
        permit,
        submit_cycle: Arc::clone(&submit_cycle),
        submit_generation,
    };
    let result = run_with_submit_key(&transport, pty_id, text, enter, submit_key, strategy, opts).map(
        |result| {
            if result.submit_anomaly {
                log::warn!(
                    "[submit-recovery] pty={} recovered={} payload_repeated=false key_writes={} elapsed_ms={}",
                    pty_id,
                    result.submit_observed,
                    result.submit_key_writes,
                    result.elapsed.as_millis()
                );
            }
            result
        },
    );
    if let Err(ref error) = result {
        let tab_current_pty = {
            let mux = svc.mux.read();
            mux.tab_id_for_pty(pty_id)
                .and_then(|tab_id| mux.get_tab(&tab_id).and_then(|tab| tab.pty_id.clone()))
        };
        log::warn!(
            "[submit-audit] send-failed pty={} tab_current_pty={:?} target_matches_composer_owner={} draft_evidence_now={} turn_started_now={} error={}",
            pty_id,
            tab_current_pty,
            tab_current_pty.as_deref() == Some(pty_id),
            transport.draft_evidence_count(text),
            transport.turn_started(),
            error
        );
    }
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
mod classification_tests;
#[cfg(test)]
mod collapsed_paste_tests;
#[cfg(test)]
mod cycle_cancellation_tests;
#[cfg(test)]
mod dropped_enter_tests;
#[cfg(test)]
mod submission_matrix;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod turn_signal_tests;
