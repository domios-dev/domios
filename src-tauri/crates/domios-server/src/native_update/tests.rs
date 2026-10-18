use super::*;
use codeterm_types::{
    native_host::NativeHostConnection,
    native_update::{
        ExpectedServer, NativeUpdateAttachment, NativeUpdateDispatchOutcome,
        NativeUpdateInstallReceipt, NativeUpdateOperationId, NativeUpdateStage,
        NativeUpdateUncertainty, NativeUpdateVerification,
    },
    server_bootstrap::{
        ProcessProvenance, ServerExpectation, ServerIdentity, ServerReady, API_VERSION,
        BOOTSTRAP_PROTOCOL, SERVER_COMPONENT_SCHEMA,
    },
    update::channel::Channel,
};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
use tokio::sync::oneshot;
use uuid::Uuid;

fn global_gate_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner())
}

fn components(client_digest: &str) -> ServerComponentManifest {
    ServerComponentManifest {
        schema: SERVER_COMPONENT_SCHEMA,
        server_build: "server-old".into(),
        server_executable: PathBuf::from("C:/Domios/domios-server.exe"),
        server_sha256: "a".repeat(64),
        client_sha256: client_digest.into(),
        api_version: API_VERSION,
        bootstrap_protocol: BOOTSTRAP_PROTOCOL,
    }
}

fn request() -> NativeUpdateRequest {
    let current = components(&"b".repeat(64));
    let staged = components(&"c".repeat(64));
    let identity = ServerIdentity {
        data: PathBuf::from("C:/Domios/data"),
        config: PathBuf::from("C:/Domios/config"),
    };
    let process = ProcessProvenance {
        pid: 41,
        created: "start-41".into(),
        executable: current.server_executable.clone(),
    };
    NativeUpdateRequest {
        operation_id: NativeUpdateOperationId::new(Uuid::from_u128(17)).unwrap(),
        action: NativeUpdateAction::ClientOnly,
        expected_server: ExpectedServer {
            components: current,
            identity,
            incarnation: ServerExpectation {
                process,
                epoch: "epoch-old".into(),
            },
        },
        staged_components: staged,
        restart_authorized: false,
    }
}

fn ready_for(request: &NativeUpdateRequest) -> ServerReady {
    let expected = &request.expected_server;
    ServerReady {
        protocol: expected.components.bootstrap_protocol,
        api_version: expected.components.api_version,
        build: expected.components.server_build.clone(),
        instance_id: "instance".into(),
        identity: expected.identity.clone(),
        epoch: expected.incarnation.epoch.clone(),
        process: expected.incarnation.process.clone(),
        api_address: "127.0.0.1:7686".parse().expect("valid test address"),
        socket_address: "socket".into(),
        bootstrap_address: "bootstrap".into(),
        resources: PathBuf::from("C:/Domios/resources"),
    }
}

fn start(
    coordinator: &NativeUpdateCoordinator,
    request: &NativeUpdateRequest,
    state: UpdateStateHandle,
    executor: Option<Arc<dyn NativeUpdateExecutor>>,
) -> Result<NativeUpdateOperationId, NativeUpdateRefusal> {
    coordinator.start(
        request.clone(),
        &ready_for(request),
        &request.expected_server.components,
        state,
        executor,
    )
}

fn state_handle() -> UpdateStateHandle {
    Arc::new(Mutex::new(codeterm_service_core::update_state::UpdateState {
        install_method: codeterm_service_core::update_state::InstallMethod::Unknown,
        last_check: None,
        last_check_time: None,
        channel: Channel::Stable,
        phase: UpdatePhase::Idle,
        last_error: None,
        phase_changed_at: None,
        downloaded_bytes: 0,
        download_total_bytes: None,
    }))
}

fn result(
    request: &NativeUpdateRequest,
    operation_id: NativeUpdateOperationId,
    sequence: u64,
    generation: u64,
    state: NativeUpdateState,
) -> NativeUpdateResult {
    NativeUpdateResult {
        operation_id,
        sequence,
        connection: NativeHostConnection {
            instance_id: "instance".into(),
            identity: request.expected_server.identity.clone(),
            server_epoch: request.expected_server.incarnation.epoch.clone(),
            generation,
        },
        state,
    }
}

