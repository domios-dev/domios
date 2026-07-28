use crate::{
    service::orchestrator::types::SpawnAgentParams,
    service::{boot_stage::BootStage, ServiceContext},
};
pub(crate) fn skip_local_worker_role(
    role: &crate::multiplexer::types::AgentRole,
    parent_tab_id: Option<&str>,
    report_target_mesh_peer: Option<&str>,
) -> bool {
    matches!(role, crate::multiplexer::types::AgentRole::Worker)
        && parent_tab_id.is_none()
        && report_target_mesh_peer.map_or(false, |p| !p.is_empty())
}

fn role_assignment_task(
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
        (Some("Orchestrator"), _) => {
            let suffix = crate::prompts::orchestrator_suffix();
            Some(match task {
                Some(task) => format!("{task}\n\n{suffix}"),
                None => suffix,
            })
        }
        _ => task.map(str::to_string),
    }
}

pub(crate) fn role_onboarding_task(
    task: Option<&str>,
    role: Option<&str>,
    role_profile: Option<&str>,
) -> Option<String> {
    let assignment = role_assignment_task(task, role, role_profile);
    Some(agent_system_prompt(assignment.as_deref()))
}

pub(crate) fn role_launch_task(
    task: Option<&str>,
    role: Option<&str>,
    role_profile: Option<&str>,
    system_prompt_capable: bool,
) -> Option<String> {
    if system_prompt_capable || role.is_some() {
        role_onboarding_task(task, role, role_profile)
    } else {
        role_assignment_task(task, role, role_profile)
    }
}

pub(crate) fn agent_system_prompt(assignment: Option<&str>) -> String {
    match assignment.filter(|text| !text.trim().is_empty()) {
        Some(assignment) => format!(
            "{}\n\n## Assignment\n\n{}",
            crate::prompts::AGENT_CODETERM_PRIMER.trim(),
            assignment
        ),
        None => crate::prompts::AGENT_CODETERM_PRIMER.trim().to_string(),
    }
}

#[cfg(test)]
pub(crate) fn spawn_onboarding_task(params: &SpawnAgentParams) -> Option<String> {
    role_onboarding_task(
        params.task.as_deref(),
        params.role.as_deref(),
        params.role_profile.as_deref(),
    )
}

pub(crate) fn spawn_launch_task(
    params: &SpawnAgentParams,
    system_prompt_capable: bool,
) -> Option<String> {
    role_launch_task(
        params.task.as_deref(),
        params.role.as_deref(),
        params.role_profile.as_deref(),
        system_prompt_capable,
    )
}

pub(crate) use crate::agent::{provider::AgentLaunchParams, registry::AgentRegistry};
#[derive(Default)]
pub(crate) struct LaunchRegistry {
    next_generation: u64,
    tabs: std::collections::HashMap<String, RegisteredLaunch>,
}
#[cfg(test)]
#[path = "launch/tests.rs"]
mod binding_tests;
#[derive(Clone, Debug)]
struct RegisteredLaunch {
    pty_id: String,
    generation: u64,
    provider_ack: std::sync::Arc<ProviderAck>,
    binding_nonce: Option<String>,
}
#[derive(Debug, Default)]
struct ProviderAck {
    ready: parking_lot::Mutex<bool>,
    changed: parking_lot::Condvar,
}
impl LaunchRegistry {
    pub(crate) fn begin(&mut self, tab_id: &str, pty_id: String) -> LaunchGeneration {
        self.next_generation = self.next_generation.wrapping_add(1).max(1);
        let launch = LaunchGeneration {
            tab_id: tab_id.into(),
            pty_id,
            generation: self.next_generation,
            stage: BootStage::PtySpawned,
            reused: false,
            provider_ack: std::sync::Arc::new(ProviderAck::default()),
            binding_nonce: None,
        };
        self.tabs.insert(
            launch.tab_id.clone(),
            RegisteredLaunch {
                pty_id: launch.pty_id.clone(),
                generation: launch.generation,
                provider_ack: launch.provider_ack.clone(),
                binding_nonce: None,
            },
        );
        launch
    }

