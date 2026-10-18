use std::{sync::Arc, time::Duration};

use codeterm_commands::update::single_flight::{global, UpdateLease};
use codeterm_service_core::{update_state::UpdatePhase, UpdateStateHandle};
use codeterm_types::{
    native_update::{
        NativeUpdateAction, NativeUpdateAttachment, NativeUpdateDispatchOutcome,
        NativeUpdateExecutor, NativeUpdateRefusal, NativeUpdateRequest, NativeUpdateResult,
        NativeUpdateState, NativeUpdateUncertainty,
    },
    server_bootstrap::{ServerComponentManifest, ServerReady},
};
use parking_lot::Mutex;
use tokio::sync::watch;

const EVENT_DEADLINE: Duration = Duration::from_secs(30);
const MAX_AUTOMATIC_OBSERVATIONS: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeUpdateHoldReason {
    InvalidReply(NativeUpdateRefusal),
    DispatchDeadline,
    DispatchUncertain(NativeUpdateUncertainty),
    ObservationDeadline,
    ObservationCancelled,
    ObservationBudgetExhausted,
    CoordinatorInterrupted,
    ReconciliationInterrupted,
    ReconciliationIncomplete,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeUpdateCoordinatorPhase {
    Dispatching,
    Observing,
    Reconciling,
    Held { reason: NativeUpdateHoldReason },
    InstalledAwaitingVerification,
    Verified,
    Rejected { reason: NativeUpdateRefusal },
}

impl NativeUpdateCoordinatorPhase {
    fn is_terminal(&self) -> bool {
        matches!(self, Self::Verified | Self::Rejected { .. })
    }

    fn is_held(&self) -> bool {
        matches!(self, Self::Held { .. })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeUpdateCoordinatorStatus {
    pub operation_id: codeterm_types::native_update::NativeUpdateOperationId,
    pub captured_attachment: NativeUpdateAttachment,
    pub sequence: Option<u64>,
    pub phase: NativeUpdateCoordinatorPhase,
    pub last_result: Option<NativeUpdateResult>,
    pub worker_running: bool,
}

#[derive(Clone)]
pub struct NativeUpdateCoordinator {
    inner: Arc<Mutex<CoordinatorState>>,
}

#[derive(Default)]
struct CoordinatorState {
    active: Option<ActiveTransaction>,
}

struct ActiveTransaction {
    request: NativeUpdateRequest,
    state: UpdateStateHandle,
    executor: Arc<dyn NativeUpdateExecutor>,
    lease: Option<UpdateLease<'static>>,
    status: NativeUpdateCoordinatorStatus,
    status_tx: watch::Sender<NativeUpdateCoordinatorStatus>,
    cancel_observation: watch::Sender<bool>,
    worker_generation: u64,
}

impl NativeUpdateCoordinator {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(CoordinatorState::default())),
        }
    }

    pub fn start(
        &self,
        request: NativeUpdateRequest,
        ready: &ServerReady,
        installed: &ServerComponentManifest,
        state: UpdateStateHandle,
        executor: Option<Arc<dyn NativeUpdateExecutor>>,
    ) -> Result<codeterm_types::native_update::NativeUpdateOperationId, NativeUpdateRefusal> {
        request.validate()?;
        if !request.matches_current_server(ready, installed) {
            return Err(NativeUpdateRefusal::ExpectedServerMismatch);
        }
        ensure_supported_action(request.action)?;
        let executor = executor.ok_or(NativeUpdateRefusal::NativeHostUnavailable)?;
        let attachment = executor
            .captured_attachment()
            .ok_or(NativeUpdateRefusal::NativeHostUnavailable)?;
        attachment.validate_for(&request)?;

        if let Some(existing) = self.inner.lock().active.as_ref() {
            if existing.request.operation_id == request.operation_id {
                return if existing.request == request {
                    Ok(request.operation_id)
                } else {
                    Err(NativeUpdateRefusal::WrongOperation)
                };
            }
        }

        let lease = global()
            .try_acquire()
            .ok_or(NativeUpdateRefusal::AlreadyInFlight)?;
        let initial_status = NativeUpdateCoordinatorStatus {
            operation_id: request.operation_id,
            captured_attachment: attachment.clone(),
            sequence: None,
            phase: NativeUpdateCoordinatorPhase::Dispatching,
            last_result: None,
            worker_running: true,
        };
        let (status_tx, _) = watch::channel(initial_status.clone());
        let (cancel_observation, cancel_rx) = watch::channel(false);

        {
            let mut coordinator = self.inner.lock();
            if let Some(existing) = coordinator.active.as_ref() {
                if existing.request.operation_id == request.operation_id {
                    let same_request = existing.request == request;
                    drop(coordinator);
                    drop(lease);
                    return if same_request {
                        Ok(request.operation_id)
                    } else {
                        Err(NativeUpdateRefusal::WrongOperation)
                    };
                }
                if !existing.status.phase.is_terminal() {
                    drop(coordinator);
                    drop(lease);
                    return Err(NativeUpdateRefusal::AlreadyInFlight);
                }
            }
            coordinator.active = Some(ActiveTransaction {
                request: request.clone(),
                state: state.clone(),
                executor: executor.clone(),
                lease: Some(lease),
                status: initial_status,
                status_tx,
                cancel_observation,
                worker_generation: 1,
            });
        }

        let operation_id = request.operation_id;
        let inner = self.inner.clone();
        let guard = DriverGuard::new(inner.clone(), operation_id, 1);
        state.lock().clear_error();
        state.lock().set_phase(UpdatePhase::Applying);
        tokio::spawn(async move {
            let mut guard = guard;
            run_client_update(
                inner,
                request,
                executor,
                attachment,
                cancel_rx,
                operation_id,
                1,
            )
            .await;
            guard.disarm();
        });
        Ok(operation_id)
    }

    pub fn status(
        &self,
        operation_id: codeterm_types::native_update::NativeUpdateOperationId,
    ) -> Option<NativeUpdateCoordinatorStatus> {
        self.inner
            .lock()
            .active
            .as_ref()
            .filter(|active| active.request.operation_id == operation_id)
            .map(|active| active.status.clone())
    }

    pub fn subscribe(
        &self,
        operation_id: codeterm_types::native_update::NativeUpdateOperationId,
    ) -> Option<watch::Receiver<NativeUpdateCoordinatorStatus>> {
        self.inner
            .lock()
            .active
            .as_ref()
            .filter(|active| active.request.operation_id == operation_id)
            .map(|active| active.status_tx.subscribe())
    }

    pub fn cancel_observation(
        &self,
        operation_id: codeterm_types::native_update::NativeUpdateOperationId,
    ) -> bool {
        let cancellation = {
            let coordinator = self.inner.lock();
            let Some(active) = coordinator.active.as_ref() else {
                return false;
            };
            if active.request.operation_id != operation_id
                || !matches!(
                    &active.status.phase,
                    NativeUpdateCoordinatorPhase::Observing
                        | NativeUpdateCoordinatorPhase::InstalledAwaitingVerification
                )
            {
                return false;
            }
            active.cancel_observation.clone()
        };

        if hold_active(
            &self.inner,
            operation_id,
            None,
            NativeUpdateHoldReason::ObservationCancelled,
        )
        .is_none()
        {
            return false;
        }
        cancellation.send_replace(true);
        true
    }

    pub async fn reconcile(
        &self,
        operation_id: codeterm_types::native_update::NativeUpdateOperationId,
    ) -> Result<NativeUpdateCoordinatorStatus, NativeUpdateRefusal> {
        let (request, executor, after_sequence, generation) = {
            let mut coordinator = self.inner.lock();
            let Some(active) = coordinator.active.as_mut() else {
                return Err(NativeUpdateRefusal::WrongOperation);
            };
            if active.request.operation_id != operation_id {
                return Err(NativeUpdateRefusal::WrongOperation);
            }
            if !active.status.phase.is_held() {
                return Err(NativeUpdateRefusal::AlreadyInFlight);
            }
            if active.status.worker_running {
                return Err(NativeUpdateRefusal::AlreadyInFlight);
            }
            let generation = active
                .worker_generation
                .checked_add(1)
                .ok_or(NativeUpdateRefusal::StaleSequence)?;
            active.worker_generation = generation;
            active.status.phase = NativeUpdateCoordinatorPhase::Reconciling;
            active.status.worker_running = true;
            active.status_tx.send_replace(active.status.clone());
            (
                active.request.clone(),
                active.executor.clone(),
                active.status.sequence.unwrap_or(0),
                generation,
            )
        };

        let mut guard = ReconciliationGuard::new(self.inner.clone(), operation_id, generation);
        let observed = tokio::time::timeout(
            EVENT_DEADLINE,
            executor.observe(operation_id, after_sequence),
        )
        .await;
        let result = match observed {
            Ok(result) => record_event(
                &self.inner,
                operation_id,
                generation,
                &request,
                result,
                EventSource::Reconciliation,
            ),
            Err(_) => hold_active(
                &self.inner,
                operation_id,
                Some(generation),
                NativeUpdateHoldReason::ObservationDeadline,
            )
            .ok_or(NativeUpdateRefusal::AlreadyInFlight),
        };
        guard.disarm();
        match result {
            Ok(_) => self
                .status(operation_id)
                .ok_or(NativeUpdateRefusal::WrongOperation),
            Err(refusal) => Err(refusal),
        }
    }
}