fn in_progress(request: &NativeUpdateRequest) -> NativeUpdateResult {
    result(
        request,
        request.operation_id,
        1,
        1,
        NativeUpdateState::InProgress {
            stage: NativeUpdateStage::Applying,
        },
    )
}

fn install_receipt(request: &NativeUpdateRequest) -> NativeUpdateInstallReceipt {
    NativeUpdateInstallReceipt {
        installed_components: request.staged_components.clone(),
        client_process: ProcessProvenance {
            pid: 72,
            created: "start-72".into(),
            executable: PathBuf::from("C:/Domios/domios.exe"),
        },
    }
}

fn completed(request: &NativeUpdateRequest) -> NativeUpdateResult {
    result(
        request,
        request.operation_id,
        2,
        2,
        NativeUpdateState::Completed {
            install: install_receipt(request),
        },
    )
}

fn verified(request: &NativeUpdateRequest) -> NativeUpdateResult {
    let install = install_receipt(request);
    result(
        request,
        request.operation_id,
        3,
        3,
        NativeUpdateState::Verified {
            verification: NativeUpdateVerification {
                client_process: install.client_process.clone(),
                server: ready_for(request),
            },
            install,
        },
    )
}

struct FakeExecutor {
    dispatch_result: NativeUpdateResult,
    dispatch_outcome: Option<NativeUpdateDispatchOutcome>,
    attachment: Option<NativeUpdateAttachment>,
    observations: Mutex<VecDeque<NativeUpdateResult>>,
    dispatch_count: AtomicUsize,
    observe_count: AtomicUsize,
    panic_first_observation: bool,
    pending_observation: bool,
    observation_started: Mutex<Option<oneshot::Sender<()>>>,
    observation_dropped: Mutex<Option<oneshot::Sender<()>>>,
}

impl FakeExecutor {
    fn new(
        dispatch_result: NativeUpdateResult,
        observations: Vec<NativeUpdateResult>,
    ) -> Self {
        let attachment = NativeUpdateAttachment {
            connection: dispatch_result.connection.clone(),
            client_process: ProcessProvenance {
                pid: 72,
                created: "start-72".into(),
                executable: PathBuf::from("C:/Domios/domios.exe"),
            },
        };
        Self {
            dispatch_result,
            dispatch_outcome: None,
            attachment: Some(attachment),
            observations: Mutex::new(observations.into()),
            dispatch_count: AtomicUsize::new(0),
            observe_count: AtomicUsize::new(0),
            panic_first_observation: false,
            pending_observation: false,
            observation_started: Mutex::new(None),
            observation_dropped: Mutex::new(None),
        }
    }

}

struct DropSignal(Option<oneshot::Sender<()>>);

impl Drop for DropSignal {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}

impl NativeUpdateExecutor for FakeExecutor {
    fn captured_attachment(&self) -> Option<NativeUpdateAttachment> {
        self.attachment.clone()
    }

    fn dispatch(
        &self,
        _request: NativeUpdateRequest,
        _attachment: NativeUpdateAttachment,
    ) -> codeterm_types::native_update::NativeUpdateDispatchFuture {
        self.dispatch_count.fetch_add(1, Ordering::SeqCst);
        let outcome = self
            .dispatch_outcome
            .clone()
            .unwrap_or_else(|| NativeUpdateDispatchOutcome::Reply(self.dispatch_result.clone()));
        Box::pin(async move { outcome })
    }

    fn observe(
        &self,
        _operation_id: NativeUpdateOperationId,
        _after_sequence: u64,
    ) -> NativeUpdateResultFuture {
        let observation = self.observe_count.fetch_add(1, Ordering::SeqCst) + 1;
        let should_panic = self.panic_first_observation && observation == 1;
        let result = if should_panic || self.pending_observation {
            None
        } else {
            self.observations.lock().pop_front()
        };
        let started = self.observation_started.lock().take();
        let dropped = self.observation_dropped.lock().take();
        let pending = self.pending_observation;
        Box::pin(async move {
            let _drop_signal = DropSignal(dropped);
            if let Some(sender) = started {
                let _ = sender.send(());
            }
            if should_panic {
                panic!("injected observation worker failure");
            }
            if pending {
                std::future::pending::<NativeUpdateResult>().await;
            }
            result.expect("the test supplied an observation result")
        })
    }
}

