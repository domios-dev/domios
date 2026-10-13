use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender};
use std::sync::{Arc, OnceLock, Weak};
use std::time::Instant;

use super::{DeferredSubmitWait, SubmitTransport};
use parking_lot::Mutex;

impl<'a> super::PtySubmitTransport<'a> {
    pub(super) fn composer_ready_now(&self) -> bool {
        let (tab_id, agent_id, owns_tab) = {
            let mux = self.svc.mux.read();
            let tab_id = mux.tab_id_for_pty(self.pty_id);
            let agent_id = tab_id
                .as_deref()
                .and_then(|tab_id| mux.get_tab(tab_id))
                .and_then(|tab| tab.agent_type.clone());
            let owns_tab = tab_id.as_deref().is_some_and(|tab_id| {
                mux.get_tab(tab_id)
                    .is_some_and(|tab| tab.pty_id.as_deref() == Some(self.pty_id))
            });
            (tab_id, agent_id, owns_tab)
        };
        let (Some(_tab_id), Some(agent_id), true) = (tab_id, agent_id, owns_tab) else {
            return false;
        };
        let (screen, input) = {
            let screen_state = self.svc.screen_state.lock();
            (
                screen_state.get_screen_text(self.pty_id),
                screen_state.composer_input_state(self.pty_id),
            )
        };
        let (Some(screen), Some(input)) = (screen, input) else {
            return false;
        };
        self.svc
            .agent_registry
            .with_provider(&agent_id, |provider| {
                let provider_tui = provider.screen_has_tui(&screen);
                let cursor_ready = crate::agent::offline_state_classifier::composer_cursor_ready(
                    input.cursor_visible,
                    input.cursor_row,
                    input.rows,
                ) || input
                    .inverse_caret_row
                    .zip(input.last_nonblank_row)
                    .is_some_and(|(row, bottom)| {
                        crate::agent::offline_state_classifier::composer_cursor_ready(
                            true,
                            row,
                            bottom.saturating_add(1),
                        )
                    });
                owns_tab && cursor_ready && (provider_tui || input.alternate_screen)
            })
            .unwrap_or(false)
    }

    pub(super) fn wait_for_cycle_event(&self, deadline: Instant) -> bool {
        match self
            .submit_cycle
            .wait_for_wake_until(self.submit_generation, deadline, &self.wake_rx)
        {
            CycleWait::Event => {
                self.observe_deferred_readiness();
                true
            }
            CycleWait::Deadline | CycleWait::Inactive => false,
        }
    }

    pub(super) fn observe_deferred_readiness(&self) -> bool {
        let idle_transition = self.status_signal.generation()
            != self.turn_status_generation_at_start.get()
            && !self.turn_started();
        let output_observed =
            self.submit_cycle.output_generation() != self.output_generation_at_start.get();
        let ready = idle_transition || (output_observed && self.composer_ready_now());
        if ready {
            self.deferred_ready_seen.set(true);
        }
        ready
    }

