use std::cell::{Cell, RefCell};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::agent::provider::{VerifySubmitStrategy, DEFAULT_SUBMIT_RETRY_OFFSETS};
use crate::agent::status::TabStatus;
use crate::pty::screen_state::ScreenChangeSignal;
use crate::service::inject_verify::{
    screen_echo_evidence_count, InjectionError, InjectionResult, VerifyOpts,
};
use crate::service::ServiceContext;
mod cycle;
mod input_reservation;
pub(crate) use input_reservation::reserve_command_input;
mod evidence_watcher;
#[cfg(test)]
mod exit_tests;
mod line_mode;
mod paste_encoding;
mod payload_write;
mod readiness;
pub(crate) use paste_encoding::live_command_input;
use paste_encoding::PasteEncodingCapability;
#[cfg(test)]
use paste_encoding::{
    encode_paste_frame, encode_paste_frame_with_tail, xterm_paste, BRACKETED_PASTE_END,
    BRACKETED_PASTE_START,
};
use readiness::{exact_session_turn_source_available, pty_owner_guard, submit_key_readiness};
pub(crate) use readiness::{observe_delivery_readiness, DELIVERY_MODAL_OPEN};
mod recovery_guard;
mod runner;
pub(crate) use cycle::{
    claim_command_submit_cycle, claim_exit_submit_cycle, claim_submit_cycle,
    notify_submit_cycle_provider_exit, notify_submit_cycle_status_transition, wake_submit_cycle,
    wake_submit_cycle_output, SubmitCycleClaim,
};
use cycle::{retire_submit_cycle, CycleWait, SubmitCycle, SubmitCycleSignal};
use evidence_watcher::watch_session_evidence;
pub(crate) use recovery_guard::nudge_unbound_launch_draft;
#[cfg(test)]
use recovery_guard::{recovery_dialog_guard, recovery_session_authorized};
#[cfg(test)]
use runner::run;
#[cfg(test)]
use runner::run_with_submit_key;
#[cfg(test)]
use runner::run_with_submit_key_and_follow_up;
use runner::run_with_submit_key_and_frame;

const TAIL_ROWS: usize = 8;
const MAX_SUBMIT_KEY_WRITES: usize = DEFAULT_SUBMIT_RETRY_OFFSETS.len() + 1;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeferredSubmitWait {
    Ready,
    Woken,
    Deadline,
    Inactive,
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
    fn has_exact_user_turn_evidence(&self) -> bool {
        true
    }
    fn exact_user_turn_present(&self, _text: &str) -> bool {
        self.turn_started()
    }
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
    fn wait_retry_deadline_for_turn(&self, deadline: Instant, text: &str) -> bool {
        self.wait_retry_deadline(deadline);
        self.exact_user_turn_present(text)
    }
    fn wait_initial_key_deadline_for_turn(&self, deadline: Instant, text: &str) -> bool {
        self.wait_retry_deadline_for_turn(deadline, text)
    }
    fn wait_for_submit_event(&self, _deadline: Instant) -> bool {
        false
    }
    fn dialog_guard(&self) -> Result<(), InjectionError> {
        Ok(())
    }
    fn prewrite_guard(&self) -> Result<(), InjectionError> {
        self.dialog_guard()
    }
    fn prepare_paste(&self, _text: &str, _enter: bool) {}
    fn wait_for_deferred_submit_ready(&self, deadline: Instant) -> DeferredSubmitWait {
        if self.wait_for_submit_event(deadline) {
            DeferredSubmitWait::Woken
        } else {
            DeferredSubmitWait::Deadline
        }
    }
    fn max_submit_keys(&self) -> usize {
        MAX_SUBMIT_KEY_WRITES
    }
    fn retry_active(&self) -> bool {
        true
    }
    fn write_paste(&self, text: &str, deadline: Instant) -> Result<usize, InjectionError>;
    fn write_paste_with_frame(
        &self,
        text: &str,
        typed_prefix: Option<&str>,
        pasted_tail: Option<&str>,
        typed_suffix: Option<&str>,
        deadline: Instant,
    ) -> Result<usize, InjectionError> {
        let framed = format!(
            "{}{text}{}{}",
            typed_prefix.unwrap_or_default(),
            pasted_tail.unwrap_or_default(),
            typed_suffix.unwrap_or_default()
        );
        self.write_paste(&framed, deadline)
    }
    fn write_submit_key(&self, key: &[u8], deadline: Instant) -> Result<(), InjectionError>;
}