async fn wait_for_status(
    receiver: &mut watch::Receiver<NativeUpdateCoordinatorStatus>,
    predicate: impl Fn(&NativeUpdateCoordinatorStatus) -> bool,
) -> NativeUpdateCoordinatorStatus {
    loop {
        let current = receiver.borrow().clone();
        if predicate(&current) {
            return current;
        }
        receiver.changed().await.expect("coordinator status remains observable");
    }
}

#[test]
fn server_handoff_refuses_without_a_durable_continuation_owner() {
    assert_eq!(
        ensure_supported_action(NativeUpdateAction::Server),
        Err(NativeUpdateRefusal::DurableHandoffUnavailable)
    );
    assert_eq!(
        ensure_supported_action(NativeUpdateAction::Pair),
        Err(NativeUpdateRefusal::DurableHandoffUnavailable)
    );
    assert_eq!(
        ensure_supported_action(NativeUpdateAction::ClientOnly),
        Ok(())
    );
}

#[tokio::test(flavor = "current_thread")]
async fn invalid_immediate_observation_holds_once_without_retrying_cursor() {
    let _gate_lock = global_gate_test_lock();
    let request = request();
    let wrong = result(
        &request,
        NativeUpdateOperationId::new(Uuid::from_u128(18)).unwrap(),
        2,
        1,
        NativeUpdateState::InProgress {
            stage: NativeUpdateStage::Applying,
        },
    );
    let executor = Arc::new(FakeExecutor::new(
        in_progress(&request),
        vec![wrong],
    ));
    let coordinator = NativeUpdateCoordinator::new();
    let operation_id = start(&coordinator, &request, state_handle(), Some(executor.clone()))
        .expect("the global update lease is free");
    let mut status = coordinator.subscribe(operation_id).unwrap();
    let held = wait_for_status(&mut status, |status| {
        matches!(
            &status.phase,
            NativeUpdateCoordinatorPhase::Held {
                reason: NativeUpdateHoldReason::InvalidReply(
                    NativeUpdateRefusal::WrongOperation
                )
            }
        ) && !status.worker_running
    })
    .await;

    assert_eq!(held.sequence, Some(1));
    assert_eq!(executor.dispatch_count.load(Ordering::SeqCst), 1);
    assert_eq!(executor.observe_count.load(Ordering::SeqCst), 1);
    assert!(global().is_in_flight());
}

#[test]
fn missing_authenticated_attachment_refuses_before_admission_or_status_change() {
    let _gate_lock = global_gate_test_lock();
    let request = request();
    let state = state_handle();
    let coordinator = NativeUpdateCoordinator::new();
    assert_eq!(
        start(&coordinator, &request, state.clone(), None),
        Err(NativeUpdateRefusal::NativeHostUnavailable)
    );
    let mut executor = FakeExecutor::new(in_progress(&request), Vec::new());
    executor.attachment = None;

    assert_eq!(
        start(&coordinator, &request, state.clone(), Some(Arc::new(executor))),
        Err(NativeUpdateRefusal::NativeHostUnavailable)
    );
    assert_eq!(state.lock().phase, UpdatePhase::Idle);
    assert!(coordinator.status(request.operation_id).is_none());
    assert!(!global().is_in_flight());
    assert!(global().try_acquire().is_some());
}

