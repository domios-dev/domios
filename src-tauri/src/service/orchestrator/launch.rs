use crate::{
    service::orchestrator::types::SpawnAgentParams,
    service::{boot_stage::BootStage, ServiceContext},
};
pub(crate) fn skip_local_worker_role(
    role: &crate::multiplexer::types::AgentRole,
    parent_pane_id: Option<&str>,
    report_target_mesh_peer: Option<&str>,
) -> bool {
    matches!(role, crate::multiplexer::types::AgentRole::Worker)
        && parent_pane_id.is_none()
        && report_target_mesh_peer.map_or(false, |p| !p.is_empty())
}

pub(crate) fn role_onboarding_task(
    task: Option<&str>,
    role: Option<&str>,
    role_profile: Option<&str>,
) -> Option<String> {
    match (role, role_profile) {
        (Some("Manager"), Some("planner")) => {
            task.map(|t| format!("{}{}", t, crate::prompts::planner_suffix()))
        }
        (Some("Manager"), _) => task.map(str::to_string),
        (Some("Worker"), _) => task.map(|t| format!("{}{}", t, crate::prompts::worker_suffix())),
        (Some("Orchestrator"), _) => task.map(str::to_string),
        _ => task.map(str::to_string),
    }
}

pub(crate) fn spawn_onboarding_task(params: &SpawnAgentParams) -> Option<String> {
    role_onboarding_task(
        params.task.as_deref(),
        params.role.as_deref(),
        params.role_profile.as_deref(),
    )
}

pub(crate) fn should_deliver_spawn_preferences(
    is_planner: bool,
    onboarding_task: Option<&str>,
) -> bool {
    is_planner && onboarding_task.is_some()
}

pub(crate) use crate::agent::{provider::AgentLaunchParams, registry::AgentRegistry};
#[derive(Default)]
struct LaunchRegistry {
    next_generation: u64,
    panes: std::collections::HashMap<String, RegisteredLaunch>,
}

#[derive(Clone, Debug)]
struct RegisteredLaunch {
    pty_id: String,
    generation: u64,
    provider_ack: std::sync::Arc<ProviderAck>,
}

#[derive(Default)]
struct ProviderAck {
    ready: parking_lot::Mutex<bool>,
    changed: parking_lot::Condvar,
}

impl LaunchRegistry {
    fn begin(&mut self, pane_id: &str, pty_id: String) -> LaunchGeneration {
        self.next_generation = self.next_generation.wrapping_add(1).max(1);
        let launch = LaunchGeneration {
            pane_id: pane_id.into(),
            pty_id,
            generation: self.next_generation,
            stage: BootStage::PtySpawned,
            reused: false,
            provider_ack: std::sync::Arc::new(ProviderAck::default()),
        };
        self.panes.insert(
            launch.pane_id.clone(),
            RegisteredLaunch {
                pty_id: launch.pty_id.clone(),
                generation: launch.generation,
                provider_ack: launch.provider_ack.clone(),
            },
        );
        launch
    }

    fn owns(&self, launch: &LaunchGeneration) -> bool {
        self.panes.get(&launch.pane_id).is_some_and(|registered| {
            registered.pty_id == launch.pty_id && registered.generation == launch.generation
        })
    }

    fn finish(&mut self, launch: &LaunchGeneration) -> bool {
        if !self.owns(launch) {
            return false;
        }
        self.panes.remove(&launch.pane_id);
        true
    }

    fn take_for_pty(&mut self, pty_id: &str) -> Option<LaunchGeneration> {
        let pane_id = self
            .panes
            .iter()
            .find(|(_, registered)| registered.pty_id == pty_id)
            .map(|(pane_id, _)| pane_id.clone())?;
        let registered = self.panes.remove(&pane_id)?;
        Some(LaunchGeneration {
            pane_id,
            pty_id: registered.pty_id,
            generation: registered.generation,
            stage: BootStage::PtySpawned,
            reused: false,
            provider_ack: registered.provider_ack,
        })
    }
}

static LAUNCH_REGISTRY: std::sync::LazyLock<parking_lot::Mutex<LaunchRegistry>> =
    std::sync::LazyLock::new(|| parking_lot::Mutex::new(LaunchRegistry::default()));
#[derive(Debug, Clone, PartialEq, Eq)]
enum LaunchFailure {
    PtySpawn(String),
    ReaderSetup(String),
    ReaderRuntime(String),
    ReaderReadyTimeout,
    ShellReadyTimeout,
    PtyEof,
    PaneClosed,
    LaunchWrite(String),
    ProviderReadyTimeout,
    TaskDelivery(String),
}
impl LaunchFailure {
    fn reason(&self) -> String {
        match self {
            Self::PtySpawn(error) => format!("PtySpawnFailed: {error}"),
            Self::ReaderSetup(error) => format!("ReaderSetupFailed: {error}"),
            Self::ReaderRuntime(error) => format!("ReaderRuntimeFailed: {error}"),
            Self::ReaderReadyTimeout => "ReaderReadyTimeout".into(),
            Self::ShellReadyTimeout => "ShellReadyTimeout".into(),
            Self::PtyEof => "PtyEof".into(),
            Self::PaneClosed => "PaneClosedBeforePtyLink".into(),
            Self::LaunchWrite(error) => format!("LaunchWriteFailed: {error}"),
            Self::ProviderReadyTimeout => "ProviderReadyTimeout".into(),
            Self::TaskDelivery(error) => format!("TaskDeliveryFailed: {error}"),
        }
    }
}