struct PtySubmitTransport<'a> {
    svc: &'a ServiceContext,
    pty_id: &'a str,
    signal: RefCell<Arc<ScreenChangeSignal>>,
    #[allow(dead_code)]
    status_signal: Arc<codeterm_types::pty::TabStatusSignal>,
    permit: Option<crate::pty::write_queue::GuardedInputPermit>,
    pre_write_guard: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    payload_queued: Option<Arc<dyn Fn() + Send + Sync>>,
    submit_cycle: Arc<SubmitCycle>,
    submit_claim: SubmitCycleClaim,
    message_send: bool,
    submit_generation: u64,
    paste_capability: PasteEncodingCapability,
    evidence_anchor: RefCell<Option<crate::service::chat_send_transaction::SendEvidenceAnchor>>,
    evidence_tab_id: RefCell<Option<String>>,
    evidence_binding_nonce: RefCell<Option<String>>,
    evidence_pty_generation: Cell<Option<u64>>,
    require_exact_session_turn: bool,
    max_submit_keys: usize,
    output_generation_at_start: Cell<u64>,
    turn_status_generation_at_start: Cell<u64>,
    deferred_ready_seen: Cell<bool>,
    turn_evidence_cursor: RefCell<crate::service::chat_send_evidence::ExactTurnCursor>,
    trace: Option<crate::flow_trace::PtyFlow>,
    wake_tx: SyncSender<SubmitCycleSignal>,
    wake_rx: Receiver<SubmitCycleSignal>,
    _session_evidence_watcher: RefCell<Option<notify::RecommendedWatcher>>,
    /// Stream-preview turn signal fires once per transaction, at the first
    /// successful submit-key enqueue. Signal only — never gates delivery.
    turn_signaled: std::cell::Cell<bool>,
    recovery_refusal: Cell<Option<&'static str>>,
}

#[cfg(test)]
fn initial_submit_frames(
    text: &str,
    key: &[u8],
    live_mode: Option<bool>,
    capability: PasteEncodingCapability,
) -> Vec<Vec<u8>> {
    vec![
        encode_paste_frame(text, None, None, live_mode, capability),
        key.to_vec(),
    ]
}

