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
/// Offsets from the first submit key at which to press again, dense early and
/// sparse later, until the composer takes it or a newer send supersedes us.
/// Pressing a key an empty composer ignores costs nothing; leaving a message
/// unsent costs everything.
const SUBMIT_RETRY_OFFSETS_MS: [u64; 6] = [200, 500, 900, 1500, 2200, 3000];
const MAX_SUBMIT_KEY_WRITES: usize = SUBMIT_RETRY_OFFSETS_MS.len() + 1;
const BRACKETED_PASTE_START: &str = "\x1b[200~";
const BRACKETED_PASTE_END: &str = "\x1b[201~";

struct SubmitCycle {
    generation: AtomicU64,
    waiter: Mutex<Option<Thread>>,
}

impl SubmitCycle {
    fn new() -> Self {
        Self {
            generation: AtomicU64::new(0),
            waiter: Mutex::new(None),
        }
    }

    fn reset(&self) -> u64 {
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        if let Some(waiter) = self.waiter.lock().take() {
            waiter.unpark();
        }
        generation
    }

    fn is_current(&self, generation: u64) -> bool {
        self.generation.load(Ordering::Acquire) == generation
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

    #[cfg(test)]
    fn is_current(&self) -> bool {
        self.cycle.is_current(self.generation)
    }
}

pub(crate) fn claim_submit_cycle(pty_id: &str) -> SubmitCycleClaim {
    let cycle = {
        let mut cycles = submit_cycles().lock();
        if let Some(cycle) = cycles.get(pty_id).and_then(Weak::upgrade) {
            cycle
        } else {
            let cycle = Arc::new(SubmitCycle::new());
            cycles.insert(pty_id.to_string(), Arc::downgrade(&cycle));
            cycle
        }
    };
    let generation = cycle.reset();
    log::info!(
        "[submit-cycle] claim pty={} generation={}",
        pty_id,
        generation
    );
    SubmitCycleClaim {
        pty_id: pty_id.to_string(),
        cycle,
        generation,
    }
}

fn retire_submit_cycle(pty_id: &str, cycle: &Arc<SubmitCycle>) {
    let mut cycles = submit_cycles().lock();
    let current_is_cycle = cycles
        .get(pty_id)
        .and_then(Weak::upgrade)
        .is_some_and(|current| Arc::ptr_eq(&current, cycle));
    if current_is_cycle && Arc::strong_count(cycle) == 1 {
        cycles.remove(pty_id);
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

#[allow(dead_code)]
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
    fn write_initial_submit(
        &self,
        _text: &str,
        _key: &[u8],
        _deadline: Instant,
    ) -> Option<Result<usize, InjectionError>> {
        None
    }
    fn write_paste(&self, text: &str, deadline: Instant) -> Result<usize, InjectionError>;
    fn write_submit_key(&self, key: &[u8], deadline: Instant) -> Result<(), InjectionError>;
}

struct PtySubmitTransport<'a> {
    svc: &'a ServiceContext,
    pty_id: &'a str,
    signal: Arc<ScreenChangeSignal>,
    #[allow(dead_code)]
    status_signal: Arc<codeterm_types::pty::TabStatusSignal>,
    permit: Option<crate::pty::write_queue::GuardedInputPermit>,
    submit_cycle: Arc<SubmitCycle>,
    submit_generation: u64,
    paste_capability: PasteEncodingCapability,
    /// Stream-preview turn signal fires once per transaction, at the first
    /// successful submit-key enqueue. Signal only — never gates delivery.
    turn_signaled: std::cell::Cell<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PasteEncodingCapability {
    LiveModeOnly,
    MultilineTui,
}

impl PasteEncodingCapability {
    fn for_strategy(strategy: &VerifySubmitStrategy) -> Self {
        if matches!(strategy, VerifySubmitStrategy::TuiInputDrained { .. }) {
            Self::MultilineTui
        } else {
            Self::LiveModeOnly
        }
    }

    fn bracketed(self, text: &str, live_mode: Option<bool>) -> bool {
        live_mode.unwrap_or_else(|| {
            self == Self::MultilineTui && (text.contains('\r') || text.contains('\n'))
        })
    }
}

fn xterm_paste(text: &str, live_mode: Option<bool>, capability: PasteEncodingCapability) -> String {
    if capability.bracketed(text, live_mode) {
        let normalized = text.replace("\r\n", "\n");
        format!("{BRACKETED_PASTE_START}{normalized}{BRACKETED_PASTE_END}")
    } else {
        text.replace("\r\n", "\r").replace('\n', "\r")
    }
}

fn initial_submit_frames(
    text: &str,
    key: &[u8],
    live_mode: Option<bool>,
    capability: PasteEncodingCapability,
) -> Vec<Vec<u8>> {
    vec![
        xterm_paste(text, live_mode, capability).into_bytes(),
        key.to_vec(),
    ]
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
            Some(TabStatus::Working | TabStatus::Waiting)
        )
    }