    fn owns(&self, launch: &LaunchGeneration) -> bool {
        self.tabs.get(&launch.tab_id).is_some_and(|registered| {
            registered.pty_id == launch.pty_id && registered.generation == launch.generation
        })
    }

    fn finish(&mut self, launch: &LaunchGeneration) -> bool {
        if !self.owns(launch) {
            return false;
        }
        self.tabs.remove(&launch.tab_id);
        true
    }

    pub(crate) fn bind_provider_session(
        &mut self,
        launch: &LaunchGeneration,
        binding_nonce: String,
    ) -> bool {
        let Some(registered) = self.tabs.get_mut(&launch.tab_id) else {
            return false;
        };
        if registered.pty_id != launch.pty_id || registered.generation != launch.generation {
            return false;
        }
        registered.binding_nonce = Some(binding_nonce);
        true
    }

    fn record_provider_session(&self, tab_id: &str, binding: &ProviderSessionBinding) -> bool {
        let Some(ack) = self
            .tabs
            .get(tab_id)
            .filter(|launch| {
                launch.pty_id == binding.pty_id
                    && launch.binding_nonce.as_deref() == Some(&binding.binding_nonce)
            })
            .map(|launch| launch.provider_ack.clone())
        else {
            return false;
        };
        *ack.ready.lock() = true;
        ack.changed.notify_all();
        true
    }