impl SubmitTransport for PtySubmitTransport<'_> {
    fn max_submit_keys(&self) -> usize {
        self.max_submit_keys
    }

    fn dialog_guard(&self) -> Result<(), InjectionError> {
        pty_owner_guard(self.svc, self.pty_id)
    }

    fn prepare_paste(&self, text: &str, enter: bool) {
        let tab_id = self.svc.mux.read().tab_id_for_pty(self.pty_id);
        let evidence = (enter || self.require_exact_session_turn)
            .then(|| {
                tab_id
                    .as_deref()
                    .map(|tab_id| self.svc.chat_send_anchor(tab_id))
            })
            .flatten();
        if enter {
            if let (Some(tab_id), Some(evidence)) = (tab_id.as_deref(), evidence.as_ref()) {
                self.svc.ensure_chat_send_append_stream(tab_id, evidence);
            }
        }
        let anchor = evidence
            .as_ref()
            .and_then(|evidence| evidence.anchor.clone());
        let binding_nonce = evidence
            .as_ref()
            .and_then(|evidence| evidence.live_binding.as_ref())
            .and_then(|binding| binding.binding_nonce.clone());
        if let Some(trace) = &self.trace {
            trace.watch(
                self.svc,
                anchor.as_ref(),
                binding_nonce.as_deref(),
                text,
                enter,
            );
        }
        *self.evidence_tab_id.borrow_mut() = tab_id;
        *self.evidence_anchor.borrow_mut() = anchor.clone();
        *self.evidence_binding_nonce.borrow_mut() = binding_nonce;
        self.evidence_pty_generation
            .set(self.svc.screen_state.lock().pty_generation(self.pty_id));
        *self.turn_evidence_cursor.borrow_mut() =
            crate::service::chat_send_evidence::ExactTurnCursor::for_anchor(anchor.as_ref());
        *self.signal.borrow_mut() = self
            .svc
            .screen_state
            .lock()
            .arm_screen_change_signal(self.pty_id);
        self.output_generation_at_start
            .set(self.submit_cycle.output_generation());
        self.turn_status_generation_at_start
            .set(self.status_signal.generation());
        self.deferred_ready_seen.set(false);
        *self._session_evidence_watcher.borrow_mut() = (self.require_exact_session_turn && enter)
            .then(|| {
                watch_session_evidence(
                    &self.svc.agent_registry,
                    anchor.as_ref(),
                    self.wake_tx.clone(),
                )
            })
            .flatten();
        self.svc.feed_input_parser(self.pty_id, text, enter);
    }

    fn observe(&self) -> Option<Observation> {
        let state = self.svc.screen_state.lock();
        state
            .tail_text(self.pty_id, TAIL_ROWS)
            .map(|tail| Observation { tail })
    }

    fn draft_evidence_count(&self, text: &str) -> usize {
        composer_draft_evidence(self.svc, self.pty_id, text)
    }

    fn generation(&self) -> u64 {
        self.signal.borrow().generation()
    }

    fn wait_change(&self, generation: u64, deadline: Instant) -> bool {
        let signal = self.signal.borrow().clone();
        deadline
            .checked_duration_since(Instant::now())
            .is_some_and(|remaining| signal.wait_after(generation, remaining))
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

    fn exact_user_turn_present(&self, text: &str) -> bool {
        if !self.require_exact_session_turn {
            return false;
        }
        let Some(anchor) = self.evidence_anchor.borrow().clone() else {
            return false;
        };
        let Some(tab_id) = self.evidence_tab_id.borrow().clone() else {
            return false;
        };
        let evidence_binding_nonce = self.evidence_binding_nonce.borrow().clone();
        let current_binding = self.svc.mux.read().get_tab(&tab_id).is_some_and(|tab| {
            tab.agent_session_id.as_deref() == Some(anchor.session_id.as_str())
                && tab.session_binding_nonce.as_deref() == evidence_binding_nonce.as_deref()
                && (self.max_submit_keys != 1
                    || tab.reported_session_binding_nonce.as_deref()
                        == evidence_binding_nonce.as_deref())
                && tab.pty_id.as_deref() == Some(self.pty_id)
        });
        let current_generation = self.svc.screen_state.lock().pty_generation(self.pty_id);
        if !current_binding || current_generation != self.evidence_pty_generation.get() {
            return false;
        }
        let found = crate::service::chat_send_evidence::contains_exact_user_turn_since(
            &self.svc.agent_registry,
            &anchor,
            text,
            &mut self.turn_evidence_cursor.borrow_mut(),
        );
        if found {
            self.svc
                .note_session_conversation(&tab_id, &anchor.session_id);
            if let Some(trace) = &self.trace {
                trace.confirmed(
                    &anchor,
                    evidence_binding_nonce.as_deref(),
                    self.turn_evidence_cursor
                        .borrow()
                        .matched_user_id
                        .as_deref(),
                );
            }
        }
        found
    }

    fn turn_generation(&self) -> u64 {
        self.status_signal.generation()
    }

    fn has_exact_user_turn_evidence(&self) -> bool {
        exact_session_turn_source_available(
            self.require_exact_session_turn,
            self.evidence_anchor.borrow().is_some() || self.max_submit_keys == 1,
        )
    }

    fn wait_turn_change(&self, turn_generation: u64, deadline: Instant) -> bool {
        deadline
            .checked_duration_since(Instant::now())
            .is_some_and(|remaining| self.status_signal.wait_after(turn_generation, remaining))
    }

    fn wait_retry_deadline_for_turn(&self, deadline: Instant, text: &str) -> bool {
        while self.retry_active() {
            if self.require_exact_session_turn && self.exact_user_turn_present(text) {
                return true;
            }
            match self.submit_cycle.wait_for_wake_until(
                self.submit_generation,
                deadline,
                &self.wake_rx,
            ) {
                CycleWait::Event => self.observe_deferred_readiness(),
                CycleWait::Deadline | CycleWait::Inactive => return false,
            };
            if self.require_exact_session_turn && self.exact_user_turn_present(text) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
        }
        false
    }

    fn wait_initial_key_deadline_for_turn(&self, deadline: Instant, text: &str) -> bool {
        loop {
            if self.require_exact_session_turn && self.exact_user_turn_present(text) {
                return true;
            }
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return false;
            };
            if self.wake_rx.recv_timeout(remaining).is_err() {
                return false;
            }
        }
    }

    fn wait_for_submit_event(&self, deadline: Instant) -> bool {
        self.wait_for_cycle_event(deadline)
    }

    fn wait_for_deferred_submit_ready(&self, deadline: Instant) -> DeferredSubmitWait {
        self.wait_for_deferred_submit_event(deadline)
    }

    fn retry_active(&self) -> bool {
        self.submit_claim.is_current()
    }

    fn write_paste(&self, text: &str, deadline: Instant) -> Result<usize, InjectionError> {
        self.write_paste_with_frame(text, None, None, None, deadline)
    }

    fn write_paste_with_frame(
        &self,
        text: &str,
        typed_prefix: Option<&str>,
        pasted_tail: Option<&str>,
        typed_suffix: Option<&str>,
        deadline: Instant,
    ) -> Result<usize, InjectionError> {
        if text.is_empty() && typed_prefix.is_none() && typed_suffix.is_none() {
            return Ok(0);
        }
        let live_bracketed_paste_mode = self
            .svc
            .screen_state
            .lock()
            .bracketed_paste_mode(self.pty_id);
        let input = paste_encoding::encode_paste_frame_with_tail(
            text,
            typed_prefix,
            pasted_tail,
            typed_suffix,
            live_bracketed_paste_mode,
            self.paste_capability,
        );
        log::debug!(
            "[submit-audit] paste-encoding pty={} live_bracketed_paste_mode={:?} paste_capability={:?} source_bytes={} encoded_bytes={}",
            self.pty_id,
            live_bracketed_paste_mode,
            self.paste_capability,
            text.len(),
            input.len()
        );
        let session = self
            .svc
            .pty
            .lock()
            .get_session(self.pty_id)
            .ok_or_else(|| InjectionError::PtyClosed {
                pty_id: self.pty_id.to_string(),
            })?;
        if self.pre_write_guard.as_ref().is_some_and(|guard| !guard()) {
            return Err(InjectionError::AgentInputUnavailable {
                pty_id: self.pty_id.to_string(),
            });
        }
        self.dialog_guard()?;
        let timeout = deadline.saturating_duration_since(Instant::now());
        let callback = self.payload_queued.clone();
        let trace = self.trace.clone();
        self.svc.replace_switch_confirmation(self.pty_id, None);
        let switch_confirmation = self
            .svc
            .prepare_switch_confirmation(self.pty_id, text, None);
        let on_enqueued = move || {
            if let Some(confirmation) = switch_confirmation {
                confirmation.start();
            }
            if let Some(trace) = trace {
                trace.written();
            }
            if let Some(callback) = callback {
                callback();
            }
        };
        let result = if self.message_send {
            payload_write::MessagePayload::new(
                input.clone(),
                self.submit_claim.clone(),
                self.wake_tx.clone(),
            )
            .enqueue(
                &session.write_queue,
                self.permit.clone(),
                timeout,
                on_enqueued,
            )
            .map(|queued| queued.len())
            .map_err(|error| match error {
                payload_write::PayloadWriteFailure::NotQueued(error) => {
                    injection_write_error(self.pty_id, error)
                }
                payload_write::PayloadWriteFailure::Queued(error) => {
                    log::debug!(
                        "[submit-audit] queued-payload uncertainty pty={} error={error}",
                        self.pty_id
                    );
                    InjectionError::SubmitNeverConfirmed {
                        waited: Duration::ZERO,
                        screen_tail: String::new(),
                    }
                }
            })
        } else {
            let result = match self.permit.as_ref() {
                Some(permit) => session.write_and_flush_guarded_with_enqueued(
                    input.clone(),
                    permit.clone(),
                    timeout,
                    on_enqueued,
                ),
                None => session.write_and_flush_with_enqueued(input.clone(), timeout, on_enqueued),
            };
            result
                .map(|()| input.len())
                .map_err(|error| injection_write_error(self.pty_id, error))
        };
        result?;
        if self.permit.is_some() {
            crate::pty::capture::CaptureRegistry::global().tap_input(
                self.pty_id,
                "pty.write",
                &input,
            );
        }
        Ok(input.len())
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
            Some(permit) => session.write_and_flush_guarded_with_enqueued(
                key.to_vec(),
                permit.clone(),
                timeout,
                || self.submit_claim.payload_enqueued(self.wake_tx.clone()),
            ),
            None => session.write_and_flush_with_enqueued(key.to_vec(), timeout, || {
                self.submit_claim.payload_enqueued(self.wake_tx.clone())
            }),
        };
        log::debug!(
            "[submit-audit] transaction-key pty={} cycle={} send_id={:?} queue_result={:?}",
            self.pty_id,
            self.submit_generation,
            self.submit_claim.trace_id,
            result,
        );
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