impl Default for NativeUpdateCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy)]
enum EventSource {
    Dispatch,
    Observation,
    Reconciliation,
}

enum UpdateStatusEffect {
    None,
    Success,
    Error(String),
}

async fn run_client_update(
    inner: Arc<Mutex<CoordinatorState>>,
    request: NativeUpdateRequest,
    executor: Arc<dyn NativeUpdateExecutor>,
    attachment: NativeUpdateAttachment,
    mut cancel_rx: watch::Receiver<bool>,
    operation_id: codeterm_types::native_update::NativeUpdateOperationId,
    generation: u64,
) {
    let dispatched = tokio::time::timeout(
        EVENT_DEADLINE,
        executor.dispatch(request.clone(), attachment),
    )
    .await;
    let first = match dispatched {
        Ok(NativeUpdateDispatchOutcome::Reply(result)) => result,
        Ok(NativeUpdateDispatchOutcome::RefusedBeforeExecution { reason }) => {
            reject_before_dispatch(&inner, operation_id, generation, reason);
            return;
        }
        Ok(NativeUpdateDispatchOutcome::UncertainAfterExecution { reason }) => {
            hold_active(
                &inner,
                operation_id,
                Some(generation),
                NativeUpdateHoldReason::DispatchUncertain(reason),
            );
            return;
        }
        Err(_) => {
            hold_active(
                &inner,
                operation_id,
                Some(generation),
                NativeUpdateHoldReason::DispatchDeadline,
            );
            return;
        }
    };
    let first_status = match record_event(
        &inner,
        operation_id,
        generation,
        &request,
        first,
        EventSource::Dispatch,
    ) {
        Ok(status) => status,
        Err(_) => return,
    };
    if first_status.phase.is_terminal() {
        return;
    }

    let mut observations = 0;
    loop {
        if observations >= MAX_AUTOMATIC_OBSERVATIONS {
            hold_active(
                &inner,
                operation_id,
                Some(generation),
                NativeUpdateHoldReason::ObservationBudgetExhausted,
            );
            return;
        }
        let after_sequence = {
            let mut coordinator = inner.lock();
            let Some(active) = coordinator.active.as_mut() else {
                return;
            };
            if active.request.operation_id != operation_id
                || active.worker_generation != generation
                || !matches!(
                    &active.status.phase,
                    NativeUpdateCoordinatorPhase::Observing
                        | NativeUpdateCoordinatorPhase::InstalledAwaitingVerification
                )
            {
                return;
            }
            if !matches!(
                &active.status.phase,
                NativeUpdateCoordinatorPhase::InstalledAwaitingVerification
            ) {
                active.status.phase = NativeUpdateCoordinatorPhase::Observing;
                active.status_tx.send_replace(active.status.clone());
            }
            active.status.sequence.unwrap_or(0)
        };

        observations += 1;
        let observed = tokio::select! {
            changed = cancel_rx.changed() => {
                let _ = changed;
                hold_active(
                    &inner,
                    operation_id,
                    Some(generation),
                    NativeUpdateHoldReason::ObservationCancelled,
                );
                return;
            }
            result = tokio::time::timeout(
                EVENT_DEADLINE,
                executor.observe(operation_id, after_sequence),
            ) => result,
        };
        let result = match observed {
            Ok(result) => result,
            Err(_) => {
                hold_active(
                    &inner,
                    operation_id,
                    Some(generation),
                    NativeUpdateHoldReason::ObservationDeadline,
                );
                return;
            }
        };
        let status = match record_event(
            &inner,
            operation_id,
            generation,
            &request,
            result,
            EventSource::Observation,
        ) {
            Ok(status) => status,
            Err(_) => return,
        };
        if status.phase.is_terminal() {
            return;
        }
    }
}