    fn take_for_pty(&mut self, pty_id: &str) -> Option<LaunchGeneration> {
        let tab_id = self
            .tabs
            .iter()
            .find(|(_, registered)| registered.pty_id == pty_id)
            .map(|(tab_id, _)| tab_id.clone())?;
        let registered = self.tabs.remove(&tab_id)?;
        Some(LaunchGeneration {
            tab_id,
            pty_id: registered.pty_id,
            generation: registered.generation,
            stage: BootStage::PtySpawned,
            reused: false,
            provider_ack: registered.provider_ack,
            binding_nonce: registered.binding_nonce,
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
    TabClosed,
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
            Self::TabClosed => "TabClosedBeforePtyLink".into(),
            Self::LaunchWrite(error) => format!("LaunchWriteFailed: {error}"),
            Self::ProviderReadyTimeout => "ProviderReadyTimeout".into(),
            Self::TaskDelivery(error) => format!("TaskDeliveryFailed: {error}"),
        }
    }
}

fn provider_launch_failure(
    reader_closed: bool,
    provider_hook_ready: bool,
    provider_tui_ready: bool,
) -> Option<LaunchFailure> {
    if reader_closed {
        Some(LaunchFailure::PtyEof)
    } else if provider_hook_ready || provider_tui_ready {
        None
    } else {
        Some(LaunchFailure::ProviderReadyTimeout)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct LaunchGeneration {
    tab_id: String,
    pty_id: String,
    generation: u64,
    stage: BootStage,
    reused: bool,
    provider_ack: std::sync::Arc<ProviderAck>,
    binding_nonce: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProviderSessionBinding {
    pub(crate) pty_id: String,
    pub(crate) binding_nonce: String,
}

pub(crate) fn record_provider_session(tab_id: &str, binding: &ProviderSessionBinding) -> bool {
    LAUNCH_REGISTRY
        .lock()
        .record_provider_session(tab_id, binding)
}
#[cfg(test)]
pub(crate) fn arm_provider_session_for_test(
    tab_id: &str,
    pty_id: &str,
    nonce: &str,
) -> LaunchGeneration {
    let launch = LaunchGeneration::begin(tab_id, pty_id.to_string());
    LAUNCH_REGISTRY
        .lock()
        .bind_provider_session(&launch, nonce.to_string());
    launch
}
impl LaunchGeneration {
    fn begin(tab_id: &str, pty_id: String) -> Self {
        LAUNCH_REGISTRY.lock().begin(tab_id, pty_id)
    }

    fn capture_binding_nonce(&mut self, svc: &ServiceContext) {
        let nonce = svc.rotate_session_binding_nonce(&self.tab_id);
        self.binding_nonce = Some(nonce.clone());
        LAUNCH_REGISTRY.lock().bind_provider_session(self, nonce);
    }

    fn transition(&mut self, stage: BootStage) {
        self.stage = stage;
    }

    fn reuse(&mut self) {
        self.reused = true;
        self.transition(BootStage::AgentReady);
    }

    fn replace(&mut self, pty_id: String) {
        let tab_id = self.tab_id.clone();
        *self = Self::begin(&tab_id, pty_id);
    }

    fn emit(&mut self, svc: &ServiceContext, stage: BootStage) -> bool {
        self.transition(stage);
        self.commit_if_current(|| {
            svc.emit_boot_stage(&self.tab_id, stage, None);
        })
    }

    fn fail(&self, svc: &ServiceContext, failure: LaunchFailure) -> bool {
        let reason = failure.reason();
        self.finish_if_current(|| {
            svc.emit_boot_stage(&self.tab_id, BootStage::SpawnFailed, Some(&reason));
        })
    }

    fn write_if_current<T>(&self, write: impl FnOnce() -> T) -> Option<T> {
        let registry = LAUNCH_REGISTRY.lock();
        registry.owns(self).then(write)
    }

    pub(crate) fn await_provider_ack(&self, timeout: std::time::Duration) -> bool {
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
        svc.emit_boot_stage(&launch.tab_id, BootStage::SpawnFailed, Some(&reason));
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

fn existing_live_pty(svc: &ServiceContext, tab_id: &str) -> Option<String> {
    let pty_id = svc
        .mux
        .read()
        .get_tab(tab_id)
        .and_then(|tab| tab.pty_id.clone())?;
    svc.pty
        .lock()
        .get_session(&pty_id)
        .is_some()
        .then_some(pty_id)
}

fn reused_provider_tui_owns_launch(
    reused_live_pty: bool,
    tab_agent_type: Option<&str>,
    requested_agent: &str,
    provider_tui_visible: bool,
) -> bool {
    reused_live_pty && tab_agent_type == Some(requested_agent) && provider_tui_visible
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

fn spawn_backend_pty(svc: &ServiceContext, tab_id: &str) -> Result<BackendPty, LaunchFailure> {
    let cwd = svc
        .mux
        .read()
        .get_tab(tab_id)
        .and_then(|tab| tab.cwd.clone());
    let env_vars = svc.tab_env(tab_id);
    let (pty_id, _) = svc
        .pty
        .lock()
        .spawn(None, cwd, 120, 40, env_vars)
        .map_err(|error| LaunchFailure::PtySpawn(error.to_string()))?;
    svc.screen_state.lock().resize(&pty_id, 120, 40);
    let reader = start_backend_reader(svc, &pty_id)?;

    Ok(BackendPty { id: pty_id, reader })
}

fn command_with_binding_nonce_for_shell(
    command: &str,
    binding_nonce: &str,
    windows: bool,
) -> String {
    if windows {
        format!(
            "$env:CODETERM_SESSION_BINDING_NONCE='{}'; {command}",
            binding_nonce.replace('\'', "''")
        )
    } else {
        format!(
            "export CODETERM_SESSION_BINDING_NONCE='{}'; {command}",
            binding_nonce.replace('\'', "'\"'\"'")
        )
    }
}

fn command_with_binding_nonce(command: &str, binding_nonce: &str) -> String {
    command_with_binding_nonce_for_shell(command, binding_nonce, cfg!(windows))
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
    tab_id: &str,
    closed_pty_id: &str,
    timeout: std::time::Duration,
) -> Option<String> {
    #[cfg(not(windows))]
    {
        let _ = (svc, tab_id, closed_pty_id, timeout);
        None
    }
    #[cfg(windows)]
    {
        let deadline = std::time::Instant::now() + timeout;
        super::tab_relink_signal::wait_for_relink(deadline, || {
            let linked = svc
                .mux
                .read()
                .get_tab(tab_id)
                .and_then(|tab| tab.pty_id.clone());
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
    tab_id: String,
    launch_cmd: String,
    agent_cmd: String,
    task: Option<String>,
    fallback_spec: Option<crate::service::launch_delivery::FallbackDeliverySpec>,
    ready_tx: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
) {
    let registry = svc.agent_registry.clone();
    let launch_started = std::time::Instant::now();
    // thread spawns: callers get the tab id immediately and may send right away.
    if let Some(p) = svc.mux.write().get_tab_mut(&tab_id) {
        p.agent_booting = true;
    }
    crate::service::boot_wait::notify_boot_changed();
    std::thread::spawn(move || {
        let mut delivery_gate = crate::service::launch_delivery::LaunchDeliveryGate::new();
        let mut launch = LaunchGeneration::begin(&tab_id, "pending".into());
        if !launch.emit(&svc, BootStage::PtyStarting) {
            return;
        }
        let set_booting = |value: bool| {
            if let Some(p) = svc.mux.write().get_tab_mut(&tab_id) {
                p.agent_booting = value;
            }
            crate::service::boot_wait::notify_boot_changed();
        };
        let pty_claim = svc.pty_ownership.acquire(&tab_id);
        let (backend_pty, reused_live_pty) = match existing_live_pty(&svc, &tab_id) {
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
            None => match spawn_backend_pty(&svc, &tab_id) {
                Ok(backend_pty) => (backend_pty, false),
                Err(failure) => {
                    let error = failure.reason();
                    log::error!(
                        "launch_agent: backend PTY spawn failed for Tab {}: {}",
                        tab_id,
                        error
                    );
                    launch.fail(&svc, failure);
                    let _ = svc.close_tab(&tab_id);
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
        let selection = svc.link_pty_with_claim(&pty_claim, &tab_id, candidate_pty_id, true);
        drop(pty_claim);
        if !selection.linked {
            let error = LaunchFailure::TabClosed.reason();
            launch.fail(&svc, LaunchFailure::TabClosed);
            set_booting(false);
            let _ = svc.close_tab(&tab_id);
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

        let (tab_agent_type, screen) = {
            let tab_agent_type = svc
                .mux
                .read()
                .get_tab(&tab_id)
                .and_then(|tab| tab.agent_type.clone());
            let screen = svc
                .screen_state
                .lock()
                .get_screen_text(&pty_id)
                .unwrap_or_default();
            (tab_agent_type, screen)
        };
        let provider_tui_visible = registry
            .with_provider(&agent_cmd, |provider| provider.screen_has_tui(&screen))
            .unwrap_or(false);
        if reused_provider_tui_owns_launch(
            reused_live_pty,
            tab_agent_type.as_deref(),
            &agent_cmd,
            provider_tui_visible,
        ) {
            log::info!(
                "launch_agent: Tab {} already owns live {} TUI on PTY {}; preserving ready state",
                tab_id,
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
        launch.capture_binding_nonce(&svc);
        let mut wrapped_launch_cmd = command_with_binding_nonce(
            &launch_cmd,
            launch
                .binding_nonce
                .as_deref()
                .expect("launch binding nonce"),
        );

        if !crate::service::pty_io::await_bare_shell_prompt(
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
            .write_if_current(|| svc.send_shell_spawn_after_ready(&pty_id, &wrapped_launch_cmd))
        {
            Some(result) => result,
            None => return,
        };
        let launch_result = match first_launch {
            Err(error) => {
                match await_replacement_pty(
                    &svc,
                    &tab_id,
                    &pty_id,
                    std::time::Duration::from_secs(10),
                ) {
                    Some(replacement) => {
                        log::warn!(
                            "launch_agent: retrying launch for Tab {} on replacement pty {} after pty {} closed",
                            tab_id, replacement, pty_id
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
                        launch.capture_binding_nonce(&svc);
                        wrapped_launch_cmd = command_with_binding_nonce(
                            &launch_cmd,
                            launch
                                .binding_nonce
                                .as_deref()
                                .expect("replacement launch binding nonce"),
                        );
                        if !launch.emit(&svc, BootStage::PtySpawned)
                            || !launch.emit(&svc, BootStage::ReaderReady)
                            || !crate::service::pty_io::await_bare_shell_prompt(
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
                            svc.send_shell_spawn_after_ready(&pty_id, &wrapped_launch_cmd)
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
                "launch_agent: launch_cmd failed for Tab {} (pty {}): {}",
                tab_id,
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

        let provider_hook_ready = launch.await_provider_ack(std::time::Duration::ZERO);
        let provider_tui_ready = provider_hook_ready
            || svc.await_agent_tui_frame_on_screen(
                &pty_id,
                &agent_cmd,
                std::time::Duration::from_secs(30),
            );
        if let Some(failure) = provider_launch_failure(
            reader_monitor.closed(),
            provider_hook_ready,
            provider_tui_ready,
        ) {
            let error = failure.reason();
            launch.fail(&svc, failure);
            set_booting(false);
            if let Some(tx) = ready_tx {
                let _ = tx.send(Err(error));
            }
            return;
        }
        if !provider_hook_ready {
            log::warn!(
                "launch_agent: Tab {} accepted live {} TUI before session binding",
                tab_id,
                agent_cmd
            );
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
                    let tab = mux.get_tab_mut(&tab_id).expect("launch Tab exists");
                    tab.pending_delivery.get_or_insert_with(|| {
                        crate::service::launch_delivery::pending_record_for_spec(spec.clone())
                    });
                }
                let consumption = delivery_gate.consume(&svc.mux, &tab_id, &svc, || {
                    crate::service::pty_io::submit_transaction::submit_entry(
                        crate::service::pty_io::submit_transaction::PtySubmitEntry::Direct,
                        &task_text,
                        |text| {
                            svc.send_text_detached(&tab_id, text, true);
                            Ok::<(), String>(())
                        },
                    )?;
                    Ok(true)
                });
                let cleanup_error = if matches!(
                    &consumption,
                    Ok(crate::service::launch_delivery::LaunchConsumptionOutcome::Confirmed)
                ) {
                    svc.confirm_pending_delivery(&tab_id)
                        .err()
                        .map(|error| error.to_string())
                } else {
                    None
                };
                let delivery_error =
                    cleanup_error.or_else(|| consumption.err().map(|error| error.to_string()));
                if let Some(e) = delivery_error {
                    log::error!(
                        "launch_agent: task delivery failed for Tab {} (pty {}): {}",
                        tab_id,
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
                delivery, &svc, &tab_id, None,
            );
        }

        set_booting(false);
        if !launch.emit(&svc, BootStage::AgentReady) {
            return;
        }
        LAUNCH_REGISTRY.lock().finish(&launch);
        log::info!(
            "launch_agent: agent ready in Tab {} after {}ms",
            tab_id,
            launch_started.elapsed().as_millis()
        );
        if let Some(tx) = ready_tx {
            let _ = tx.send(Ok(()));
        }
    });
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
            let system_prompt = crate::service::launch_delivery::content_for_descriptor(
                &provider.system_prompt_delivery(),
                task,
            )
            .system_prompt;
            system_prompt.or_else(|| {
                matches!(
                    provider.task_delivery(),
                    crate::agent::provider::TaskDelivery::InlineFlag
                )
                .then(|| task.map(str::to_string))
                .flatten()
            })
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