fn composer_draft_evidence(svc: &ServiceContext, pty_id: &str, text: &str) -> usize {
    if text.is_empty() {
        return 1;
    }
    let state = svc.screen_state.lock();
    let lines = state
        .lines_and_tail(pty_id, TAIL_ROWS)
        .map(|(lines, _)| lines)
        .unwrap_or_default();
    let cursor_row = state
        .cursor_input_state(pty_id)
        .map(|(_, _, row, _)| row as usize)
        .unwrap_or_else(|| lines.len().saturating_sub(1));
    screen_echo_evidence_count(&composer_region_text(&lines, cursor_row), text)
}

pub(crate) fn submit_text(
    svc: &ServiceContext,
    pty_id: &str,
    text: &str,
    enter: bool,
    strategy: VerifySubmitStrategy,
    opts: VerifyOpts,
) -> Result<InjectionResult, InjectionError> {
    submit_text_with_key(svc, pty_id, text, enter, b"\r", strategy, opts, None, false)
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
    track_exact_session_turn: bool,
) -> Result<InjectionResult, InjectionError> {
    submit_text_with_key_and_follow_up(
        svc,
        pty_id,
        text,
        None,
        enter,
        submit_key,
        strategy,
        opts,
        permit,
        track_exact_session_turn,
    )
}