    fn turn_generation(&self) -> u64 {
        self.status_signal.generation()
    }

    fn wait_turn_change(&self, turn_generation: u64, deadline: Instant) -> bool {
        deadline
            .checked_duration_since(Instant::now())
            .is_some_and(|remaining| self.status_signal.wait_after(turn_generation, remaining))
    }

    fn wait_retry_deadline(&self, deadline: Instant) {
        self.submit_cycle
            .wait_until(self.submit_generation, deadline);
    }

    fn retry_active(&self) -> bool {
        self.submit_cycle.is_current(self.submit_generation)
    }

    fn write_initial_submit(
        &self,
        text: &str,
        key: &[u8],
        deadline: Instant,
    ) -> Option<Result<usize, InjectionError>> {
        if text.is_empty() {
            return Some(self.write_submit_key(key, deadline).map(|()| 0));
        }
        let live_bracketed_paste_mode = self
            .svc
            .screen_state
            .lock()
            .bracketed_paste_mode(self.pty_id);
        let frames =
            initial_submit_frames(text, key, live_bracketed_paste_mode, self.paste_capability);
        let paste_len = frames[0].len();
        let session = match self.svc.pty.lock().get_session(self.pty_id) {
            Some(session) => session,
            None => {
                return Some(Err(InjectionError::PtyClosed {
                    pty_id: self.pty_id.to_string(),
                }));
            }
        };
        let timeout = deadline.saturating_duration_since(Instant::now());
        log::info!(
            "[submit-audit] initial-submit pty={} live_bracketed_paste_mode={:?} paste_capability={:?} source_bytes={} encoded_bytes={} session_gen={}",
            self.pty_id,
            live_bracketed_paste_mode,
            self.paste_capability,
            text.len(),
            paste_len,
            session.generation
        );
        let result = match self.permit.as_ref() {
            Some(permit) => {
                session.write_frames_and_flush_guarded(frames.clone(), permit.clone(), timeout)
            }
            None => {
                self.svc
                    .pty
                    .lock()
                    .write_frames_and_flush(self.pty_id, frames.clone(), timeout)
            }
        };
        if result.is_ok() && self.permit.is_some() {
            for frame in &frames {
                crate::pty::capture::CaptureRegistry::global().tap_input(
                    self.pty_id,
                    "pty.write",
                    frame,
                );
            }
        }
        Some(
            result
                .map(|()| paste_len)
                .map_err(|error| injection_write_error(self.pty_id, error)),
        )
    }