    pub(super) fn wait_for_deferred_submit_event(&self, deadline: Instant) -> DeferredSubmitWait {
        if !self.retry_active() {
            return DeferredSubmitWait::Inactive;
        }
        if self.deferred_ready_seen.get() || self.observe_deferred_readiness() {
            self.deferred_ready_seen.set(true);
            return DeferredSubmitWait::Ready;
        }
        match self
            .submit_cycle
            .wait_for_wake_until(self.submit_generation, deadline, &self.wake_rx)
        {
            CycleWait::Event if self.observe_deferred_readiness() => DeferredSubmitWait::Ready,
            CycleWait::Event => DeferredSubmitWait::Woken,
            CycleWait::Deadline => DeferredSubmitWait::Deadline,
            CycleWait::Inactive => DeferredSubmitWait::Inactive,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SubmitCycleSignal {
    Output,
    TurnStatus,
    SessionEvidence,
    Closed,
}

pub(super) enum CycleWait {
    Event,
    Deadline,
    Inactive,
}

#[derive(Default)]
struct SubmitCycleState {
    next_generation: u64,
    reservations: HashSet<u64>,
    current_generation: Option<u64>,
    waiter: Option<(u64, SyncSender<SubmitCycleSignal>)>,
    provider_exit: Option<Arc<AtomicBool>>,
}

pub(super) struct SubmitCycle {
    state: Mutex<SubmitCycleState>,
    output_generation: AtomicU64,
}

impl SubmitCycle {
    fn new() -> Self {
        Self {
            state: Mutex::new(SubmitCycleState {
                next_generation: 1,
                ..SubmitCycleState::default()
            }),
            output_generation: AtomicU64::new(0),
        }
    }

    pub(super) fn output_generation(&self) -> u64 {
        self.output_generation.load(Ordering::Acquire)
    }

    fn reserve(&self) -> u64 {
        let mut state = self.state.lock();
        let generation = state.next_generation;
        state.next_generation = state.next_generation.wrapping_add(1).max(1);
        state.reservations.insert(generation);
        generation
    }

    fn activate(&self, generation: u64, provider_exit: Option<Arc<AtomicBool>>) {
        let mut state = self.state.lock();
        if state.current_generation == Some(generation) {
            return;
        }
        if let Some((_, waiter)) = state.waiter.take() {
            let _ = waiter.try_send(SubmitCycleSignal::Closed);
        }
        state.current_generation = Some(generation);
        state.provider_exit = provider_exit;
    }

    fn provider_exited(&self) {
        let mut state = self.state.lock();
        let Some(observed) = state.provider_exit.take() else {
            return;
        };
        observed.store(true, Ordering::Release);
        state.current_generation = None;
        if let Some((_, waiter)) = state.waiter.take() {
            let _ = waiter.try_send(SubmitCycleSignal::Closed);
        }
    }

    pub(super) fn is_current(&self, generation: u64) -> bool {
        self.state.lock().current_generation == Some(generation)
    }

    fn is_empty(&self) -> bool {
        self.state.lock().reservations.is_empty()
    }

    fn finish(&self, generation: u64) {
        let mut state = self.state.lock();
        state.reservations.remove(&generation);
        if state.current_generation == Some(generation) {
            if let Some((_, waiter)) = state.waiter.take() {
                let _ = waiter.try_send(SubmitCycleSignal::Closed);
            }
            state.current_generation = None;
            state.provider_exit = None;
        }
    }

    pub(super) fn install_waiter(&self, generation: u64, sender: SyncSender<SubmitCycleSignal>) {
        let mut state = self.state.lock();
        if state.current_generation != Some(generation) {
            drop(state);
            let _ = sender.try_send(SubmitCycleSignal::Closed);
            return;
        }
        state.waiter = Some((generation, sender));
    }

    pub(super) fn wait_for_wake_until(
        &self,
        generation: u64,
        deadline: Instant,
        receiver: &Receiver<SubmitCycleSignal>,
    ) -> CycleWait {
        if !self.is_current(generation) {
            return CycleWait::Inactive;
        }
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            return CycleWait::Deadline;
        };
        match receiver.recv_timeout(remaining) {
            Ok(signal) if signal != SubmitCycleSignal::Closed && self.is_current(generation) => {
                CycleWait::Event
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) if self.is_current(generation) => {
                CycleWait::Deadline
            }
            _ => CycleWait::Inactive,
        }
    }

    pub(super) fn clear_waiter(&self, generation: u64) {
        let mut state = self.state.lock();
        if state
            .waiter
            .as_ref()
            .is_some_and(|(waiter_generation, _)| *waiter_generation == generation)
        {
            state.waiter.take();
        }
    }

    fn wake(&self, signal: SubmitCycleSignal) {
        if signal == SubmitCycleSignal::Output {
            self.output_generation.fetch_add(1, Ordering::AcqRel);
        }
        let sender = self
            .state
            .lock()
            .waiter
            .as_ref()
            .map(|(_, sender)| sender.clone());
        if let Some(sender) = sender {
            let _ = sender.try_send(signal);
        }
    }
}

fn submit_cycles() -> &'static Mutex<HashMap<String, Weak<SubmitCycle>>> {
    static CYCLES: OnceLock<Mutex<HashMap<String, Weak<SubmitCycle>>>> = OnceLock::new();
    CYCLES.get_or_init(|| Mutex::new(HashMap::new()))
}

struct ClaimLease {
    _input: Result<
        Option<super::input_reservation::InputReservation>,
        super::input_reservation::CommandWait,
    >,
    pty_id: String,
    cycle: Arc<SubmitCycle>,
    generation: u64,
    armed: AtomicBool,
}

impl Drop for ClaimLease {
    fn drop(&mut self) {
        self.cycle.finish(self.generation);
        retire_submit_cycle(&self.pty_id, &self.cycle);
    }
}

#[derive(Clone)]
pub(crate) struct SubmitCycleClaim {
    pty_id: String,
    pub(super) trace_id: Option<String>,
    pub(super) cycle: Arc<SubmitCycle>,
    pub(super) generation: u64,
    _lease: Arc<ClaimLease>,
    provider_exit: Option<Arc<AtomicBool>>,
}

impl SubmitCycleClaim {
    pub(crate) fn command_wait(&self) -> Option<super::input_reservation::CommandWait> {
        self._lease._input.as_ref().err().cloned()
    }