pub(crate) fn submit_text_with_key_and_follow_up(
    svc: &ServiceContext,
    pty_id: &str,
    text: &str,
    typed_follow_up: Option<&str>,
    enter: bool,
    submit_key: &[u8],
    strategy: VerifySubmitStrategy,
    opts: VerifyOpts,
    permit: Option<crate::pty::write_queue::GuardedInputPermit>,
    track_exact_session_turn: bool,
) -> Result<InjectionResult, InjectionError> {
    let claim = claim_submit_cycle(pty_id);
    submit_text_with_key_claimed_and_follow_up(
        svc,
        pty_id,
        text,
        typed_follow_up,
        enter,
        submit_key,
        strategy,
        opts,
        permit,
        track_exact_session_turn,
        claim,
    )
}

/// A submit key beyond the initial write means the bounded recovery burst ran.
/// Reporting on this rather than on `submit_anomaly` keeps `recovered` bivalent:
/// `submit_anomaly` is `two_phase && !submit_observed`, so it can never coexist
/// with a recovered submit.
pub(crate) fn recovery_attempted(result: &InjectionResult) -> bool {
    result.submit_key_writes > 1
}

pub(crate) fn submit_text_with_key_claimed_and_follow_up(
    svc: &ServiceContext,
    pty_id: &str,
    text: &str,
    typed_follow_up: Option<&str>,
    enter: bool,
    submit_key: &[u8],
    strategy: VerifySubmitStrategy,
    opts: VerifyOpts,
    permit: Option<crate::pty::write_queue::GuardedInputPermit>,
    track_exact_session_turn: bool,
    claim: SubmitCycleClaim,
) -> Result<InjectionResult, InjectionError> {
    submit_text_with_key_claimed_and_follow_up_revalidated(
        svc,
        pty_id,
        text,
        typed_follow_up,
        enter,
        submit_key,
        strategy,
        opts,
        permit,
        track_exact_session_turn,
        claim,
        None,
        false,
        None,
    )
}