    fn write_paste(&self, text: &str, deadline: Instant) -> Result<usize, InjectionError> {
        if text.is_empty() {
            return Ok(0);
        }
        let live_bracketed_paste_mode = self
            .svc
            .screen_state
            .lock()
            .bracketed_paste_mode(self.pty_id);
        let paste = xterm_paste(text, live_bracketed_paste_mode, self.paste_capability);
        log::info!(
            "[submit-audit] paste-encoding pty={} live_bracketed_paste_mode={:?} paste_capability={:?} source_bytes={} encoded_bytes={}",
            self.pty_id,
            live_bracketed_paste_mode,
            self.paste_capability,
            text.len(),
            paste.len()
        );
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
        let composer_generation = { self.svc.screen_state.lock().pty_generation(self.pty_id) };
        let session = self
            .svc
            .pty
            .lock()
            .get_session(self.pty_id)
            .ok_or_else(|| {
                log::warn!(
                    "[submit-audit] key-write pty={} session=missing composer_gen={:?}",
                    self.pty_id,
                    composer_generation
                );
                InjectionError::PtyClosed {
                    pty_id: self.pty_id.to_string(),
                }
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
        log::info!(
            "[submit-audit] key-write pty={} session_gen={} composer_gen={:?} enqueue_timeout_ms={} zero_timeout={} queue_result={}",
            self.pty_id,
            session.generation,
            composer_generation,
            timeout.as_millis(),
            timeout.is_zero(),
            match &result {
                Ok(()) => "ok",
                Err(error) => error.as_str(),
            }
        );
        result.map_err(|error| injection_write_error(self.pty_id, error))?;
        if !self.turn_signaled.replace(true) {
            crate::service::stream_preview::begin_turn(self.pty_id);
        }
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
    if matches!(strategy, VerifySubmitStrategy::TuiInputDrained { .. }) {
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
    run_with_submit_key(transport, text, enter, b"\r", strategy, opts, |_| Ok(()))
}

fn run_with_submit_key<T: SubmitTransport>(
    transport: &T,
    text: &str,
    enter: bool,
    submit_key: &[u8],
    strategy: VerifySubmitStrategy,
    opts: VerifyOpts,
    mut key_guard: impl FnMut(bool) -> Result<(), InjectionError>,
) -> Result<InjectionResult, InjectionError> {
    let started = Instant::now();
    let turn_deadline = started + opts.submit_timeout;
    let two_phase = enter && opts.verify_submit;
    let initial_key_delay = if enter && !text.is_empty() {
        strategy.initial_key_delay()
    } else {
        Duration::ZERO
    };

    let baseline_evidence = transport.draft_evidence_count(text);
    let turn_was_started = enter.then(|| transport.turn_started());
    let initial_enter_armed = transport.generation();
    let initial_submit = if enter && initial_key_delay.is_zero() {
        key_guard(true)?;
        transport.write_initial_submit(text, submit_key, turn_deadline)
    } else {
        None
    };
    let (bytes_written, mut submit_key_writes) = match initial_submit {
        Some(result) => (result?, 1),
        None => (transport.write_paste(text, turn_deadline)?, 0),
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

    let turn_was_started = turn_was_started.unwrap_or(false);
    let submit_started = Instant::now();
    let mut submit_observed = !two_phase;
    if submit_key_writes == 1 && !two_phase {
        return Ok(InjectionResult {
            bytes_written,
            echo_observed: paste_observed,
            submit_observed,
            submit_anomaly: false,
            submit_key_writes,
            elapsed: started.elapsed(),
        });
    }
    if submit_key_writes == 1 {
        submit_observed |= submission_acknowledged_now(
            transport,
            text,
            baseline_evidence,
            paste_observed,
            turn_was_started,
            initial_enter_armed,
            &strategy,
        );
    }
    while submit_key_writes < MAX_SUBMIT_KEY_WRITES {
        if submit_key_writes == 0 && !initial_key_delay.is_zero() {
            let first_key_deadline = submit_started + initial_key_delay;
            transport.wait_retry_deadline(first_key_deadline.min(turn_deadline));
            if !transport.retry_active() || Instant::now() >= turn_deadline {
                break;
            }
        } else if submit_key_writes > 0 {
            if !transport.retry_active() {
                break;
            }
            let offset = SUBMIT_RETRY_OFFSETS_MS
                .get(submit_key_writes - 1)
                .copied()
                .unwrap_or(*SUBMIT_RETRY_OFFSETS_MS.last().expect("non-empty"));
            let retry_deadline = submit_started + Duration::from_millis(offset);
            transport.wait_retry_deadline(retry_deadline.min(turn_deadline));
            if !transport.retry_active() || Instant::now() >= turn_deadline {
                break;
            }
        }
        if let Err(error) = key_guard(submit_key_writes == 0) {
            if submit_key_writes > 0 {
                break;
            }
            if bytes_written == 0 {
                return Err(error);
            }
            let screen_tail = transport
                .observe()
                .map(|state| state.tail)
                .unwrap_or_default();
            return Err(InjectionError::SubmitNeverConfirmed {
                waited: started.elapsed(),
                screen_tail,
            });
        }
        let enter_armed = transport.generation();
        // A prior payload or submit key makes this failure submission uncertainty.
        if let Err(error) = transport.write_submit_key(submit_key, turn_deadline) {
            if text.is_empty() && submit_key_writes == 0 {
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
    let claim = claim_submit_cycle(pty_id);
    submit_text_with_key_claimed(
        svc, pty_id, text, enter, submit_key, strategy, opts, permit, claim,
    )
}

/// A submit key beyond the initial write means the bounded recovery burst ran.
/// Reporting on this rather than on `submit_anomaly` keeps `recovered` bivalent:
/// `submit_anomaly` is `two_phase && !submit_observed`, so it can never coexist
/// with a recovered submit.
pub(crate) fn recovery_attempted(result: &InjectionResult) -> bool {
    result.submit_key_writes > 1
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
    claim: SubmitCycleClaim,
) -> Result<InjectionResult, InjectionError> {
    if claim.pty_id() != pty_id {
        return Err(InjectionError::AgentInputUnavailable {
            pty_id: pty_id.to_string(),
        });
    }
    let submit_cycle = Arc::clone(&claim.cycle);
    let submit_generation = claim.generation;
    let paste_capability = PasteEncodingCapability::for_strategy(&strategy);
    let signal = svc.screen_state.lock().arm_screen_change_signal(pty_id);
    let status_signal = svc.agent_tracker.lock().arm_status_change_signal(pty_id);
    svc.feed_input_parser(pty_id, text, enter);
    let transport = PtySubmitTransport {
        svc,
        pty_id,
        signal,
        status_signal,
        permit,
        submit_cycle: Arc::clone(&submit_cycle),
        submit_generation,
        paste_capability,
        turn_signaled: std::cell::Cell::new(false),
    };
    let mut key_guard = |allow_initial_key: bool| {
        let (tab_id, agent_id, session_bound, session_id, cwd, owns_pty) = {
            let mux = svc.mux.read();
            let Some(tab_id) = mux.tab_id_for_pty(pty_id) else {
                return Err(InjectionError::AgentInputUnavailable {
                    pty_id: pty_id.to_string(),
                });
            };
            let Some(tab) = mux.get_tab(&tab_id) else {
                return Err(InjectionError::AgentInputUnavailable {
                    pty_id: pty_id.to_string(),
                });
            };
            (
                tab_id,
                tab.agent_type.clone(),
                tab.launch_session_authenticated(),
                tab.agent_session_id.clone(),
                tab.cwd.clone().or_else(|| tab.provisioned_cwd.clone()),
                tab.pty_id.as_deref() == Some(pty_id),
            )
        };
        if !owns_pty {
            return Err(InjectionError::AgentInputUnavailable {
                pty_id: pty_id.to_string(),
            });
        }
        let (screen, input) = {
            let screen_state = svc.screen_state.lock();
            (
                screen_state.get_screen_text(pty_id),
                screen_state.composer_input_state(pty_id),
            )
        };
        let Some(agent_id) = agent_id else {
            return if allow_initial_key {
                Ok(())
            } else {
                Err(InjectionError::AgentInputUnavailable {
                    pty_id: pty_id.to_string(),
                })
            };
        };
        let Some(screen) = screen else {
            return if allow_initial_key {
                Ok(())
            } else {
                Err(InjectionError::AgentInputUnavailable {
                    pty_id: pty_id.to_string(),
                })
            };
        };
        let snapshot = codeterm_plugin::tab_state::TabStateSnapshot {
            tab_id,
            agent_type: agent_id.clone(),
            session_id,
            cwd,
            screen_text: screen.clone(),
        };
        let readiness = svc.agent_registry.with_provider(&agent_id, |provider| {
            use codeterm_plugin::tab_state::TabState;

            let provider_tui = provider.screen_has_tui(&screen);
            let composer_ready = input.as_ref().is_some_and(|state| {
                let cursor_ready = crate::agent::offline_state_classifier::composer_cursor_ready(
                    state.cursor_visible,
                    state.cursor_row,
                    state.rows,
                ) || state
                    .inverse_caret_row
                    .zip(state.last_nonblank_row)
                    .is_some_and(|(row, bottom)| {
                        crate::agent::offline_state_classifier::composer_cursor_ready(
                            true,
                            row,
                            bottom.saturating_add(1),
                        )
                    });
                cursor_ready && (provider_tui || state.alternate_screen)
            });
            let classification = provider.classify_tab_state(&snapshot);
            let high_confidence = classification.as_ref().is_some_and(|value| {
                value.confidence >= crate::agent::state_reactor::GLOBAL_THRESHOLD
            });
            let structured_choice = high_confidence
                && classification
                    .as_ref()
                    .is_some_and(|value| value.state == TabState::ClarifyingQuestion)
                && provider.parse_prompt(&screen).is_some();
            let classified_dialog = high_confidence
                && classification
                    .as_ref()
                    .is_some_and(|value| value.state == TabState::UpdatePrompt);
            let trust_dialog = provider.directory_trust_prompt_response(&screen).is_some();
            (
                composer_ready,
                trust_dialog || structured_choice || classified_dialog,
            )
        });
        let Some((composer_ready, modal_visible)) = readiness else {
            return if allow_initial_key {
                Ok(())
            } else {
                Err(InjectionError::AgentInputUnavailable {
                    pty_id: pty_id.to_string(),
                })
            };
        };
        if modal_visible {
            return Err(InjectionError::AgentTuiPresent {
                pty_id: pty_id.to_string(),
            });
        }
        if allow_initial_key || session_bound || composer_ready {
            Ok(())
        } else {
            Err(InjectionError::AgentInputUnavailable {
                pty_id: pty_id.to_string(),
            })
        }
    };
    let result = run_with_submit_key(
        &transport,
        text,
        enter,
        submit_key,
        strategy,
        opts,
        &mut key_guard,
    )
    .inspect(|result| {
            if recovery_attempted(result) {
                // `recovered=false` is not a lost message: a TUI strategy proves submit
                // only by a turn transition, which a busy tab cannot produce.
                let level = if result.submit_observed {
                    log::Level::Debug
                } else {
                    log::Level::Trace
                };
                log::log!(
                    level,
                    "[submit-recovery] pty={} recovered={} bytes_written={} key_writes={} elapsed_ms={}",
                    pty_id,
                    result.submit_observed,
                    result.bytes_written,
                    result.submit_key_writes,
                    result.elapsed.as_millis()
                );
            }
        });
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
    retire_submit_cycle(pty_id, &submit_cycle);
    result
}

pub(crate) fn write_terminal_reply(
    svc: &ServiceContext,
    pty_id: &str,
    reply: &[u8],
) -> Result<(), String> {
    svc.pty.lock().write(pty_id, reply)
}

#[cfg(test)]
mod classification_tests;
#[cfg(test)]
mod collapsed_paste_tests;
#[cfg(test)]
mod dropped_enter_tests;
#[cfg(test)]
mod paste_encoding_tests;
#[cfg(test)]
mod submission_matrix;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod turn_signal_tests;