#[tokio::test(flavor = "current_thread")]
async fn pre_dispatch_detach_refuses_without_fabricating_an_event() {
    let _gate_lock = global_gate_test_lock();
    let request = request();
    let mut executor = FakeExecutor::new(in_progress(&request), Vec::new());
    executor.dispatch_outcome = Some(NativeUpdateDispatchOutcome::RefusedBeforeExecution {
            reason: NativeUpdateRefusal::NativeHostUnavailable,
        });
    let executor = Arc::new(executor);
    let coordinator = NativeUpdateCoordinator::new();
    start(&coordinator, &request, state_handle(), Some(executor.clone()))
        .expect("the active attachment admits one server-owned operation");
    let mut status = coordinator.subscribe(request.operation_id).unwrap();
    let refused = wait_for_status(&mut status, |status| {
        matches!(&status.phase, NativeUpdateCoordinatorPhase::Rejected {
            reason: NativeUpdateRefusal::NativeHostUnavailable
        }) && !status.worker_running
    }).await;

    assert_eq!(refused.sequence, None);
    assert!(refused.last_result.is_none());
    assert_eq!(refused.captured_attachment, executor.captured_attachment().unwrap());
    assert_eq!(executor.dispatch_count.load(Ordering::SeqCst), 1);
    assert_eq!(executor.observe_count.load(Ordering::SeqCst), 0);
    assert!(!global().is_in_flight());
}

#[tokio::test(flavor = "current_thread")]
async fn uncertain_dispatch_keeps_the_captured_attachment_and_shared_lease() {
    let _gate_lock = global_gate_test_lock();
    let request = request();
    let mut executor = FakeExecutor::new(in_progress(&request), Vec::new());
    executor.dispatch_outcome = Some(NativeUpdateDispatchOutcome::UncertainAfterExecution {
            reason: NativeUpdateUncertainty::DisconnectedAfterDispatch,
        });
    let executor = Arc::new(executor);
    let captured = executor.captured_attachment().unwrap();
    let coordinator = NativeUpdateCoordinator::new();
    start(&coordinator, &request, state_handle(), Some(executor.clone()))
        .expect("the active attachment admits one server-owned operation");
    let mut status = coordinator.subscribe(request.operation_id).unwrap();
    let held = wait_for_status(&mut status, |status| {
        matches!(&status.phase, NativeUpdateCoordinatorPhase::Held {
            reason: NativeUpdateHoldReason::DispatchUncertain(
                NativeUpdateUncertainty::DisconnectedAfterDispatch
            )
        }) && !status.worker_running
    }).await;

    assert_eq!(held.sequence, None);
    assert!(held.last_result.is_none());
    assert_eq!(held.captured_attachment, captured);
    assert_eq!(executor.dispatch_count.load(Ordering::SeqCst), 1);
    assert_eq!(executor.observe_count.load(Ordering::SeqCst), 0);
    assert!(global().is_in_flight());
}