fn record_event(
    inner: &Arc<Mutex<CoordinatorState>>,
    operation_id: codeterm_types::native_update::NativeUpdateOperationId,
    generation: u64,
    request: &NativeUpdateRequest,
    result: NativeUpdateResult,
    source: EventSource,
) -> Result<NativeUpdateCoordinatorStatus, NativeUpdateRefusal> {
    let (status, lease, state, effect, refusal) = {
        let mut coordinator = inner.lock();
        let Some(active) = coordinator.active.as_mut() else {
            return Err(NativeUpdateRefusal::WrongOperation);
        };
        if active.request.operation_id != operation_id
            || active.worker_generation != generation
            || &active.request != request
        {
            return Err(NativeUpdateRefusal::WrongOperation);
        }
        let expected_phase = match source {
            EventSource::Dispatch => matches!(
                &active.status.phase,
                NativeUpdateCoordinatorPhase::Dispatching
            ),
            EventSource::Observation => matches!(
                &active.status.phase,
                NativeUpdateCoordinatorPhase::Observing
                    | NativeUpdateCoordinatorPhase::InstalledAwaitingVerification
            ),
            EventSource::Reconciliation => matches!(
                &active.status.phase,
                NativeUpdateCoordinatorPhase::Reconciling
            ),
        };
        if !expected_phase {
            return Err(NativeUpdateRefusal::AlreadyInFlight);
        }

        let connection = &result.connection;
        let captured = &active.status.captured_attachment.connection;
        let connection_refusal = if connection.instance_id != captured.instance_id
            || connection.identity != captured.identity
            || connection.server_epoch != captured.server_epoch
            || connection.generation < captured.generation
            || (matches!(source, EventSource::Dispatch) && connection != captured)
        {
            Err(NativeUpdateRefusal::ExpectedServerMismatch)
        } else {
            Ok(())
        };
        if let Err(refusal) = result
            .validate_after(request, active.status.last_result.as_ref())
            .and(connection_refusal)
        {
            active.status.phase = NativeUpdateCoordinatorPhase::Held {
                reason: NativeUpdateHoldReason::InvalidReply(refusal),
            };
            active.status_tx.send_replace(active.status.clone());
            (
                active.status.clone(),
                None,
                active.state.clone(),
                UpdateStatusEffect::Error(format!("Native update held: {refusal:?}")),
                Some(refusal),
            )
        } else {
            active.status.sequence = Some(result.sequence);
            active.status.last_result = Some(result.clone());
            let (phase, effect, terminal) = match result.state {
                NativeUpdateState::Verified { .. } => (
                    NativeUpdateCoordinatorPhase::Verified,
                    UpdateStatusEffect::Success,
                    true,
                ),
                NativeUpdateState::Rejected { reason } => (
                    NativeUpdateCoordinatorPhase::Rejected { reason },
                    UpdateStatusEffect::Error(format!("Native update refused: {reason:?}")),
                    true,
                ),
                NativeUpdateState::Completed { .. }
                | NativeUpdateState::InstalledAwaitingVerification { .. } => {
                    if matches!(source, EventSource::Reconciliation) {
                        (
                            NativeUpdateCoordinatorPhase::Held {
                                reason: NativeUpdateHoldReason::ReconciliationIncomplete,
                            },
                            UpdateStatusEffect::Error(
                                "Native install is awaiting replacement verification".into(),
                            ),
                            false,
                        )
                    } else {
                        (
                            NativeUpdateCoordinatorPhase::InstalledAwaitingVerification,
                            UpdateStatusEffect::None,
                            false,
                        )
                    }
                }
                NativeUpdateState::Admitted
                | NativeUpdateState::InProgress { .. }
                | NativeUpdateState::Uncertain { .. } => match source {
                    EventSource::Reconciliation => (
                        NativeUpdateCoordinatorPhase::Held {
                            reason: NativeUpdateHoldReason::ReconciliationIncomplete,
                        },
                        UpdateStatusEffect::Error(
                            "Native update remains unverified after reconciliation".into(),
                        ),
                        false,
                    ),
                    EventSource::Dispatch | EventSource::Observation => (
                        NativeUpdateCoordinatorPhase::Observing,
                        UpdateStatusEffect::None,
                        false,
                    ),
                },
            };
            active.status.phase = phase;
            active.status_tx.send_replace(active.status.clone());
            let lease = if terminal { active.lease.take() } else { None };
            (
                active.status.clone(),
                lease,
                active.state.clone(),
                effect,
                None,
            )
        }
    };

    match effect {
        UpdateStatusEffect::None => {}
        UpdateStatusEffect::Success => {
            let mut update = state.lock();
            update.clear_error();
            update.set_phase(UpdatePhase::Success);
        }
        UpdateStatusEffect::Error(message) => state.lock().set_error(message),
    }
    drop(lease);
    match refusal {
        Some(refusal) => Err(refusal),
        None => Ok(status),
    }
}