#[derive(Debug, Clone)]
struct LaunchGeneration {
    pane_id: String,
    pty_id: String,
    generation: u64,
    stage: BootStage,
    reused: bool,
    provider_ack: std::sync::Arc<ProviderAck>,
}

pub(crate) fn record_provider_session(svc: &ServiceContext, pane_id: &str) {
    let pty_id = svc
        .mux
        .read()
        .get_pane(pane_id)
        .and_then(|pane| pane.pty_id.clone());
    let Some(pty_id) = pty_id else {
        return;
    };
    let ack = LAUNCH_REGISTRY
        .lock()
        .panes
        .get(pane_id)
        .filter(|launch| launch.pty_id == pty_id)
        .map(|launch| launch.provider_ack.clone());
    if let Some(ack) = ack {
        *ack.ready.lock() = true;
        ack.changed.notify_all();
    }
}
impl LaunchGeneration {
    fn begin(pane_id: &str, pty_id: String) -> Self {
        LAUNCH_REGISTRY.lock().begin(pane_id, pty_id)
    }

    fn owns(&self) -> bool {
        LAUNCH_REGISTRY.lock().owns(self)
    }

    fn transition(&mut self, stage: BootStage) {
        self.stage = stage;
    }

    fn reuse(&mut self) {
        self.reused = true;
        self.transition(BootStage::AgentReady);
    }

    fn replace(&mut self, pty_id: String) {
        let pane_id = self.pane_id.clone();
        *self = Self::begin(&pane_id, pty_id);
    }

    fn emit(&mut self, svc: &ServiceContext, stage: BootStage) -> bool {
        self.transition(stage);
        self.commit_if_current(|| {
            svc.emit_boot_stage(&self.pane_id, stage, None);
        })
    }

    fn fail(&self, svc: &ServiceContext, failure: LaunchFailure) -> bool {
        let reason = failure.reason();
        self.finish_if_current(|| {
            svc.emit_boot_stage(&self.pane_id, BootStage::SpawnFailed, Some(&reason));
        })
    }

    fn write_if_current<T>(&self, write: impl FnOnce() -> T) -> Option<T> {
        let registry = LAUNCH_REGISTRY.lock();
        registry.owns(self).then(write)
    }

    fn await_provider_ack(&self, timeout: std::time::Duration) -> bool {
        let mut ready = self.provider_ack.ready.lock();
        if *ready {
            return true;
        }
        self.provider_ack.changed.wait_for(&mut ready, timeout);
        *ready
    }

    fn commit_if_current(&self, commit: impl FnOnce()) -> bool {
        let registry = LAUNCH_REGISTRY.lock();
        if !registry.owns(self) {
            return false;
        }
        commit();
        true
    }

    fn finish_if_current(&self, commit: impl FnOnce()) -> bool {
        let mut registry = LAUNCH_REGISTRY.lock();
        if !registry.finish(self) {
            return false;
        }
        commit();
        true
    }
}

pub(crate) fn record_reader_termination(svc: &ServiceContext, pty_id: &str) {
    take_reader_termination(pty_id, |launch| {
        let reason = LaunchFailure::PtyEof.reason();
        svc.emit_boot_stage(&launch.pane_id, BootStage::SpawnFailed, Some(&reason));
    });
}

fn take_reader_termination(pty_id: &str, commit: impl FnOnce(LaunchGeneration)) -> bool {
    let mut registry = LAUNCH_REGISTRY.lock();
    let launch = registry.take_for_pty(pty_id);
    if let Some(launch) = launch {
        commit(launch);
        true
    } else {
        false
    }
}

enum ReaderReadiness {
    Ready,
    Pending {
        startup: std::sync::mpsc::Receiver<Result<(), LaunchFailure>>,
        closed: std::sync::mpsc::Receiver<()>,
    },
}

enum ReaderMonitor {
    Active(std::sync::mpsc::Receiver<()>),
    Unavailable,
}

impl ReaderMonitor {
    fn closed(&self) -> bool {
        match self {
            Self::Active(rx) => !matches!(rx.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty)),
            Self::Unavailable => false,
        }
    }
}