    pub(crate) fn provider_exited(&self) -> bool {
        self.provider_exit
            .as_ref()
            .is_some_and(|observed| observed.load(Ordering::Acquire))
    }

    pub(crate) fn with_trace_id(mut self, id: &str) -> Self {
        if crate::flow_trace::enabled() {
            self.trace_id = Some(id.into());
        }
        self
    }
    pub(crate) fn pty_id(&self) -> &str {
        &self.pty_id
    }

    pub(super) fn is_current(&self) -> bool {
        self.command_wait().is_none()
            && (!self._lease.armed.load(Ordering::Acquire)
                || self.cycle.is_current(self.generation))
    }

    #[cfg(test)]
    pub(crate) fn enqueue_test_payload(&self) {
        let (sender, _receiver) = std::sync::mpsc::sync_channel(1);
        self.payload_enqueued(sender);
    }

    pub(super) fn payload_enqueued(&self, waiter: SyncSender<SubmitCycleSignal>) {
        if !self._lease.armed.swap(true, Ordering::AcqRel) {
            self.cycle
                .activate(self.generation, self.provider_exit.clone());
            self.cycle.install_waiter(self.generation, waiter);
        }
    }
}

pub(crate) fn claim_submit_cycle(pty_id: &str) -> SubmitCycleClaim {
    claim_cycle(
        pty_id,
        None,
        super::input_reservation::reserve_send_input(pty_id).map(Some),
    )
}

pub(crate) fn claim_exit_submit_cycle(pty_id: &str) -> SubmitCycleClaim {
    claim_cycle(
        pty_id,
        Some(Arc::new(AtomicBool::new(false))),
        super::input_reservation::reserve_send_input(pty_id).map(Some),
    )
}

pub(crate) fn claim_command_submit_cycle(pty_id: &str) -> SubmitCycleClaim {
    claim_cycle(pty_id, None, Ok(None))
}

fn claim_cycle(
    pty_id: &str,
    provider_exit: Option<Arc<AtomicBool>>,
    input: Result<
        Option<super::input_reservation::InputReservation>,
        super::input_reservation::CommandWait,
    >,
) -> SubmitCycleClaim {
    let blocked = input.is_err();
    let cycle = if blocked {
        Arc::new(SubmitCycle::new())
    } else {
        let mut cycles = submit_cycles().lock();
        if let Some(cycle) = cycles.get(pty_id).and_then(Weak::upgrade) {
            cycle
        } else {
            let cycle = Arc::new(SubmitCycle::new());
            cycles.insert(pty_id.to_string(), Arc::downgrade(&cycle));
            cycle
        }
    };
    let generation = if blocked { 0 } else { cycle.reserve() };
    log::info!(
        "[submit-cycle] claim pty={} generation={}",
        pty_id,
        generation
    );
    SubmitCycleClaim {
        pty_id: pty_id.to_string(),
        trace_id: None,
        cycle: Arc::clone(&cycle),
        generation,
        _lease: Arc::new(ClaimLease {
            _input: input,
            pty_id: pty_id.to_string(),
            cycle,
            generation,
            armed: AtomicBool::new(false),
        }),
        provider_exit,
    }
}

pub(crate) fn notify_submit_cycle_provider_exit(pty_id: &str) {
    super::input_reservation::command_pty_exited(pty_id);
    let cycle = submit_cycles().lock().get(pty_id).and_then(Weak::upgrade);
    if let Some(cycle) = cycle {
        cycle.provider_exited();
    }
}

pub(crate) fn wake_submit_cycle(pty_id: &str) {
    wake_submit_cycle_with(pty_id, SubmitCycleSignal::SessionEvidence);
}

pub(crate) fn wake_submit_cycle_output(pty_id: &str) {
    wake_submit_cycle_with(pty_id, SubmitCycleSignal::Output);
}

pub(crate) fn wake_submit_cycle_status(pty_id: &str) {
    wake_submit_cycle_with(pty_id, SubmitCycleSignal::TurnStatus);
}

pub(crate) fn notify_submit_cycle_status_transition(
    pty_id: &str,
    previous: crate::agent::status::TabStatus,
    current: crate::agent::status::TabStatus,
) {
    if previous != current
        && (current == crate::agent::status::TabStatus::Idle
            || (previous.has_active_turn() && !current.has_active_turn()))
    {
        wake_submit_cycle_status(pty_id);
    }
}

#[cfg(test)]
pub(super) fn submit_cycle_registered(pty_id: &str) -> bool {
    submit_cycles().lock().contains_key(pty_id)
}

fn wake_submit_cycle_with(pty_id: &str, signal: SubmitCycleSignal) {
    let cycle = submit_cycles().lock().get(pty_id).and_then(Weak::upgrade);
    if let Some(cycle) = cycle {
        cycle.wake(signal);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::status::TabStatus;

    #[test]
    fn provider_turn_event_wakes_the_bounded_submit_wait() {
        let pty_id = "structured-turn-wake";
        let claim = claim_submit_cycle(pty_id);
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        claim.payload_enqueued(sender);

        wake_submit_cycle(pty_id);

        assert_eq!(receiver.try_recv(), Ok(SubmitCycleSignal::SessionEvidence));
    }

    #[test]
    fn idle_poll_transition_wakes_the_deferred_submit_waiter() {
        let pty_id = "idle-poll-submit-wake";
        let claim = claim_submit_cycle(pty_id);
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        claim.payload_enqueued(sender);

        notify_submit_cycle_status_transition(pty_id, TabStatus::Working, TabStatus::Idle);

        assert_eq!(receiver.try_recv(), Ok(SubmitCycleSignal::TurnStatus));
    }

    #[test]
    fn newer_payload_enqueue_replaces_the_active_cycle() {
        let pty_id = "queued-claim-deadline";
        let first = claim_submit_cycle(pty_id);
        let (sender, _) = std::sync::mpsc::sync_channel(1);
        first.payload_enqueued(sender);
        let second = claim_submit_cycle(pty_id);
        assert!(first.is_current());
        let (sender, _) = std::sync::mpsc::sync_channel(1);
        second.payload_enqueued(sender);

        assert!(!first.is_current());
        assert!(second.is_current());
        assert!(second.is_current());
    }
}

pub(super) fn retire_submit_cycle(pty_id: &str, cycle: &Arc<SubmitCycle>) {
    let mut cycles = submit_cycles().lock();
    let current_is_cycle = cycles
        .get(pty_id)
        .and_then(Weak::upgrade)
        .is_some_and(|current| Arc::ptr_eq(&current, cycle));
    if current_is_cycle && cycle.is_empty() {
        cycles.remove(pty_id);
    }
}