fn reject_before_dispatch(
    inner: &Arc<Mutex<CoordinatorState>>,
    operation_id: codeterm_types::native_update::NativeUpdateOperationId,
    generation: u64,
    reason: NativeUpdateRefusal,
) {
    let (state, lease) = {
        let mut coordinator = inner.lock();
        let Some(active) = coordinator.active.as_mut() else {
            return;
        };
        if active.request.operation_id != operation_id
            || active.worker_generation != generation
            || !matches!(
                &active.status.phase,
                NativeUpdateCoordinatorPhase::Dispatching
            )
        {
            return;
        }
        active.status.phase = NativeUpdateCoordinatorPhase::Rejected { reason };
        active.status_tx.send_replace(active.status.clone());
        (active.state.clone(), active.lease.take())
    };
    state
        .lock()
        .set_error(format!("Native update refused before execution: {reason:?}"));
    drop(lease);
}

fn hold_active(
    inner: &Arc<Mutex<CoordinatorState>>,
    operation_id: codeterm_types::native_update::NativeUpdateOperationId,
    generation: Option<u64>,
    reason: NativeUpdateHoldReason,
) -> Option<NativeUpdateCoordinatorStatus> {
    let (status, state) = {
        let mut coordinator = inner.lock();
        let active = coordinator.active.as_mut()?;
        if active.request.operation_id != operation_id
            || generation.is_some_and(|generation| active.worker_generation != generation)
            || active.status.phase.is_terminal()
            || active.status.phase.is_held()
        {
            return None;
        }
        active.status.phase = NativeUpdateCoordinatorPhase::Held {
            reason: reason.clone(),
        };
        active.status_tx.send_replace(active.status.clone());
        (active.status.clone(), active.state.clone())
    };
    state
        .lock()
        .set_error(format!("Native update held: {reason:?}"));
    Some(status)
}

