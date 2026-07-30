use std::collections::HashMap;
use std::sync::{Arc, OnceLock, Weak};
use std::time::Instant;

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
        .map(|line| framed_composer_row(line).unwrap_or(line))
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
    fn write_paste(&self, text: &str, deadline: Instant) -> Result<usize, InjectionError>;
    fn write_submit_key(&self, key: &[u8], deadline: Instant) -> Result<(), InjectionError>;
}

struct PtySubmitTransport<'a> {
    svc: &'a ServiceContext,
    pty_id: &'a str,
    signal: Arc<ScreenChangeSignal>,
    status_signal: Arc<codeterm_types::pty::TabStatusSignal>,
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
            Some(TabStatus::Working | TabStatus::Waiting)
        )
    }

    fn turn_generation(&self) -> u64 {
        self.status_signal.generation()
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

fn wait_submission_acknowledged<T: SubmitTransport>(
    transport: &T,
    text: &str,
    baseline_evidence: usize,
    echo_observed: bool,
    turn_was_started: bool,
    armed: u64,
    turn_armed: u64,
    deadline: Instant,
    strategy: &VerifySubmitStrategy,
) -> bool {
    let mut generation = armed;
    loop {
        if submission_acknowledged_now(
            transport,
            text,
            baseline_evidence,
            echo_observed,
            turn_was_started,
            armed,
            turn_armed,
            strategy,
        ) {
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
    echo_observed: bool,
    turn_was_started: bool,
    armed: u64,
    turn_armed: u64,
    strategy: &VerifySubmitStrategy,
) -> bool {
    let changed = transport.generation() != armed;
    let fresh_turn = changed && !turn_was_started && transport.turn_started();
    let draft_left_composer =
        echo_observed && changed && transport.draft_evidence_count(text) <= baseline_evidence;
    if matches!(strategy, VerifySubmitStrategy::TuiInputDrained) {
        let completed_turn = draft_left_composer && transport.turn_generation() != turn_armed;
        return fresh_turn || completed_turn;
    }
    draft_left_composer || fresh_turn
}

fn wait_paste_ingested<T: SubmitTransport>(
    transport: &T,
    text: &str,
    baseline_evidence: usize,
    armed: u64,
    deadline: Instant,
) -> bool {
    let mut generation = armed;
    loop {
        if transport.generation() != armed
            && transport.draft_evidence_count(text) > baseline_evidence
        {
            return true;
        }
        if !transport.wait_change(generation, deadline) {
            return false;
        }
        generation = transport.generation();
    }
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
    strategy: VerifySubmitStrategy,
    opts: VerifyOpts,
) -> Result<InjectionResult, InjectionError> {
    let started = Instant::now();
    let paste_deadline = started + opts.echo_timeout;
    let turn_deadline = paste_deadline + opts.submit_timeout;
    let two_phase = enter && opts.verify_submit;

    let baseline_evidence = transport.draft_evidence_count(text);
    let paste_armed = transport.generation();
    let bytes_written = transport.write_paste(text, turn_deadline)?;

    // A key-only submit pastes nothing, so there is no payload echo to gate on;
    // verifying one would compare the composer against an empty expectation.
    let echo_observed = if text.is_empty() {
        true
    } else if enter {
        wait_paste_ingested(
            transport,
            text,
            baseline_evidence,
            paste_armed,
            paste_deadline,
        )
    } else {
        transport.draft_evidence_count(text) > baseline_evidence
    };

    if !enter {
        return Ok(InjectionResult {
            bytes_written,
            echo_observed,
            submit_observed: false,
            submit_anomaly: false,
            elapsed: started.elapsed(),
        });
    }

    if !echo_observed {
        let screen_tail = transport
            .observe()
            .map(|state| state.tail)
            .unwrap_or_default();
        return Err(if opts.verify_echo {
            InjectionError::EchoMismatch {
                expected_substr: text.chars().take(120).collect(),
                screen_tail,
            }
        } else {
            InjectionError::SubmitNeverConfirmed {
                waited: started.elapsed(),
                screen_tail,
            }
        });
    }

    let turn_was_started = transport.turn_started();
    let mut submit_observed = !two_phase;
    let mut submit_key_written = false;
    for _ in 0..MAX_SUBMIT_KEY_WRITES {
        // The first CR is the delivery itself and always goes out; a RETRY
        // write may never begin with an expired phase deadline — enqueueing it
        // can only produce a doomed ack wait or a stray late keypress.
        if submit_key_written && Instant::now() >= turn_deadline {
            break;
        }
        let enter_armed = transport.generation();
        let turn_armed = transport.turn_generation();
        // The paste already reached the PTY: a submit-key write failure can
        // only mean unconfirmed submission, never a zero-write terminal error.
        if let Err(error) = transport.write_submit_key(submit_key, turn_deadline) {
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
        submit_key_written = true;
        if !two_phase {
            break;
        }
        submit_observed = wait_submission_acknowledged(
            transport,
            text,
            baseline_evidence,
            echo_observed,
            turn_was_started,
            enter_armed,
            turn_armed,
            turn_deadline,
            &strategy,
        );
        if submit_observed {
            break;
        }
    }

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
    let status_signal = svc.agent_tracker.lock().arm_status_change_signal(pty_id);
    svc.feed_input_parser(pty_id, text, enter);
    let transport = PtySubmitTransport {
        svc,
        pty_id,
        signal,
        status_signal,
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
mod classification_tests;
#[cfg(test)]
mod collapsed_paste_tests;
#[cfg(test)]
mod submission_matrix;
#[cfg(test)]
mod tests;