pub(crate) fn submit_text_with_key_claimed_and_frame(
    svc: &ServiceContext,
    pty_id: &str,
    text: &str,
    typed_prefix: Option<&str>,
    pasted_tail: Option<&str>,
    typed_suffix: Option<&str>,
    enter: bool,
    submit_key: &[u8],
    strategy: VerifySubmitStrategy,
    opts: VerifyOpts,
    permit: Option<crate::pty::write_queue::GuardedInputPermit>,
    track_exact_session_turn: bool,
    claim: SubmitCycleClaim,
) -> Result<InjectionResult, InjectionError> {
    submit_text_with_key_claimed_and_frame_revalidated(
        svc,
        pty_id,
        text,
        typed_prefix,
        pasted_tail,
        typed_suffix,
        enter,
        submit_key,
        strategy,
        opts,
        permit,
        track_exact_session_turn,
        claim,
        None,
        false,
        None,
    )
}

pub(crate) fn submit_text_with_key_claimed_and_follow_up_revalidated(
    svc: &ServiceContext,
    pty_id: &str,
    text: &str,
    typed_follow_up: Option<&str>,
    enter: bool,
    submit_key: &[u8],
    strategy: VerifySubmitStrategy,
    opts: VerifyOpts,
    permit: Option<crate::pty::write_queue::GuardedInputPermit>,
    track_exact_session_turn: bool,
    claim: SubmitCycleClaim,
    pre_write_guard: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    allow_superseded_payload: bool,
    payload_queued: Option<Arc<dyn Fn() + Send + Sync>>,
) -> Result<InjectionResult, InjectionError> {
    submit_text_with_key_claimed_and_parts_revalidated(
        svc,
        pty_id,
        text,
        None,
        typed_follow_up,
        None,
        enter,
        submit_key,
        strategy,
        opts,
        permit,
        track_exact_session_turn,
        claim,
        pre_write_guard,
        allow_superseded_payload,
        payload_queued,
    )
}

pub(crate) fn submit_text_with_key_claimed_and_frame_revalidated(
    svc: &ServiceContext,
    pty_id: &str,
    text: &str,
    typed_prefix: Option<&str>,
    pasted_tail: Option<&str>,
    typed_suffix: Option<&str>,
    enter: bool,
    submit_key: &[u8],
    strategy: VerifySubmitStrategy,
    opts: VerifyOpts,
    permit: Option<crate::pty::write_queue::GuardedInputPermit>,
    track_exact_session_turn: bool,
    claim: SubmitCycleClaim,
    pre_write_guard: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    allow_superseded_payload: bool,
    payload_queued: Option<Arc<dyn Fn() + Send + Sync>>,
) -> Result<InjectionResult, InjectionError> {
    submit_text_with_key_claimed_and_parts_revalidated(
        svc,
        pty_id,
        text,
        typed_prefix,
        pasted_tail,
        typed_suffix,
        enter,
        submit_key,
        strategy,
        opts,
        permit,
        track_exact_session_turn,
        claim,
        pre_write_guard,
        allow_superseded_payload,
        payload_queued,
    )
}