struct DriverGuard {
    inner: Arc<Mutex<CoordinatorState>>,
    operation_id: codeterm_types::native_update::NativeUpdateOperationId,
    generation: u64,
    armed: bool,
}

impl DriverGuard {
    fn new(
        inner: Arc<Mutex<CoordinatorState>>,
        operation_id: codeterm_types::native_update::NativeUpdateOperationId,
        generation: u64,
    ) -> Self {
        Self {
            inner,
            operation_id,
            generation,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        finish_worker(&self.inner, self.operation_id, self.generation);
        self.armed = false;
    }
}

impl Drop for DriverGuard {
    fn drop(&mut self) {
        if self.armed {
            hold_active(
                &self.inner,
                self.operation_id,
                Some(self.generation),
                NativeUpdateHoldReason::CoordinatorInterrupted,
            );
            finish_worker(&self.inner, self.operation_id, self.generation);
        }
    }
}

struct ReconciliationGuard {
    inner: Arc<Mutex<CoordinatorState>>,
    operation_id: codeterm_types::native_update::NativeUpdateOperationId,
    generation: u64,
    armed: bool,
}

impl ReconciliationGuard {
    fn new(
        inner: Arc<Mutex<CoordinatorState>>,
        operation_id: codeterm_types::native_update::NativeUpdateOperationId,
        generation: u64,
    ) -> Self {
        Self {
            inner,
            operation_id,
            generation,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        finish_worker(&self.inner, self.operation_id, self.generation);
        self.armed = false;
    }
}

impl Drop for ReconciliationGuard {
    fn drop(&mut self) {
        if self.armed {
            hold_active(
                &self.inner,
                self.operation_id,
                Some(self.generation),
                NativeUpdateHoldReason::ReconciliationInterrupted,
            );
            finish_worker(&self.inner, self.operation_id, self.generation);
        }
    }
}

fn finish_worker(
    inner: &Arc<Mutex<CoordinatorState>>,
    operation_id: codeterm_types::native_update::NativeUpdateOperationId,
    generation: u64,
) {
    let mut coordinator = inner.lock();
    let Some(active) = coordinator.active.as_mut() else {
        return;
    };
    if active.request.operation_id != operation_id || active.worker_generation != generation {
        return;
    }
    active.status.worker_running = false;
    active.status_tx.send_replace(active.status.clone());
}

fn ensure_supported_action(action: NativeUpdateAction) -> Result<(), NativeUpdateRefusal> {
    if action.requires_server_handoff() {
        Err(NativeUpdateRefusal::DurableHandoffUnavailable)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