impl ReaderReadiness {
    fn wait(self, timeout: std::time::Duration) -> Result<ReaderMonitor, LaunchFailure> {
        match self {
            Self::Ready => Ok(ReaderMonitor::Unavailable),
            Self::Pending { startup, closed } => match startup.recv_timeout(timeout) {
                Ok(Ok(())) => Ok(ReaderMonitor::Active(closed)),
                Ok(Err(error)) => Err(error),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    Err(LaunchFailure::ReaderReadyTimeout)
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(
                    LaunchFailure::ReaderRuntime("reader startup channel closed".into()),
                ),
            },
        }
    }
}

struct BackendPty {
    id: String,
    reader: ReaderReadiness,
}

fn existing_live_pty(svc: &ServiceContext, pane_id: &str) -> Option<String> {
    let pty_id = svc
        .mux
        .read()
        .get_pane(pane_id)
        .and_then(|pane| pane.pty_id.clone())?;
    svc.pty
        .lock()
        .get_session(&pty_id)
        .is_some()
        .then_some(pty_id)
}

fn reused_provider_tui_owns_launch(
    reused_live_pty: bool,
    pane_agent_type: Option<&str>,
    requested_agent: &str,
    provider_tui_visible: bool,
) -> bool {
    reused_live_pty && pane_agent_type == Some(requested_agent) && provider_tui_visible
}

fn start_backend_reader(
    svc: &ServiceContext,
    pty_id: &str,
) -> Result<ReaderReadiness, LaunchFailure> {
    let svc_arc = std::sync::Arc::new(svc.clone());
    let prepared = crate::service::pty_lifecycle::prepare_backend_reader(&svc_arc, pty_id)
        .map_err(LaunchFailure::ReaderSetup)?;

    let Some(prepared) = prepared else {
        return Ok(ReaderReadiness::Ready);
    };
    let reader_pty_id = pty_id.to_string();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
    let (closed_tx, closed_rx) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => {
                let _ = ready_tx.send(Err(LaunchFailure::ReaderRuntime(error.to_string())));
                return;
            }
        };
        runtime.block_on(async {
            let reader = crate::service::pty_lifecycle::spawn_prepared_backend_reader(
                &svc_arc,
                &reader_pty_id,
                prepared,
                true,
            );
            let _ = ready_tx.send(Ok(()));
            let _ = reader.await;
            let _ = closed_tx.send(());
        });
    });
    Ok(ReaderReadiness::Pending {
        startup: ready_rx,
        closed: closed_rx,
    })
}

fn spawn_backend_pty(svc: &ServiceContext, pane_id: &str) -> Result<BackendPty, LaunchFailure> {
    let cwd = svc
        .mux
        .read()
        .get_pane(pane_id)
        .and_then(|pane| pane.cwd.clone());
    let env_vars = svc.pane_env(pane_id);
    let (pty_id, _) = svc
        .pty
        .lock()
        .spawn(None, cwd, 120, 40, env_vars)
        .map_err(|error| LaunchFailure::PtySpawn(error.to_string()))?;
    svc.screen_state.lock().resize(&pty_id, 120, 40);
    let reader = start_backend_reader(svc, &pty_id)?;

    Ok(BackendPty { id: pty_id, reader })
}

fn replacement_pty_candidate(
    linked_pty_id: Option<&str>,
    closed_pty_id: &str,
    session_exists: bool,
) -> Option<String> {
    linked_pty_id
        .filter(|candidate| *candidate != closed_pty_id && session_exists)
        .map(str::to_string)
}

