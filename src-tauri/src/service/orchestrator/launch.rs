use crate::multiplexer::types::TabCloseReason;
use crate::service::{boot_stage::BootStage, ServiceContext};
pub(crate) fn skip_local_worker_role(
    role: &crate::multiplexer::types::AgentRole,
    parent_tab_id: Option<&str>,
    report_target_mesh_peer: Option<&str>,
) -> bool {
    matches!(role, crate::multiplexer::types::AgentRole::Worker)
        && parent_tab_id.is_none()
        && report_target_mesh_peer.map_or(false, |p| !p.is_empty())
}

pub(crate) fn worker_brief_delivery_incident(tab_id: &str, error: &str) -> (String, String) {
    (
        "Orchestration incident: launch brief undelivered".to_string(),
        format!(
            "Impact: Tab {tab_id} was not marked ready, so its assigned work must not proceed. \
             Cleanup: confirmed-delivery cleanup was skipped and the pending brief was retained. \
             Fallback: the launch failed after the bounded submit-key attempt; retry the pending assignment explicitly. \
             Cause: {error}"
        ),
    )
}

pub(crate) use crate::agent::registry::AgentRegistry;
#[derive(Default)]
pub(crate) struct LaunchRegistry {
    next_generation: u64,
    tabs: std::collections::HashMap<String, RegisteredLaunch>,
}
mod command;
#[cfg(test)]
#[path = "launch/exit_recovery_shell_tests.rs"]
mod exit_recovery_shell_tests;
#[path = "launch/failure.rs"]
mod failure;
#[path = "launch/generation.rs"]
mod generation;
mod startup_readiness;
pub(crate) use command::{
    build_agent_command, build_agent_command_for_service, build_agent_command_with_launch_marker,
    build_agent_command_with_launch_marker_for_service,
};
#[cfg(test)]
#[path = "launch/tests.rs"]
mod tests;
use failure::LaunchFailure;
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
pub(crate) fn record_reader_termination(svc: &ServiceContext, pty_id: &str) {
    take_reader_termination(pty_id, |launch| {
        svc.void_launch_session_proof(&launch.tab_id);
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

/// The screen gate misses a prompt drawn beneath an exited provider's frame, so
/// recovery on its own PTY reads the shell's command lifecycle as a level instead.
fn shell_prompt_already_idle(
    recovers_exited_provider: bool,
    reused_live_pty: bool,
    prompt_idle_after_command: impl FnOnce() -> bool,
) -> bool {
    recovers_exited_provider && reused_live_pty && prompt_idle_after_command()
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

fn spawn_backend_pty(
    svc: &ServiceContext,
    tab_id: &str,
    launch_env: &std::collections::HashMap<String, String>,
) -> Result<BackendPty, LaunchFailure> {
    let cwd = svc
        .mux
        .read()
        .get_tab(tab_id)
        .and_then(|tab| tab.cwd.clone());
    let mut env_vars = svc.tab_env(tab_id);
    env_vars.extend(launch_env.clone());
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
    startup_attempt_id: &str,
    windows: bool,
    cli_path: Option<&std::ffi::OsStr>,
) -> String {
    super::launch_env::command_with_binding_nonce_for_shell(
        command,
        binding_nonce,
        startup_attempt_id,
        windows,
        cli_path,
    )
}

fn command_with_binding_nonce(
    command: &str,
    binding_nonce: &str,
    startup_attempt_id: &str,
) -> String {
    let cli_path = std::env::var_os("CODETERM_CLI_PATH");
    command_with_binding_nonce_for_shell(
        command,
        binding_nonce,
        startup_attempt_id,
        cfg!(windows),
        cli_path.as_deref(),
    )
}

#[allow(dead_code)]
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
    origin: crate::service::launch_diagnostics::AgentLaunchOrigin,
    task: Option<String>,
    fallback_spec: Option<crate::service::launch_delivery::FallbackDeliverySpec>,
    task_identity: Option<crate::service::startup_lease::StartupTaskIdentity>,
    ready_tx: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
) {
    launch_agent_in_background_with_env(
        svc,
        tab_id,
        launch_cmd,
        agent_cmd,
        origin,
        task,
        fallback_spec,
        Default::default(),
        task_identity,
        ready_tx,
    );
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn launch_agent_in_background_with_env(
    svc: ServiceContext,
    tab_id: String,
    launch_cmd: String,
    agent_cmd: String,
    origin: crate::service::launch_diagnostics::AgentLaunchOrigin,
    task: Option<String>,
    fallback_spec: Option<crate::service::launch_delivery::FallbackDeliverySpec>,
    launch_env: std::collections::HashMap<String, String>,
    task_identity: Option<crate::service::startup_lease::StartupTaskIdentity>,
    ready_tx: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
) {
    let registry = svc.agent_registry.clone();
    let resumes_on_first_turn =
        startup_readiness::resume_waits_for_first_turn(&svc, &tab_id, &agent_cmd, origin);
    let launch_started = std::time::Instant::now();
    let (role, role_profile) = svc
        .mux
        .read()
        .get_tab(&tab_id)
        .map(|tab| {
            (
                tab.role
                    .map(|role| agent_role_label(role, tab.role_profile.as_deref()).to_string()),
                tab.role_profile.clone(),
            )
        })
        .unwrap_or_default();
    let prompt_delivery = registry
        .with_provider(&agent_cmd, |provider| {
            match provider.system_prompt_delivery() {
                crate::agent::provider::SystemPromptDelivery::CliFlag(_) => "cli_flag",
                crate::agent::provider::SystemPromptDelivery::SessionConfig => "session_config",
                crate::agent::provider::SystemPromptDelivery::External => "confirmed_fallback",
            }
        })
        .unwrap_or("unknown");
    crate::service::launch_diagnostics::log_launch(
        origin,
        &tab_id,
        &agent_cmd,
        &launch_cmd,
        task.as_deref(),
        role.as_deref(),
        role_profile.as_deref(),
        prompt_delivery,
    );
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
            None => match spawn_backend_pty(&svc, &tab_id, &launch_env) {
                Ok(backend_pty) => (backend_pty, false),
                Err(failure) => {
                    let error = failure.reason();
                    log::error!(
                        "launch_agent: backend PTY spawn failed for Tab {}: {}",
                        tab_id,
                        error
                    );
                    launch.fail(&svc, failure);
                    let _ = svc.close_tab_with_reason(&tab_id, TabCloseReason::SpawnRollback);
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
            let _ = svc.close_tab_with_reason(&tab_id, TabCloseReason::SpawnRollback);
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
        // Off the boot critical path: reading the provider's install report
        // touches disk, and the tab is already up by here.
        svc.announce_integration_silence(&tab_id, &agent_cmd);

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

        let (tab_agent_type, existing_binding_nonce) = {
            let mux = svc.mux.read();
            mux.get_tab(&tab_id)
                .map(|tab| {
                    let binding_nonce = tab
                        .launch_session_authenticated()
                        .then(|| tab.session_binding_nonce.clone())
                        .flatten();
                    (tab.agent_type.clone(), binding_nonce)
                })
                .unwrap_or_default()
        };
        let screen = svc
            .screen_state
            .lock()
            .get_screen_text(&pty_id)
            .unwrap_or_default();
        let provider_tui_visible = registry
            .with_provider(&agent_cmd, |provider| provider.screen_has_tui(&screen))
            .unwrap_or(false);
        // An exited provider's frame can outlive it on screen; recovery still writes its command.
        let recovers_exited_provider = matches!(
            origin,
            crate::service::launch_diagnostics::AgentLaunchOrigin::ExitRecovery
        );
        if !recovers_exited_provider
            && startup_readiness::reused_provider_tui_owns_launch(
                reused_live_pty,
                existing_binding_nonce.is_some(),
                tab_agent_type.as_deref(),
                &agent_cmd,
                provider_tui_visible,
            )
        {
            log::info!(
                "launch_agent: Tab {} already owns live {} TUI on PTY {}; preserving ready state",
                tab_id,
                agent_cmd,
                pty_id
            );
            let Some(binding_nonce) = existing_binding_nonce else {
                unreachable!("a reused provider TUI owns an authenticated binding")
            };
            if !launch.adopt_authenticated_binding(binding_nonce.clone()) {
                let failure = LaunchFailure::SessionBindingNotObserved(
                    "the authenticated session could not be adopted".into(),
                );
                let error = failure.reason();
                launch.fail(&svc, failure);
                set_booting(false);
                if let Some(tx) = ready_tx {
                    let _ = tx.send(Err(error));
                }
                return;
            }
            if let Err(error) = svc.activate_agent_startup_supervision(
                &tab_id,
                &launch.pty_id,
                &binding_nonce,
                task_identity.as_ref(),
            ) {
                let failure = LaunchFailure::StartupSupervision(error.clone());
                launch.fail(&svc, failure);
                set_booting(false);
                if let Some(tx) = ready_tx {
                    let _ = tx.send(Err(error));
                }
                return;
            }
            launch.reuse();
            if !launch.emit(&svc, launch.stage) {
                let failure = LaunchFailure::SessionBindingNotObserved(
                    "the reused launch lost its authenticated session".into(),
                );
                let error = failure.reason();
                launch.fail(&svc, failure);
                set_booting(false);
                if let Some(tx) = ready_tx {
                    let _ = tx.send(Err(error));
                }
                return;
            }
            LAUNCH_REGISTRY.lock().finish(&launch);
            set_booting(false);
            if let Some(tx) = ready_tx {
                let _ = tx.send(Ok(()));
            }
            return;
        }
        if let Err(error) = launch.capture_binding_nonce(&svc, task_identity.as_ref()) {
            let failure = LaunchFailure::StartupSupervision(error.clone());
            launch.fail(&svc, failure);
            set_booting(false);
            if let Some(tx) = ready_tx {
                let _ = tx.send(Err(error));
            }
            return;
        }
        let mut wrapped_launch_cmd = command_with_binding_nonce(
            &launch_cmd,
            launch
                .binding_nonce
                .as_deref()
                .expect("launch binding nonce"),
            &launch.pty_id,
        );

        let shell_already_idle =
            shell_prompt_already_idle(recovers_exited_provider, reused_live_pty, || {
                svc.shell_caps.lock().prompt_idle_after_command(&pty_id)
            });
        if shell_already_idle {
            log::info!(
                "launch_agent: Tab {} exit recovery reuses the idle shell prompt on PTY {}",
                tab_id,
                pty_id
            );
        }
        if !shell_already_idle
            && !crate::service::pty_io::await_bare_shell_prompt(
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
                        if let Err(error) =
                            launch.capture_binding_nonce(&svc, task_identity.as_ref())
                        {
                            let failure = LaunchFailure::StartupSupervision(error.clone());
                            launch.fail(&svc, failure);
                            set_booting(false);
                            if let Some(tx) = ready_tx {
                                let _ = tx.send(Err(error));
                            }
                            return;
                        }
                        wrapped_launch_cmd = command_with_binding_nonce(
                            &launch_cmd,
                            launch
                                .binding_nonce
                                .as_deref()
                                .expect("replacement launch binding nonce"),
                            &launch.pty_id,
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
            log::warn!(
                "[provider-readiness-stage] tab={} pty={} ownership_lost_after_launch_write",
                tab_id,
                pty_id,
            );
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

        // A written launch command is not readiness; observing this attempt to the
        // bounded deadline turns an early provider exit into one launch failure.
        let readiness_attempt = crate::service::launch_readiness::LaunchAttempt {
            tab_id: tab_id.clone(),
            pty_id: pty_id.clone(),
            binding_nonce: launch.binding_nonce.clone().expect("launch binding nonce"),
        };
        crate::service::launch_readiness::arm(readiness_attempt.clone());
        if launch.await_provider_ack(std::time::Duration::ZERO) {
            crate::service::launch_readiness::note_acknowledged(
                &readiness_attempt.tab_id,
                &readiness_attempt.pty_id,
                &readiness_attempt.binding_nonce,
            );
        }
        let readiness_receiver = svc.observe_launch_readiness(
            readiness_attempt.clone(),
            crate::service::worker_ack::STARTUP_BINDING_DEADLINE,
            resumes_on_first_turn,
        );

        let provider_readiness = svc
            .agent_registry
            .with_provider(&agent_cmd, |provider| {
                provider.provider_readiness_strategy()
            })
            .unwrap_or_default();
        let input_readiness_timeout = provider_readiness.timeout();
        log::info!(
            "[provider-readiness-stage] tab={} pty={} strategy={:?} require_composer={}",
            tab_id,
            pty_id,
            provider_readiness,
            provider_readiness.requires_composer(),
        );
        let launch_input_readiness = if fallback_spec.is_some() {
            match svc.await_launch_input_readiness(
                &readiness_attempt,
                &agent_cmd,
                input_readiness_timeout,
            ) {
                Ok(readiness) => Some(readiness),
                Err(refusal) => {
                    let failure = LaunchFailure::ProviderInputNotReady(refusal.reason());
                    launch.fail(&svc, failure.clone());
                    set_booting(false);
                    if let Some(tx) = ready_tx {
                        let _ = tx.send(Err(failure.reason()));
                    }
                    return;
                }
            }
        } else {
            None
        };
        if let Some(readiness) = launch_input_readiness {
            log::info!(
                "[launch-input-readiness] tab={} pty={} evidence={:?}",
                tab_id,
                pty_id,
                readiness,
            );
        }
        let input_ready_stage =
            startup_readiness::input_ready_stage_after_provider_ready(resumes_on_first_turn);
        if resumes_on_first_turn {
            set_booting(false);
        }
        let input_ready = if resumes_on_first_turn {
            launch.emit_resume_first_turn_input_ready(&svc)
        } else {
            launch.emit(&svc, input_ready_stage)
        };
        if !input_ready {
            return;
        }
        delivery_gate.mark_provider_ready();

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
            let submit_strategy = registry
                .with_provider(&agent_cmd, |provider| provider.verify_submit_strategy())
                .unwrap_or(crate::agent::provider::VerifySubmitStrategy::None);
            let launch_input_readiness = match svc.await_launch_input_readiness(
                &readiness_attempt,
                &agent_cmd,
                std::time::Duration::ZERO,
            ) {
                Ok(readiness) => readiness,
                Err(refusal) => {
                    let failure = LaunchFailure::ProviderInputNotReady(refusal.reason());
                    launch.fail(&svc, failure.clone());
                    set_booting(false);
                    if let Some(tx) = ready_tx {
                        let _ = tx.send(Err(failure.reason()));
                    }
                    return;
                }
            };
            let consumption = delivery_gate.consume_injection_with_readiness(
                &svc.mux,
                &tab_id,
                &svc,
                Some(launch_input_readiness),
                || {
                    crate::service::pty_io::submit_transaction::submit_text_with_key(
                        &svc,
                        &pty_id,
                        &task_text,
                        true,
                        b"\r",
                        submit_strategy,
                        crate::service::inject_verify::VerifyOpts {
                            require_prompt_first: false,
                            verify_echo: true,
                            verify_submit: true,
                            ..Default::default()
                        },
                        None,
                    )
                },
            );
            let delivery_error = match consumption {
                Ok(receipt)
                    if receipt.outcome
                        == crate::service::launch_delivery::LaunchConsumptionOutcome::Confirmed =>
                {
                    svc.confirm_pending_delivery(
                        &tab_id,
                        receipt
                            .attempt_id
                            .as_deref()
                            .expect("confirmed launch delivery carries its attempt"),
                    )
                    .err()
                    .map(|error| error.to_string())
                }
                Ok(_) => None,
                Err(error) if error.surfaces_as_launch_failure() => Some(error.to_string()),
                Err(error) => {
                    log::warn!(
                        "[launch-delivery] tab={} pty={} state={} bytes_written=true; pending evidence retained",
                        tab_id,
                        pty_id,
                        error.delivery.code(),
                    );
                    None
                }
            };
            if let Some(e) = delivery_error {
                let (title, body) = worker_brief_delivery_incident(&tab_id, &e);
                svc.create_notification(title, Some(body), Some("error"));
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
        } else if task.is_some() {
            let _ = crate::service::launch_delivery::dispatch_initial_delivery::<(), fn()>(
                delivery, &svc, &tab_id, None,
            );
        }

        set_booting(false);
        let readiness = readiness_receiver.recv().unwrap_or_else(|_| {
            crate::service::launch_readiness::LaunchSettlement::Provisional {
                stage: BootStage::LaunchDeadlineElapsed,
                reason: "launch readiness observer stopped before session binding".into(),
            }
        });
        if let Some(finished) =
            startup_readiness::finish_waiting_for_first_turn(&mut launch, &readiness)
        {
            if finished {
                if let Some(tx) = ready_tx {
                    let _ = tx.send(Ok(()));
                }
            } else if let Some(tx) = ready_tx {
                let _ = tx.send(Err("launch readiness was superseded".into()));
            }
            return;
        }
        if let Err(error) = startup_readiness::emit_agent_ready(&mut launch, &svc, readiness) {
            if let Some(tx) = ready_tx {
                let _ = tx.send(Err(error));
            }
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

fn agent_role_label(
    role: crate::multiplexer::types::AgentRole,
    role_profile: Option<&str>,
) -> &'static str {
    codeterm_types::agent::role_kind::kind_label(Some(role), role_profile)
        .unwrap_or_else(|| role.label())
}
