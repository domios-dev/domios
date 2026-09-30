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
mod evidence_watcher;
pub(crate) use cycle::{
    claim_submit_cycle, wake_submit_cycle, wake_submit_cycle_output, wake_submit_cycle_status,
    SubmitCycleClaim,
};
use cycle::{retire_submit_cycle, CycleWait, SubmitCycle, SubmitCycleSignal};
use evidence_watcher::watch_session_evidence;

const TAIL_ROWS: usize = 8;
const MAX_SUBMIT_KEY_WRITES: usize = DEFAULT_SUBMIT_RETRY_OFFSETS.len() + 1;
const BRACKETED_PASTE_START: &str = "\x1b[200~";
const BRACKETED_PASTE_END: &str = "\x1b[201~";

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
    fn wait_for_submit_event(&self, _deadline: Instant) -> bool {
        false
    }
    fn dialog_guard(&self) -> Result<(), InjectionError> {
        Ok(())
    }
    fn prepare_paste(&self, _text: &str, _enter: bool) {}
    fn wait_for_deferred_submit_ready(&self, deadline: Instant) -> DeferredSubmitWait {
        if self.wait_for_submit_event(deadline) {
            DeferredSubmitWait::Woken
        } else {
            DeferredSubmitWait::Deadline
        }
    }
    fn retry_active(&self) -> bool {
        true
    }
    fn write_paste(&self, text: &str, deadline: Instant) -> Result<usize, InjectionError>;
    fn write_submit_key(&self, key: &[u8], deadline: Instant) -> Result<(), InjectionError>;
}

struct PtySubmitTransport<'a> {
    svc: &'a ServiceContext,
    pty_id: &'a str,
    signal: RefCell<Arc<ScreenChangeSignal>>,
    #[allow(dead_code)]
    status_signal: Arc<codeterm_types::pty::TabStatusSignal>,
    permit: Option<crate::pty::write_queue::GuardedInputPermit>,
    submit_cycle: Arc<SubmitCycle>,
    submit_generation: u64,
    paste_capability: PasteEncodingCapability,
    evidence_anchor: RefCell<Option<crate::service::chat_send_transaction::SendEvidenceAnchor>>,
    evidence_tab_id: RefCell<Option<String>>,
    evidence_binding_nonce: RefCell<Option<String>>,
    evidence_pty_generation: Cell<Option<u64>>,
    require_exact_session_turn: bool,
    output_generation_at_start: Cell<u64>,
    turn_status_generation_at_start: Cell<u64>,
    deferred_ready_seen: Cell<bool>,
    turn_evidence_cursor: RefCell<crate::service::chat_send_evidence::ExactTurnCursor>,
    wake_tx: SyncSender<SubmitCycleSignal>,
    wake_rx: Receiver<SubmitCycleSignal>,
    _session_evidence_watcher: RefCell<Option<notify::RecommendedWatcher>>,
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