fn await_replacement_pty(
    svc: &ServiceContext,
    pane_id: &str,
    closed_pty_id: &str,
    timeout: std::time::Duration,
) -> Option<String> {
    #[cfg(not(windows))]
    {
        let _ = (svc, pane_id, closed_pty_id, timeout);
        None
    }
    #[cfg(windows)]
    {
        let deadline = std::time::Instant::now() + timeout;
        super::pane_relink_signal::wait_for_relink(deadline, || {
            let linked = svc
                .mux
                .read()
                .get_pane(pane_id)
                .and_then(|pane| pane.pty_id.clone());
            let session_exists = linked
                .as_deref()
                .is_some_and(|pty_id| svc.pty.lock().get_session(pty_id).is_some());
            replacement_pty_candidate(linked.as_deref(), closed_pty_id, session_exists)
        })
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn launch_agent_in_background(
    svc: ServiceContext,
    pane_id: String,
    tab_id: String,
    ws_id: String,
    launch_cmd: String,
    agent_cmd: String,
    task: Option<String>,
    fallback_spec: Option<crate::service::launch_delivery::FallbackDeliverySpec>,
    ready_tx: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
) {
    let registry = svc.agent_registry.clone();
    let launch_started = std::time::Instant::now();
    // thread spawns: callers get the pane id immediately and may send right away.
    if let Some(p) = svc.mux.write().get_pane_mut(&pane_id) {
        p.agent_booting = true;
    }
    std::thread::spawn(move || {
        let mut delivery_gate = crate::service::launch_delivery::LaunchDeliveryGate::new();
        let mut launch = LaunchGeneration::begin(&pane_id, "pending".into());
        if !launch.emit(&svc, BootStage::PtyStarting) {
            return;
        }
        let set_booting = |value: bool| {
            if let Some(p) = svc.mux.write().get_pane_mut(&pane_id) {
                p.agent_booting = value;
            }
        };
        let pty_claim = svc.pty_ownership.acquire(&pane_id);
        let (backend_pty, reused_live_pty) = match existing_live_pty(&svc, &pane_id) {
            Some(pty_id) => match start_backend_reader(&svc, &pty_id) {
                Ok(reader) => (BackendPty { id: pty_id, reader }, true),
                Err(failure) => {
                    let error = failure.reason();
                    launch.fail(&svc, failure);
                    set_booting(false);
                    if let Some(tx) = ready_tx {
                        let _ = tx.send(Err(error));
                    }
                    return;
                }
            },
            None => match spawn_backend_pty(&svc, &pane_id) {
                Ok(backend_pty) => (backend_pty, false),
                Err(failure) => {
                    let error = failure.reason();
                    log::error!(
                        "launch_agent: backend PTY spawn failed for pane {}: {}",
                        pane_id,
                        error
                    );
                    launch.fail(&svc, failure);
                    let _ = svc.close_tab(&ws_id, &tab_id);
                    set_booting(false);
                    if let Some(tx) = ready_tx {
                        let _ = tx.send(Err(format!("backend PTY spawn failed: {}", error)));
                    }
                    return;
                }
            },
        };
        let BackendPty {
            id: candidate_pty_id,
            reader: reader_readiness,
        } = backend_pty;
        let selection = svc.link_pty_with_claim(&pty_claim, &pane_id, candidate_pty_id, true);
        drop(pty_claim);
        if !selection.linked {
            let error = LaunchFailure::PaneClosed.reason();
            launch.fail(&svc, LaunchFailure::PaneClosed);
            set_booting(false);
            let _ = svc.close_tab(&ws_id, &tab_id);
            if let Some(tx) = ready_tx {
                let _ = tx.send(Err(error));
            }
            return;
        }
        let mut pty_id = selection.pty_id;
        launch.replace(pty_id.clone());
        if !launch.emit(&svc, BootStage::PtySpawned) {
            return;
        }

        let mut reader_monitor = match reader_readiness.wait(std::time::Duration::from_secs(10)) {
            Ok(monitor) => monitor,
            Err(failure) => {
                let error = failure.reason();
                launch.fail(&svc, failure);
                set_booting(false);
                if let Some(tx) = ready_tx {
                    let _ = tx.send(Err(error));
                }
                return;
            }
        };
        if !launch.emit(&svc, BootStage::ReaderReady) {
            return;
        }

        let (pane_agent_type, screen) = {
            let pane_agent_type = svc
                .mux
                .read()
                .get_pane(&pane_id)
                .and_then(|pane| pane.agent_type.clone());
            let screen = svc
                .screen_state
                .lock()
                .get_screen_text(&pty_id)
                .unwrap_or_default();
            (pane_agent_type, screen)
        };
        let provider_tui_visible = registry
            .with_provider(&agent_cmd, |provider| provider.screen_has_tui(&screen))
            .unwrap_or(false);
        if reused_provider_tui_owns_launch(
            reused_live_pty,
            pane_agent_type.as_deref(),
            &agent_cmd,
            provider_tui_visible,
        ) {
            log::info!(
                "launch_agent: pane {} already owns live {} TUI on PTY {}; preserving ready state",
                pane_id,
                agent_cmd,
                pty_id
            );
            launch.reuse();
            launch.emit(&svc, launch.stage);
            LAUNCH_REGISTRY.lock().finish(&launch);
            set_booting(false);
            if let Some(tx) = ready_tx {
                let _ = tx.send(Ok(()));
            }
            return;
        }

        if !crate::service::pty_io::await_shell_prompt(
            &svc.screen_state,
            &pty_id,
            std::time::Duration::from_secs(30),
        ) {
            let error = LaunchFailure::ShellReadyTimeout.reason();
            launch.fail(&svc, LaunchFailure::ShellReadyTimeout);
            set_booting(false);
            if let Some(tx) = ready_tx {
                let _ = tx.send(Err(error));
            }
            return;
        }
        if !launch.emit(&svc, BootStage::ShellReady) {
            return;
        }
        delivery_gate.mark_shell_ready();

        let delivery = registry
            .with_provider(&agent_cmd, |provider| {
                crate::service::launch_delivery::mode_for_descriptor(
                    &provider.system_prompt_delivery(),
                )
            })
            .unwrap_or(crate::service::launch_delivery::LaunchDeliveryMode::ConfirmedFallback);
        if !launch.emit(&svc, BootStage::AgentLaunching) {
            return;
        }
        if reader_monitor.closed() {
            let error = LaunchFailure::PtyEof.reason();
            launch.fail(&svc, LaunchFailure::PtyEof);
            set_booting(false);
            if let Some(tx) = ready_tx {
                let _ = tx.send(Err(error));
            }
            return;
        }
        // (#93 — keeps shell-level launch off the bracketed-paste path).
        let first_launch = match launch
            .write_if_current(|| svc.send_shell_spawn_after_ready(&pty_id, &launch_cmd))
        {
            Some(result) => result,
            None => return,
        };
        let launch_result = match first_launch {
            Err(error) => {
                match await_replacement_pty(
                    &svc,
                    &pane_id,
                    &pty_id,
                    std::time::Duration::from_secs(10),
                ) {
                    Some(replacement) => {
                        log::warn!(
                            "launch_agent: retrying launch for pane {} on replacement pty {} after pty {} closed",
                            pane_id, replacement, pty_id
                        );
                        pty_id = replacement;
                        let replacement_reader = match start_backend_reader(&svc, &pty_id) {
                            Ok(reader) => reader,
                            Err(failure) => {
                                let error = failure.reason();
                                launch.fail(&svc, failure);
                                set_booting(false);
                                if let Some(tx) = ready_tx {
                                    let _ = tx.send(Err(error));
                                }
                                return;
                            }
                        };
                        reader_monitor =
                            match replacement_reader.wait(std::time::Duration::from_secs(10)) {
                                Ok(monitor) => monitor,
                                Err(failure) => {
                                    let error = failure.reason();
                                    launch.fail(&svc, failure);
                                    set_booting(false);
                                    if let Some(tx) = ready_tx {
                                        let _ = tx.send(Err(error));
                                    }
                                    return;
                                }
                            };
                        launch.replace(pty_id.clone());
                        if !launch.emit(&svc, BootStage::PtySpawned)
                            || !launch.emit(&svc, BootStage::ReaderReady)
                            || !crate::service::pty_io::await_shell_prompt(
                                &svc.screen_state,
                                &pty_id,
                                std::time::Duration::from_secs(30),
                            )
                        {
                            let error = LaunchFailure::ShellReadyTimeout.reason();
                            launch.fail(&svc, LaunchFailure::ShellReadyTimeout);
                            set_booting(false);
                            if let Some(tx) = ready_tx {
                                let _ = tx.send(Err(error));
                            }
                            return;
                        }
                        if !launch.emit(&svc, BootStage::ShellReady)
                            || !launch.emit(&svc, BootStage::AgentLaunching)
                        {
                            return;
                        }
                        match launch.write_if_current(|| {
                            svc.send_shell_spawn_after_ready(&pty_id, &launch_cmd)
                        }) {
                            Some(result) => result,
                            None => return,
                        }
                    }
                    None => Err(error),
                }
            }
            result => result,
        };
        if let Err(e) = launch_result {
            log::error!(
                "launch_agent: launch_cmd failed for pane {} (pty {}): {}",
                pane_id,
                pty_id,
                e
            );
            let failure = LaunchFailure::LaunchWrite(e.to_string());
            let error = failure.reason();
            launch.fail(&svc, failure);
            set_booting(false);
            if let Some(tx) = ready_tx {
                let _ = tx.send(Err(error));
            }
            return;
        }
        if !launch.emit(&svc, BootStage::LaunchWritten) {
            return;
        }

        if !launch.await_provider_ack(std::time::Duration::from_secs(30)) {
            let error = if reader_monitor.closed() {
                LaunchFailure::PtyEof.reason()
            } else {
                LaunchFailure::ProviderReadyTimeout.reason()
            };
            launch.fail(
                &svc,
                if reader_monitor.closed() {
                    LaunchFailure::PtyEof
                } else {
                    LaunchFailure::ProviderReadyTimeout
                },
            );
            set_booting(false);
            if let Some(tx) = ready_tx {
                let _ = tx.send(Err(error));
            }
            return;
        }
        if !launch.emit(&svc, BootStage::ProviderReady) {
            return;
        }
        delivery_gate.mark_provider_ready();

        // The fallback is allowed only after launch and a provider-ready TUI.
        if matches!(
            delivery,
            crate::service::launch_delivery::LaunchDeliveryMode::ConfirmedFallback
        ) {
            if let Some(spec) = fallback_spec {
                let task_text = spec.payload.clone();
                set_booting(false);
                {
                    let mut mux = svc.mux.write();
                    let pane = mux.get_pane_mut(&pane_id).expect("launch pane exists");
                    pane.pending_delivery.get_or_insert_with(|| {
                        crate::service::launch_delivery::pending_record_for_spec(spec.clone())
                    });
                }
                let consumption = delivery_gate.consume(&svc.mux, &pane_id, &svc, || {
                    crate::service::pty_io::submit_transaction::submit_entry(
                        crate::service::pty_io::submit_transaction::PtySubmitEntry::Direct,
                        &task_text,
                        |text| {
                            svc.send_text_detached(&pane_id, text, true);
                            Ok::<(), String>(())
                        },
                    )?;
                    Ok(true)
                });
                let cleanup_error = if matches!(
                    &consumption,
                    Ok(crate::service::launch_delivery::LaunchConsumptionOutcome::Confirmed)
                ) {
                    svc.confirm_pending_delivery(&pane_id)
                        .err()
                        .map(|error| error.to_string())
                } else {
                    None
                };
                let delivery_error =
                    cleanup_error.or_else(|| consumption.err().map(|error| error.to_string()));
                if let Some(e) = delivery_error {
                    log::error!(
                        "launch_agent: task delivery failed for pane {} (pty {}): {}",
                        pane_id,
                        pty_id,
                        e
                    );
                    launch.fail(&svc, LaunchFailure::TaskDelivery(e.to_string()));
                    set_booting(false);
                    if let Some(tx) = ready_tx {
                        let _ = tx.send(Err(format!("task delivery failed: {}", e)));
                    }
                    return;
                }
            }
        } else if task.is_some() {
            let _ = crate::service::launch_delivery::dispatch_initial_delivery::<(), fn()>(
                delivery, &svc, &pane_id, None,
            );
        }

        set_booting(false);
        if !launch.emit(&svc, BootStage::AgentReady) {
            return;
        }
        LAUNCH_REGISTRY.lock().finish(&launch);
        log::info!(
            "launch_agent: agent ready in pane {} after {}ms",
            pane_id,
            launch_started.elapsed().as_millis()
        );
        if let Some(tx) = ready_tx {
            let _ = tx.send(Ok(()));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{
        build_agent_command, replacement_pty_candidate, reused_provider_tui_owns_launch,
        spawn_onboarding_task, take_reader_termination, AgentRegistry, LaunchFailure,
        LaunchGeneration, LaunchRegistry,
    };
    use crate::chatbackend::{ChatBackendError, ChatBackendProvider, ChatBackendRegistry};
    use crate::multiplexer::state::MultiplexerState;
    use crate::multiplexer::types::AgentRole;
    use crate::service::orchestrator::types::SpawnAgentParams;
    use crate::service::pty_io::pane_identity_env;
    use crate::service::spawn::{PaneCommand, SpawnPaneSpec};
    use std::sync::Arc;
    use std::sync::Barrier;

    fn orchestrator_params(task: Option<&str>) -> SpawnAgentParams {
        SpawnAgentParams {
            agent: Some("test-provider".into()),
            task: task.map(str::to_string),
            role: Some("Orchestrator".into()),
            ..Default::default()
        }
    }

    #[test]
    fn replacement_pty_must_be_new_and_live() {
        assert_eq!(
            replacement_pty_candidate(Some("new"), "closed", true).as_deref(),
            Some("new")
        );
        assert!(replacement_pty_candidate(Some("closed"), "closed", true).is_none());
        assert!(replacement_pty_candidate(Some("new"), "closed", false).is_none());
        assert!(replacement_pty_candidate(None, "closed", true).is_none());
    }

    #[test]
    fn reused_live_provider_tui_is_not_a_shell_readiness_failure() {
        assert!(reused_provider_tui_owns_launch(
            true,
            Some("codex"),
            "codex",
            true,
        ));
        assert!(!reused_provider_tui_owns_launch(
            false,
            Some("codex"),
            "codex",
            true,
        ));
        assert!(!reused_provider_tui_owns_launch(
            true,
            Some("claude"),
            "codex",
            true,
        ));
        assert!(!reused_provider_tui_owns_launch(
            true,
            Some("codex"),
            "codex",
            false,
        ));
    }

    #[test]
    fn production_driver_rejects_a_stale_generation_before_emit_or_write() {
        let mut driver = LaunchRegistry::default();
        let stale = driver.begin("pane-a", "pty-old".into());
        let current = driver.begin("pane-a", "pty-new".into());

        assert!(!driver.owns(&stale));
        assert!(driver.owns(&current));
        assert!(!driver.finish(&stale));
        assert!(driver.owns(&current));
    }

    #[test]
    fn production_driver_write_claim_rejects_a_superseded_generation() {
        let stale = LaunchGeneration::begin("pane-write", "pty-old".into());
        let _current = LaunchGeneration::begin("pane-write", "pty-new".into());
        let wrote = stale.write_if_current(|| true);

        assert_eq!(wrote, None);
    }

    #[test]
    fn barrier_commit_cannot_be_overwritten_by_concurrent_begin() {
        let launch = LaunchGeneration::begin("race-emit", "pty-old".into());
        let barrier = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let held = barrier.clone();
        let unhold = release.clone();
        let worker = std::thread::spawn(move || {
            launch.commit_if_current(|| {
                held.wait();
                unhold.wait();
            })
        });
        barrier.wait();
        release.wait();
        let replacement = LaunchGeneration::begin("race-emit", "pty-new".into());
        assert!(worker.join().unwrap());
        assert!(replacement.owns());
    }

    #[test]
    fn barrier_reader_eof_for_old_pty_cannot_take_replacement() {
        let old = LaunchGeneration::begin("race-eof", "pty-old".into());
        let replacement = LaunchGeneration::begin("race-eof", "pty-new".into());

        assert!(!take_reader_termination("pty-old", |_| {}));
        assert!(replacement.owns());
        assert!(!old.owns());
    }

    #[test]
    fn production_driver_reports_reused_reader_eof_for_its_current_pty() {
        let mut driver = LaunchRegistry::default();
        let launch = driver.begin("pane-a", "pty-live".into());

        let terminated = driver.take_for_pty("pty-live").expect("registered reader");

        assert_eq!(terminated.pane_id, "pane-a");
        assert_eq!(terminated.generation, launch.generation);
        assert!(!driver.owns(&launch));
        assert_eq!(LaunchFailure::PtyEof.reason(), "PtyEof");
    }

    #[test]
    fn reused_success_finishes_before_a_later_reader_eof() {
        let mut driver = LaunchRegistry::default();
        let reused = driver.begin("pane-reused", "pty-live".into());

        assert!(driver.finish(&reused));
        assert!(driver.take_for_pty("pty-live").is_none());
    }

    #[test]
    fn production_driver_replaces_pty_and_reader_generation_together() {
        let mut driver = LaunchRegistry::default();
        let original = driver.begin("pane-a", "pty-old".into());
        let replacement = driver.begin("pane-a", "pty-new".into());

        assert!(!driver.owns(&original));
        assert!(driver.owns(&replacement));
        assert!(driver.take_for_pty("pty-old").is_none());
        assert_eq!(
            driver.take_for_pty("pty-new").unwrap().generation,
            replacement.generation
        );
    }

    #[test]
    fn production_driver_uses_typed_terminal_causes() {
        assert_eq!(
            LaunchFailure::ReaderSetup("closed".into()).reason(),
            "ReaderSetupFailed: closed"
        );
        assert_eq!(
            LaunchFailure::ProviderReadyTimeout.reason(),
            "ProviderReadyTimeout"
        );
        assert_eq!(LaunchFailure::PtyEof.reason(), "PtyEof");
        assert_eq!(
            LaunchFailure::ShellReadyTimeout.reason(),
            "ShellReadyTimeout"
        );
    }

    #[test]
    fn plain_orchestrator_no_task_spawn_submits_nothing() {
        assert_eq!(spawn_onboarding_task(&orchestrator_params(None)), None);
    }

    #[test]
    fn plain_orchestrator_no_task_attaches_nothing() {
        let spec = SpawnPaneSpec::from_agent_params(orchestrator_params(None));

        assert_eq!(spec.parent_pane_id, None);
        match spec.command {
            PaneCommand::Agent(agent) => {
                assert_eq!(agent.role.as_deref(), Some("Orchestrator"));
                assert_eq!(agent.task, None);
                assert_eq!(agent.worker_id, None);
            }
            other => panic!("expected agent spawn state, got {other:?}"),
        }
    }

    #[test]
    fn plain_orchestrator_role_metadata_uses_the_non_task_channel() {
        let (mut mux, _) = MultiplexerState::new();
        let workspace_id = mux.list_workspaces()[0].id.clone();
        let pane_id = mux.create_tab(&workspace_id).unwrap().1;
        mux.set_pane_role(&pane_id, AgentRole::Orchestrator, None)
            .unwrap();
        let pane = mux.get_pane(&pane_id).unwrap();

        assert_eq!(
            pane_identity_env(pane).get("CODETERM_ROLE"),
            Some(&"Orchestrator".to_string())
        );
    }

    #[test]
    fn explicit_orchestrator_task_is_submitted_exactly_once() {
        assert_eq!(
            spawn_onboarding_task(&orchestrator_params(Some("inspect launch"))),
            Some("inspect launch".into())
        );
    }

    #[test]
    fn system_capable_pty_launch_carries_manager_bootstrap_without_fallback_content() {
        let registry = AgentRegistry::new();
        registry.register_provider(Box::new(crate::plugin::agent_claude::ClaudeProvider::new()));
        let content = "## CodeTerm manager identity\nTeam Alpha";
        let command = build_agent_command(
            "claude",
            &orchestrator_params(None),
            Some(content),
            None,
            &registry,
        );
        assert!(command.contains("--append-system-prompt-file"));
        assert!(!command.contains(content));
    }

    #[test]
    fn planner_onboarding_remains_work_when_a_task_is_provided() {
        let params = SpawnAgentParams {
            task: Some("plan this".into()),
            role: Some("Manager".into()),
            role_profile: Some("planner".into()),
            ..Default::default()
        };

        assert!(spawn_onboarding_task(&params)
            .unwrap()
            .contains(&crate::prompts::planner_suffix()));
    }

    struct SpawnableChatBackend;

    impl ChatBackendProvider for SpawnableChatBackend {
        fn id(&self) -> &str {
            "test-chat"
        }

        fn spawnable(&self) -> bool {
            true
        }

        fn open_session(&self, _: serde_json::Value) -> Result<String, ChatBackendError> {
            Ok("session".into())
        }

        fn send_message(&self, _: &str, _: &str) -> Result<(), ChatBackendError> {
            Ok(())
        }

        fn poll(
            &self,
            _: &str,
            _: Option<&str>,
        ) -> Result<crate::agent::chat_types::StructuredChat, ChatBackendError> {
            Ok(Default::default())
        }

        fn close_session(&self, _: &str) -> Result<(), ChatBackendError> {
            Ok(())
        }

        fn list_models(&self) -> Result<Vec<crate::agent::provider::ModelInfo>, ChatBackendError> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn transport_capability_paths_receive_no_task_for_plain_orchestrator() {
        let params = orchestrator_params(None);
        assert_eq!(spawn_onboarding_task(&params), None);

        match SpawnPaneSpec::from_agent_params(params.clone()).command {
            PaneCommand::Agent(agent) => assert_eq!(agent.task, None),
            other => panic!("expected PTY agent route, got {other:?}"),
        }

        let chatbackends = ChatBackendRegistry::new();
        chatbackends.register("test-chat", Arc::new(SpawnableChatBackend));
        let chat_params = SpawnAgentParams {
            agent: Some("test-chat".into()),
            ..params
        };
        match SpawnPaneSpec::from_agent_params_with_chatbackends(&chatbackends, chat_params).command
        {
            PaneCommand::ChatBackend { config, role, .. } => {
                assert!(config.as_object().unwrap().is_empty());
                assert_eq!(role.as_deref(), Some("Orchestrator"));
            }
            other => panic!("expected chatBackend route, got {other:?}"),
        }
    }
}

/// Build a single shell command that launches the agent.
/// Uses the AgentRegistry for known providers; falls back to simple command for unknown agents.
pub(crate) fn build_agent_command(
    agent_cmd: &str,
    params: &SpawnAgentParams,
    task: Option<&str>,
    injected_session_id: Option<&str>,
    registry: &AgentRegistry,
) -> String {
    if let Some(ref sid) = params.resume_session_id {
        let skip = params.skip_permissions.unwrap_or(true);
        if let Some(cmd) = registry
            .with_provider(agent_cmd, |p| p.build_resume_command(sid, skip))
            .flatten()
        {
            return cmd;
        }
    }

    let mut args: Vec<String> = params.args.clone().unwrap_or_default();
    if let Some(ref model) = params.model {
        if !model.is_empty() {
            let resolved = crate::agent::model_catalog::resolve_model(agent_cmd, Some(model));
            let model_args = registry
                .with_provider(agent_cmd, |p| p.model_launch_args(&resolved.id))
                .unwrap_or_else(|| vec!["--model".into(), resolved.id]);
            args.extend(model_args);
        }
    }
    if let Some(ref effort) = params.reasoning_effort {
        if !effort.is_empty() {
            if let Some(effort_args) = registry
                .with_provider(agent_cmd, |p| {
                    p.supports_reasoning_effort(effort)
                        .then(|| p.reasoning_effort_launch_args(effort))
                })
                .flatten()
            {
                args.extend(effort_args);
            }
        }
    }

    let launch_task = registry
        .with_provider(agent_cmd, |provider| {
            crate::service::launch_delivery::content_for_descriptor(
                &provider.system_prompt_delivery(),
                task,
            )
            .system_prompt
        })
        .unwrap_or_default();
    let launch_params = AgentLaunchParams {
        agent_name: params.agent_name.clone(),
        task: launch_task.clone(),
        starter_prompt: params.starter_prompt,
        args: if args.is_empty() {
            None
        } else {
            Some(args.clone())
        },
        skip_permissions: params.skip_permissions.unwrap_or(true),
        session_id: injected_session_id.map(std::string::ToString::to_string),
        structured_prompt_server: params.structured_prompt_server.clone(),
    };
    let cmd = registry
        .with_provider(agent_cmd, |p| {
            crate::agent::stub_launch::apply(agent_cmd, p.build_launch_command(&launch_params))
        })
        .unwrap_or_else(|| {
            let mut parts: Vec<String> = vec![agent_cmd.to_string()];
            parts.extend(args.iter().cloned());
            parts.join(" ")
        });
    crate::service::launch_diagnostics::log_launch(
        "visible_agent",
        "-",
        agent_cmd,
        &cmd,
        launch_task.as_deref(),
    );
    cmd
}