fn submit_text_with_key_claimed_and_parts_revalidated(
    svc: &ServiceContext,
    pty_id: &str,
    text: &str,
    typed_prefix: Option<&str>,
    pasted_tail: Option<&str>,
    typed_suffix: Option<&str>,
    enter: bool,
    submit_key: &[u8],
    strategy: VerifySubmitStrategy,
    opts: VerifyOpts,
    permit: Option<crate::pty::write_queue::GuardedInputPermit>,
    track_exact_session_turn: bool,
    claim: SubmitCycleClaim,
    pre_write_guard: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    allow_superseded_payload: bool,
    payload_queued: Option<Arc<dyn Fn() + Send + Sync>>,
) -> Result<InjectionResult, InjectionError> {
    if claim.pty_id() != pty_id || claim.command_wait().is_some() {
        return Err(InjectionError::AgentInputUnavailable {
            pty_id: pty_id.to_string(),
        });
    }
    pty_owner_guard(svc, pty_id)?;
    if !allow_superseded_payload && !claim.is_current() {
        return Err(InjectionError::AgentInputUnavailable {
            pty_id: pty_id.to_string(),
        });
    }
    let submit_cycle = Arc::clone(&claim.cycle);
    let trace = crate::flow_trace::PtyFlow::transaction(
        svc,
        pty_id,
        claim.trace_id.as_deref(),
        text,
        enter,
    );
    let submit_generation = claim.generation;
    let (wake_tx, wake_rx) = mpsc::sync_channel(1);
    let signal = svc.screen_state.lock().arm_screen_change_signal(pty_id);
    let status_signal = svc.agent_tracker.lock().arm_status_change_signal(pty_id);
    let output_generation_at_start = submit_cycle.output_generation();
    let turn_status_generation_at_start = status_signal.generation();
    let (provider, paste_capability) = paste_encoding::resolve_pty_paste_capability(
        &svc.mux,
        &svc.agent_registry,
        pty_id,
        &strategy,
    );
    let composer = provider.and_then(|provider| {
        svc.agent_registry
            .with_provider(&provider, |provider| provider.composer_cursor_position())
    });
    let policy = line_mode::submit_policy(track_exact_session_turn, composer);
    let transport = PtySubmitTransport {
        svc,
        pty_id,
        signal: RefCell::new(signal),
        status_signal,
        permit,
        pre_write_guard,
        payload_queued,
        submit_cycle: Arc::clone(&submit_cycle),
        submit_claim: claim.clone(),
        message_send: enter,
        submit_generation,
        paste_capability,
        evidence_anchor: RefCell::new(None),
        evidence_tab_id: RefCell::new(None),
        evidence_binding_nonce: RefCell::new(None),
        evidence_pty_generation: Cell::new(None),
        require_exact_session_turn: policy.0,
        max_submit_keys: policy.1,
        output_generation_at_start: Cell::new(output_generation_at_start),
        turn_status_generation_at_start: Cell::new(turn_status_generation_at_start),
        turn_evidence_cursor: RefCell::new(
            crate::service::chat_send_evidence::ExactTurnCursor::for_anchor(None),
        ),
        trace,
        wake_tx,
        wake_rx,
        _session_evidence_watcher: RefCell::new(None),
        turn_signaled: std::cell::Cell::new(false),
        deferred_ready_seen: Cell::new(false),
        recovery_refusal: Cell::new(None),
    };
    let mut key_guard = |allow_initial_key: bool, _turn_was_started: bool| {
        recovery_guard::submit_key_guard(&transport, allow_initial_key, text)
    };
    let run = run_with_submit_key_and_frame(
        &transport,
        text,
        enter,
        submit_key,
        typed_prefix,
        pasted_tail,
        typed_suffix,
        strategy,
        opts,
        &mut key_guard,
    );
    let mut result = run.inspect(|result| {
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
    let expected_user_turn = if enter && !text.is_empty() {
        format!(
            "{}{text}{}{}",
            typed_prefix.unwrap_or_default(),
            pasted_tail.unwrap_or_default(),
            typed_suffix.unwrap_or_default()
        )
    } else {
        text.to_string()
    };
    let exact_turn_present =
        track_exact_session_turn && transport.exact_user_turn_present(&expected_user_turn);
    if exact_turn_present {
        if let Ok(result) = &mut result {
            result.submit_observed = true;
            result.submit_anomaly = false;
        }
    }
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
    submit_cycle.clear_waiter(submit_generation);
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
mod cycle_cancellation_tests;
#[cfg(test)]
mod deferred_key_tests;
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
#[cfg(test)]
mod unbound_recovery_tests;

#[cfg(test)]
mod submission_reporting_tests;

#[cfg(test)]
mod contention_tests;

#[cfg(test)]
mod modal_tests;