#[cfg(test)]
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
    fn dialog_guard(&self) -> Result<(), InjectionError> {
        provider_dialog_guard(self.svc, self.pty_id)
    }

    fn prepare_paste(&self, text: &str, enter: bool) {
        let tab_id = self.svc.mux.read().tab_id_for_pty(self.pty_id);
        let evidence = self
            .require_exact_session_turn
            .then(|| {
                tab_id
                    .as_deref()
                    .map(|tab_id| self.svc.chat_send_anchor(tab_id))
            })
            .flatten();
        let anchor = evidence
            .as_ref()
            .and_then(|evidence| evidence.anchor.clone());
        let binding_nonce = evidence
            .as_ref()
            .and_then(|evidence| evidence.live_binding.as_ref())
            .and_then(|binding| binding.binding_nonce.clone());
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
                && tab.pty_id.as_deref() == Some(self.pty_id)
        });
        let current_generation = self.svc.screen_state.lock().pty_generation(self.pty_id);
        if !current_binding || current_generation != self.evidence_pty_generation.get() {
            return false;
        }
        crate::service::chat_send_evidence::contains_exact_user_turn_since(
            &self.svc.agent_registry,
            &anchor,
            text,
            &mut self.turn_evidence_cursor.borrow_mut(),
        )
    }

    fn turn_generation(&self) -> u64 {
        self.status_signal.generation()
    }

    fn has_exact_user_turn_evidence(&self) -> bool {
        self.require_exact_session_turn
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
                CycleWait::Event(_) => self.observe_deferred_readiness(),
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

    fn wait_for_submit_event(&self, deadline: Instant) -> bool {
        self.wait_for_cycle_event(deadline)
    }

    fn wait_for_deferred_submit_ready(&self, deadline: Instant) -> DeferredSubmitWait {
        self.wait_for_deferred_submit_event(deadline)
    }

    fn retry_active(&self) -> bool {
        self.submit_cycle.is_current(self.submit_generation)
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

fn provider_dialog_visible(
    svc: &ServiceContext,
    agent_id: &str,
    snapshot: &codeterm_plugin::tab_state::TabStateSnapshot,
    screen: &str,
) -> bool {
    svc.agent_registry
        .with_provider(agent_id, |provider| {
            use codeterm_plugin::tab_state::TabState;

            let classification = provider.classify_tab_state(snapshot);
            let high_confidence = classification.as_ref().is_some_and(|value| {
                value.confidence >= crate::agent::state_reactor::GLOBAL_THRESHOLD
            });
            let structured_choice = high_confidence
                && classification
                    .as_ref()
                    .is_some_and(|value| value.state == TabState::ClarifyingQuestion)
                && provider.parse_prompt(screen).is_some();
            let classified_dialog = high_confidence
                && classification
                    .as_ref()
                    .is_some_and(|value| value.state == TabState::UpdatePrompt);
            structured_choice
                || classified_dialog
                || provider.directory_trust_prompt_response(screen).is_some()
        })
        .unwrap_or(false)
}

fn provider_dialog_guard(svc: &ServiceContext, pty_id: &str) -> Result<(), InjectionError> {
    let (tab_id, agent_id, session_id, cwd, owns_pty) = {
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
    let Some(agent_id) = agent_id else {
        return Ok(());
    };
    let screen = svc.screen_state.lock().get_screen_text(pty_id);
    let Some(screen) = screen else {
        return Ok(());
    };
    let snapshot = codeterm_plugin::tab_state::TabStateSnapshot {
        tab_id,
        agent_type: agent_id.clone(),
        session_id,
        cwd,
        screen_text: screen.clone(),
    };
    if provider_dialog_visible(svc, &agent_id, &snapshot, &screen) {
        Err(InjectionError::AgentTuiPresent {
            pty_id: pty_id.to_string(),
        })
    } else {
        Ok(())
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
        return if transport.has_exact_user_turn_evidence() {
            transport.exact_user_turn_present(text)
        } else {
            fresh_turn
        };
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
    if let Err(error) = transport.dialog_guard() {
        return Err(error);
    }
    transport.prepare_paste(text, enter);
    let started = Instant::now();
    let turn_deadline = started + opts.submit_timeout;
    let two_phase = enter && opts.verify_submit;
    let exact_turn_evidence = matches!(&strategy, VerifySubmitStrategy::TuiInputDrained { .. })
        && transport.has_exact_user_turn_evidence();
    let initial_key_delay = if enter && !text.is_empty() {
        strategy.initial_key_delay()
    } else {
        Duration::ZERO
    };

    let baseline_evidence = transport.draft_evidence_count(text);
    let turn_was_started = enter.then(|| transport.turn_started());
    let bytes_written = transport.write_paste(text, turn_deadline)?;
    let mut submit_key_writes = 0;
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
    let mut first_submit_key_at = (submit_key_writes > 0).then_some(submit_started);
    let mut submit_observed = !two_phase;
    let mut dialog_blocked = false;
    while submit_key_writes < MAX_SUBMIT_KEY_WRITES {
        if (submit_key_writes > 0 && submit_observed)
            || (exact_turn_evidence && transport.exact_user_turn_present(text))
        {
            submit_observed = true;
            break;
        }
        if submit_key_writes == 0 && !initial_key_delay.is_zero() {
            let first_key_deadline = submit_started + initial_key_delay;
            if transport.wait_retry_deadline_for_turn(first_key_deadline.min(turn_deadline), text) {
                submit_observed = true;
                break;
            }
            if !transport.retry_active()
                || (submit_key_writes > 0 && Instant::now() >= turn_deadline)
            {
                break;
            }
        } else if submit_key_writes > 0 {
            if !transport.retry_active() {
                break;
            }
            let retry_offsets = strategy.retry_offsets();
            let offset = retry_offsets
                .get(submit_key_writes - 1)
                .copied()
                .unwrap_or(*retry_offsets.last().expect("non-empty"));
            let retry_deadline = first_submit_key_at.unwrap_or(submit_started) + offset;
            if transport.wait_retry_deadline_for_turn(retry_deadline.min(turn_deadline), text) {
                submit_observed = true;
                break;
            }
            if !transport.retry_active() || Instant::now() >= turn_deadline {
                break;
            }
        }
        if exact_turn_evidence && transport.exact_user_turn_present(text) {
            submit_observed = true;
            break;
        }
        let allow_initial_key = submit_key_writes == 0;
        let mut key_error = None;
        let key_ready = loop {
            match key_guard(allow_initial_key) {
                Ok(()) => break true,
                Err(error @ InjectionError::AgentTuiPresent { .. }) if bytes_written > 0 => {
                    dialog_blocked = true;
                    key_error = Some(error);
                    break false;
                }
                Err(error @ InjectionError::AgentInputUnavailable { .. }) if bytes_written > 0 => {
                    if transport.wait_for_submit_event(turn_deadline) {
                        continue;
                    }
                    key_error = Some(error);
                    break false;
                }
                Err(error) => {
                    key_error = Some(error);
                    break false;
                }
            }
        };
        if !key_ready {
            if submit_key_writes > 0 {
                break;
            }
            if bytes_written == 0 {
                return Err(key_error.expect("failed key guard retains its error"));
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
        if exact_turn_evidence && transport.exact_user_turn_present(text) {
            submit_observed = true;
            break;
        }
        let enter_armed = transport.generation();
        // A prior payload or submit key makes this failure submission uncertainty.
        let key_deadline = if Instant::now() < turn_deadline {
            turn_deadline
        } else {
            Instant::now() + opts.submit_timeout
        };
        if let Err(error) = transport.write_submit_key(submit_key, key_deadline) {
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
        first_submit_key_at.get_or_insert_with(Instant::now);
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
        submit_observed |= exact_turn_evidence && transport.exact_user_turn_present(text);
        if submit_observed {
            break;
        }
    }

    if two_phase
        && !submit_observed
        && bytes_written > 0
        && submit_key_writes > 0
        && !dialog_blocked
        && transport.retry_active()
    {
        let deferred_deadline = Instant::now() + opts.submit_timeout;
        loop {
            if exact_turn_evidence && transport.exact_user_turn_present(text) {
                submit_observed = true;
                break;
            }
            match transport.wait_for_deferred_submit_ready(deferred_deadline) {
                DeferredSubmitWait::Ready => {}
                DeferredSubmitWait::Woken => {
                    if exact_turn_evidence && transport.exact_user_turn_present(text) {
                        submit_observed = true;
                        break;
                    }
                    continue;
                }
                DeferredSubmitWait::Deadline | DeferredSubmitWait::Inactive => break,
            }
            if exact_turn_evidence && transport.exact_user_turn_present(text) {
                submit_observed = true;
                break;
            }
            match key_guard(false) {
                Ok(()) => {}
                Err(InjectionError::AgentTuiPresent { .. }) => break,
                Err(InjectionError::AgentInputUnavailable { .. }) => {
                    if transport.wait_for_submit_event(deferred_deadline) {
                        continue;
                    }
                    break;
                }
                Err(_) => break,
            }
            let enter_armed = transport.generation();
            let key_deadline = Instant::now() + opts.submit_timeout;
            if let Err(error) = transport.write_submit_key(submit_key, key_deadline) {
                log::warn!(
                    "[submit-transaction] deferred submit-key write failed ({error}); classified unconfirmed"
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
            if exact_turn_evidence {
                while !transport.exact_user_turn_present(text) {
                    if !transport.wait_for_submit_event(deferred_deadline) {
                        break;
                    }
                }
                submit_observed = transport.exact_user_turn_present(text);
            } else {
                submit_observed = submission_acknowledged_now(
                    transport,
                    text,
                    baseline_evidence,
                    paste_observed,
                    turn_was_started,
                    enter_armed,
                    &strategy,
                );
            }
            break;
        }
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
    let claim = claim_submit_cycle(pty_id);
    submit_text_with_key_claimed(
        svc,
        pty_id,
        text,
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

pub(crate) fn submit_text_with_key_claimed(
    svc: &ServiceContext,
    pty_id: &str,
    text: &str,
    enter: bool,
    submit_key: &[u8],
    strategy: VerifySubmitStrategy,
    opts: VerifyOpts,
    permit: Option<crate::pty::write_queue::GuardedInputPermit>,
    track_exact_session_turn: bool,
    claim: SubmitCycleClaim,
) -> Result<InjectionResult, InjectionError> {
    if claim.pty_id() != pty_id {
        return Err(InjectionError::AgentInputUnavailable {
            pty_id: pty_id.to_string(),
        });
    }
    provider_dialog_guard(svc, pty_id)?;
    if !claim.wait_until_active(Instant::now() + opts.submit_timeout) {
        return Err(InjectionError::AgentInputUnavailable {
            pty_id: pty_id.to_string(),
        });
    }
    let submit_cycle = Arc::clone(&claim.cycle);
    let submit_generation = claim.generation;
    let paste_capability = PasteEncodingCapability::for_strategy(&strategy);
    let (wake_tx, wake_rx) = mpsc::sync_channel(1);
    submit_cycle.install_waiter(submit_generation, wake_tx.clone());
    let signal = svc.screen_state.lock().arm_screen_change_signal(pty_id);
    let status_signal = svc.agent_tracker.lock().arm_status_change_signal(pty_id);
    let output_generation_at_start = submit_cycle.output_generation();
    let turn_status_generation_at_start = status_signal.generation();
    let transport = PtySubmitTransport {
        svc,
        pty_id,
        signal: RefCell::new(signal),
        status_signal,
        permit,
        submit_cycle: Arc::clone(&submit_cycle),
        submit_generation,
        paste_capability,
        evidence_anchor: RefCell::new(None),
        evidence_tab_id: RefCell::new(None),
        evidence_binding_nonce: RefCell::new(None),
        evidence_pty_generation: Cell::new(None),
        require_exact_session_turn: track_exact_session_turn,
        output_generation_at_start: Cell::new(output_generation_at_start),
        turn_status_generation_at_start: Cell::new(turn_status_generation_at_start),
        turn_evidence_cursor: RefCell::new(
            crate::service::chat_send_evidence::ExactTurnCursor::for_anchor(None),
        ),
        wake_tx,
        wake_rx,
        _session_evidence_watcher: RefCell::new(None),
        turn_signaled: std::cell::Cell::new(false),
        deferred_ready_seen: Cell::new(false),
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
        if allow_initial_key && agent_id.is_none() {
            transport.dialog_guard()?;
            return Ok(());
        }
        let (screen, input) = {
            let screen_state = svc.screen_state.lock();
            (
                screen_state.get_screen_text(pty_id),
                screen_state.composer_input_state(pty_id),
            )
        };
        let Some(agent_id) = agent_id else {
            return Err(InjectionError::AgentInputUnavailable {
                pty_id: pty_id.to_string(),
            });
        };
        let Some(screen) = screen else {
            return Err(InjectionError::AgentInputUnavailable {
                pty_id: pty_id.to_string(),
            });
        };
        let snapshot = codeterm_plugin::tab_state::TabStateSnapshot {
            tab_id: tab_id.clone(),
            agent_type: agent_id.clone(),
            session_id,
            cwd,
            screen_text: screen.clone(),
        };
        if provider_dialog_visible(svc, &agent_id, &snapshot, &screen) {
            return Err(InjectionError::AgentTuiPresent {
                pty_id: pty_id.to_string(),
            });
        }
        let readiness = svc.agent_registry.with_provider(&agent_id, |provider| {
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
            composer_ready
        });
        let Some(composer_ready) = readiness else {
            return Err(InjectionError::AgentInputUnavailable {
                pty_id: pty_id.to_string(),
            });
        };
        if composer_ready || (session_bound && !transport.turn_started()) {
            Ok(())
        } else {
            Err(InjectionError::AgentInputUnavailable {
                pty_id: pty_id.to_string(),
            })
        }
    };
    let mut result = run_with_submit_key(
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
    let exact_turn_present = track_exact_session_turn && transport.exact_user_turn_present(text);
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
mod submission_reporting_tests;