#[tokio::test(flavor = "current_thread")]
async fn caller_disconnect_and_observation_cancel_keep_uncertain_lease_reachable() {
    let _gate_lock = global_gate_test_lock();
    let request = request();
    let uncertain = result(
        &request,
        request.operation_id,
        1,
        1,
        NativeUpdateState::Uncertain {
            reason: NativeUpdateUncertainty::DisconnectedAfterDispatch,
        },
    );
    let (started_tx, started_rx) = oneshot::channel();
    let (dropped_tx, dropped_rx) = oneshot::channel();
    let mut executor = FakeExecutor::new(uncertain, Vec::new());
    executor.pending_observation = true;
    *executor.observation_started.lock() = Some(started_tx);
    *executor.observation_dropped.lock() = Some(dropped_tx);
    let executor = Arc::new(executor);
    let coordinator = NativeUpdateCoordinator::new();
    let caller = coordinator.clone();
    let state = state_handle();
    state.lock().set_phase(UpdatePhase::Applying);
    assert!(!global().is_in_flight());
    let operation_id = start(&caller, &request, state, Some(executor.clone()))
        .expect("the global update lease is free");
    drop(caller);
    let mut status = coordinator.subscribe(operation_id).unwrap();
    started_rx.await.expect("the server began a read-only observation");
    assert!(coordinator.cancel_observation(
        request.operation_id
    ));
    let held = wait_for_status(&mut status, |status| {
        matches!(
            &status.phase,
            NativeUpdateCoordinatorPhase::Held {
                reason: NativeUpdateHoldReason::ObservationCancelled
            }
        ) && !status.worker_running
    })
    .await;
    dropped_rx.await.expect("cancellation dropped the pending observation");

    assert!(matches!(
        held.last_result.as_ref().map(|result| &result.state),
        Some(NativeUpdateState::Uncertain { .. })
    ));
    assert_eq!(executor.dispatch_count.load(Ordering::SeqCst), 1);
    assert_eq!(executor.observe_count.load(Ordering::SeqCst), 1);
    assert!(global().is_in_flight());
    assert!(global().try_acquire().is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn interrupted_coordinator_reconciles_exact_completion_without_redispatch() {
    let _gate_lock = global_gate_test_lock();
    let request = request();
    let mut executor = FakeExecutor::new(in_progress(&request), vec![verified(&request)]);
    executor.panic_first_observation = true;
    let executor = Arc::new(executor);
    let coordinator = NativeUpdateCoordinator::new();
    let operation_id = start(&coordinator, &request, state_handle(), Some(executor.clone()))
        .expect("the global update lease is free");
    let mut status = coordinator.subscribe(operation_id).unwrap();
    let held = wait_for_status(&mut status, |status| {
        matches!(
            &status.phase,
            NativeUpdateCoordinatorPhase::Held {
                reason: NativeUpdateHoldReason::CoordinatorInterrupted
            }
        ) && !status.worker_running
    })
    .await;
    assert_eq!(held.sequence, Some(1));
    assert!(global().is_in_flight());

    let verified = coordinator
        .reconcile(operation_id)
        .await
        .expect("the exact later completion is authoritative");

    assert_eq!(verified.phase, NativeUpdateCoordinatorPhase::Verified);
    assert_eq!(verified.sequence, Some(3));
    assert_eq!(executor.dispatch_count.load(Ordering::SeqCst), 1);
    assert_eq!(executor.observe_count.load(Ordering::SeqCst), 2);
    assert!(!global().is_in_flight());
}

#[tokio::test(flavor = "current_thread")]
async fn completed_reconciliation_stays_reachable_until_verified_without_redispatch() {
    let _gate_lock = global_gate_test_lock();
    let request = request();
    let mut executor = FakeExecutor::new(
        in_progress(&request),
        vec![completed(&request), verified(&request)],
    );
    executor.panic_first_observation = true;
    let executor = Arc::new(executor);
    let coordinator = NativeUpdateCoordinator::new();
    let operation_id = start(&coordinator, &request, state_handle(), Some(executor.clone()))
        .expect("the global update lease is free");
    let mut status = coordinator.subscribe(operation_id).unwrap();
    wait_for_status(&mut status, |status| {
        matches!(
            &status.phase,
            NativeUpdateCoordinatorPhase::Held {
                reason: NativeUpdateHoldReason::CoordinatorInterrupted
            }
        ) && !status.worker_running
    })
    .await;

    let installed = coordinator
        .reconcile(operation_id)
        .await
        .expect("the broker installation receipt is exact");
    assert!(matches!(
        &installed.phase,
        NativeUpdateCoordinatorPhase::Held {
            reason: NativeUpdateHoldReason::ReconciliationIncomplete
        }
    ));
    assert_eq!(installed.sequence, Some(2));
    assert!(global().is_in_flight());

    let verified = coordinator
        .reconcile(operation_id)
        .await
        .expect("replacement verification is exact");
    assert_eq!(verified.phase, NativeUpdateCoordinatorPhase::Verified);
    assert_eq!(verified.sequence, Some(3));
    assert_eq!(executor.dispatch_count.load(Ordering::SeqCst), 1);
    assert_eq!(executor.observe_count.load(Ordering::SeqCst), 3);
    assert!(!global().is_in_flight());
}

#[test]
fn existing_http_lease_refuses_native_admission_without_status_mutation() {
    let _gate_lock = global_gate_test_lock();
    let lease = global().try_acquire().expect("the shared gate is idle");
    let request = request();
    let executor = Arc::new(FakeExecutor::new(in_progress(&request), Vec::new()));
    let state = state_handle();
    let coordinator = NativeUpdateCoordinator::new();

    assert_eq!(
        start(&coordinator, &request, state.clone(), Some(executor.clone())),
        Err(NativeUpdateRefusal::AlreadyInFlight)
    );
    assert_eq!(state.lock().phase, UpdatePhase::Idle);
    assert_eq!(executor.dispatch_count.load(Ordering::SeqCst), 0);
    assert!(global().is_in_flight());
    drop(lease);
    assert!(!global().is_in_flight());
}
