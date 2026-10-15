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

pub(crate) use crate::agent::registry::AgentRegistry;
mod generation_registry;
use generation_registry::LaunchRegistry;
#[path = "launch/backend_reader.rs"]
mod backend_reader;
#[path = "launch/bootstrap.rs"]
mod bootstrap;
mod command;
use backend_reader::start_backend_reader;
#[path = "launch/cwd_guard.rs"]
mod cwd_guard;
#[path = "launch/exit_recovery_shell.rs"]
mod exit_recovery_shell;
#[cfg(test)]
#[path = "launch/exit_recovery_shell_tests.rs"]
mod exit_recovery_shell_tests;
#[path = "launch/failure.rs"]
mod failure;
use exit_recovery_shell::exited_provider_recovery_has_shell_ready_evidence;
#[path = "launch/generation.rs"]
mod generation;
#[path = "launch/owner_prompt_wait.rs"]
mod owner_prompt_wait;
mod preparation;
mod public_send;
#[path = "launch/reader_readiness.rs"]
mod reader_readiness;
mod resume_binding;
mod startup_readiness;
mod user_input;
pub(crate) use command::{
    build_agent_command, build_agent_command_for_service, build_agent_command_with_launch_marker,
    build_agent_command_with_launch_marker_for_service,
};
use reader_readiness::{BackendPty, ReaderReadiness};
#[cfg(any(test, feature = "test-support"))]
pub use startup_readiness::launch_waits_for_first_turn_binding;
#[cfg(test)]
#[path = "launch/tests.rs"]
mod tests;
use failure::{worker_brief_delivery_incident, LaunchFailure};
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

static LAUNCH_REGISTRY: std::sync::LazyLock<parking_lot::Mutex<LaunchRegistry>> =
    std::sync::LazyLock::new(|| parking_lot::Mutex::new(LaunchRegistry::default()));
#[derive(Debug, Clone)]
pub struct LaunchGeneration {
    tab_id: String,
    pty_id: String,
    generation: u64,
    stage: BootStage,
    reused: bool,
    provider_ack: std::sync::Arc<ProviderAck>,
    binding_nonce: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderSessionBinding {
    pub pty_id: String,
    pub binding_nonce: String,
}

pub(crate) fn record_provider_session(tab_id: &str, binding: &ProviderSessionBinding) -> bool {
    let recorded = LAUNCH_REGISTRY
        .lock()
        .record_provider_session(tab_id, binding);
    if recorded {
        crate::service::agent_restart::timing::stage(tab_id, &binding.pty_id, "session-bound");
    }
    recorded
}
#[cfg(any(test, feature = "test-support"))]
pub fn arm_provider_session_for_test(tab_id: &str, pty_id: &str, nonce: &str) -> LaunchGeneration {
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

pub(crate) fn supersede_tab_launch(tab_id: &str) {
    LAUNCH_REGISTRY.lock().supersede(tab_id);
    log::debug!("[launch] tab={tab_id} superseded before restart teardown");
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

fn spawn_backend_pty(
    svc: &ServiceContext,
    tab_id: &str,
    launch_env: &std::collections::HashMap<String, String>,
    provider_id: &str,
) -> Result<BackendPty, LaunchFailure> {
    let cwd = svc
        .mux
        .read()
        .get_tab(tab_id)
        .and_then(|tab| tab.cwd.clone());
    let mut env_vars = svc.tab_env(tab_id);
    env_vars.extend(launch_env.clone());
    env_vars.extend(
        svc.model_provider_launch_env(provider_id)
            .map_err(|error| LaunchFailure::PtySpawn(error.to_string()))?,
    );
    crate::service::agent_restart::timing::stage(tab_id, "pending", "relaunch-pty-request");
    let (pty_id, _) = svc
        .pty
        .lock()
        .spawn(None, cwd, 120, 40, env_vars)
        .map_err(|error| LaunchFailure::PtySpawn(error.to_string()))?;
    crate::service::agent_restart::timing::stage(tab_id, &pty_id, "relaunch-pty-created");
    crate::service::pty_resize::resize(&svc.pty, &svc.screen_state, &pty_id, None)
        .map_err(LaunchFailure::PtySpawn)?;
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
    let cli_path = crate::service::instance_env::instance_cli_binary_path();
    command_with_binding_nonce_for_shell(
        command,
        binding_nonce,
        startup_attempt_id,
        cfg!(windows),
        cli_path.as_ref().map(|path| path.as_os_str()),
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
        false,
        None,
        task,
        fallback_spec,
        Default::default(),
        task_identity,
        ready_tx,
        None,
    );
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn launch_agent_in_background_with_task_turn(
    svc: ServiceContext,
    tab_id: String,
    launch_cmd: String,
    agent_cmd: String,
    origin: crate::service::launch_diagnostics::AgentLaunchOrigin,
    has_startup_task_turn: bool,
    startup_turn_text: Option<String>,
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
        has_startup_task_turn,
        startup_turn_text,
        task,
        fallback_spec,
        Default::default(),
        task_identity,
        ready_tx,
        None,
    );
}

#[allow(clippy::too_many_arguments)]
pub fn launch_agent_in_background_with_env(
    svc: ServiceContext,
    tab_id: String,
    launch_cmd: String,
    agent_cmd: String,
    origin: crate::service::launch_diagnostics::AgentLaunchOrigin,
    has_startup_task_turn: bool,
    startup_turn_text: Option<String>,
    task: Option<String>,
    fallback_spec: Option<crate::service::launch_delivery::FallbackDeliverySpec>,
    launch_env: std::collections::HashMap<String, String>,
    task_identity: Option<crate::service::startup_lease::StartupTaskIdentity>,
    ready_tx: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
    resume_session_id: Option<String>,
) {
    if let Some(reason) = cwd_guard::refuse_launch_without_cwd(&svc, &tab_id) {
        if let Some(tx) = ready_tx {
            let _ = tx.send(Err(reason));
        }
        return;
    }
    let registry = svc.agent_registry.clone();
    let launch_started = std::time::Instant::now();
    let (role, role_profile, model, reasoning_effort) = svc
        .mux
        .read()
        .get_tab(&tab_id)
        .map(|tab| {
            (
                tab.role
                    .map(|role| agent_role_label(role, tab.role_profile.as_deref()).to_string()),
                tab.role_profile.clone(),
                tab.restorable_launch
                    .model
                    .clone()
                    .or_else(|| tab.agent_provider_resolved_model.clone()),
                tab.restorable_launch
                    .resolved_reasoning_effort
                    .clone()
                    .or_else(|| tab.reasoning_effort_identity.resolved.clone()),
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
    crate::service::launch_diagnostics::log_launch_with_parameters(
        origin,
        &tab_id,
        &agent_cmd,
        &launch_cmd,
        task.as_deref(),
        role.as_deref(),
        role_profile.as_deref(),
        prompt_delivery,
        model.as_deref(),
        reasoning_effort.as_deref(),
    );
    let structured_startup_text = startup_turn_text.clone();
    let public_send_key = public_send::arm(&svc, &tab_id, &agent_cmd, startup_turn_text);
    // thread spawns: callers get the tab id immediately and may send right away.
    let authenticated = svc
        .mux
        .read()
        .get_tab(&tab_id)
        .is_some_and(|tab| tab.launch_session_authenticated());
    let reserved_nonce = (!authenticated)
        .then(|| generation::reserve_binding_nonce(&svc, &tab_id, resume_session_id.as_deref()));
    mark_tab_agent_launch_started(&svc.mux, &tab_id);
    crate::service::boot_wait::notify_boot_changed();
    let mut launch = LaunchGeneration::begin(&tab_id, "pending".into());
    std::thread::spawn(move || {
        let pty_claim = svc.pty_ownership.acquire(&tab_id);
        crate::service::agent_restart::timing::stage(&tab_id, "pending", "relaunch-worker");
        let public_send_guard = public_send::guard(&svc, &tab_id, public_send_key);
        let mut delivery_gate = crate::service::launch_delivery::LaunchDeliveryGate::new();
        if !launch.emit(&svc, BootStage::PtyStarting) {
            return;
        }
        let preparation = preparation::start(&svc, &tab_id, &agent_cmd, launch_cmd);
        let set_booting = |launch: &LaunchGeneration, value: bool| {
            if !value
                && crate::service::codex_rpc::lifecycle::for_tab(&tab_id).is_some_and(|sidecar| {
                    crate::service::codex_rpc::recovery::recovering(&tab_id, &sidecar.launch_nonce)
                })
            {
                return;
            }
            launch.commit_if_current(|| {
                if let Some(p) = svc.mux.write().get_tab_mut(&tab_id) {
                    p.agent_booting = value;
                }
            });
            crate::service::boot_wait::notify_boot_changed();
        };
        let existing_pty = existing_live_pty(&svc, &tab_id);
        let fresh_pty = svc.model_provider_requires_fresh_pty(&agent_cmd)
            || crate::service::codex_rpc::launch::requested(&svc, &agent_cmd);
        let reusable_pty = existing_pty.clone().filter(|_| !fresh_pty);
        let (backend_pty, reused_live_pty) = match reusable_pty {
            Some(pty_id) => match start_backend_reader(&svc, &pty_id) {
                Ok(reader) => (BackendPty { id: pty_id, reader }, true),
                Err(failure) => {
                    let error = failure.reason();
                    launch.fail(&svc, failure);
                    set_booting(&launch, false);
                    if let Some(tx) = ready_tx {
                        let _ = tx.send(Err(error));
                    }
                    return;
                }
            },
            None => match spawn_backend_pty(&svc, &tab_id, &launch_env, &agent_cmd) {
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
                    set_booting(&launch, false);
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
        let selection = svc.link_provider_launch_pty(
            &pty_claim,
            &tab_id,
            candidate_pty_id,
            existing_pty.as_deref(),
            fresh_pty,
        );
        if !selection.linked {
            let error = LaunchFailure::TabClosed.reason();
            launch.fail(&svc, LaunchFailure::TabClosed);
            set_booting(&launch, false);
            let _ = svc.close_tab_with_reason(&tab_id, TabCloseReason::SpawnRollback);
            if let Some(tx) = ready_tx {
                let _ = tx.send(Err(error));
            }
            return;
        }
        let mut pty_id = selection.pty_id;
        if !launch.replace(pty_id.clone()) {
            return;
        }
        if !launch.emit(&svc, BootStage::PtySpawned) {
            return;
        }
        crate::service::agent_restart::timing::stage(&tab_id, &pty_id, "relaunch-spawn");
        // Off the boot critical path: reading the provider's install report
        // touches disk, and the tab is already up by here.
        svc.announce_integration_silence(&tab_id, &agent_cmd);

        let mut reader_monitor = match reader_readiness.wait(std::time::Duration::from_secs(10)) {
            Ok(monitor) => monitor,
            Err(failure) => {
                let error = failure.reason();
                launch.fail(&svc, failure);
                set_booting(&launch, false);
                if let Some(tx) = ready_tx {
                    let _ = tx.send(Err(error));
                }
                return;
            }
        };
        if !launch.emit(&svc, BootStage::ReaderReady) {
            return;
        }

        let launch_cmd = match preparation.finish() {
            Ok(command) => command,
            Err(reason) => {
                launch.fail(&svc, LaunchFailure::LaunchWrite(reason.clone()));
                set_booting(&launch, false);
                if let Some(tx) = ready_tx {
                    let _ = tx.send(Err(reason));
                }
                return;
            }
        };
        let structured = crate::service::codex_rpc::lifecycle::for_tab(&tab_id).is_some();
        let binding_turn = if structured {
            None
        } else {
            resume_binding::for_launch(
                &svc,
                &tab_id,
                &agent_cmd,
                origin,
                has_startup_task_turn || fallback_spec.is_some(),
                resume_session_id.as_deref(),
            )
        };
        let binding_turn_planned = binding_turn.is_some();
        let fallback_spec = fallback_spec.or(binding_turn).or_else(|| {
            (structured && has_startup_task_turn)
                .then_some(structured_startup_text)
                .flatten()
                .map(
                    |payload| crate::service::launch_delivery::FallbackDeliverySpec {
                        channel:
                            crate::multiplexer::types::PendingDeliveryChannel::InteractiveInput,
                        payload,
                    },
                )
        });
        let has_startup_task_turn = has_startup_task_turn || fallback_spec.is_some();
        let session_starts_on_first_turn = !structured
            && (binding_turn_planned
                || startup_readiness::launch_waits_for_first_turn_binding(
                    &svc,
                    &tab_id,
                    &agent_cmd,
                    has_startup_task_turn,
                    origin,
                ));

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
        let exited_provider_recovery = matches!(
            origin,
            crate::service::launch_diagnostics::AgentLaunchOrigin::ExitRecovery
                | crate::service::launch_diagnostics::AgentLaunchOrigin::AutomaticExitRecovery
        );
        if !exited_provider_recovery
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
                set_booting(&launch, false);
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
                set_booting(&launch, false);
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
                set_booting(&launch, false);
                if let Some(tx) = ready_tx {
                    let _ = tx.send(Err(error));
                }
                return;
            }
            set_booting(&launch, false);
            LAUNCH_REGISTRY.lock().finish(&launch);
            if let Some(tx) = ready_tx {
                let _ = tx.send(Ok(()));
            }
            return;
        }
        crate::service::task_files::retire_settings_for_launch(&svc, &tab_id, &launch_cmd);
        if let Err(error) = launch.capture_binding_nonce(
            &svc,
            task_identity.as_ref(),
            public_send_key,
            origin,
            resume_session_id.as_deref(),
            reserved_nonce,
        ) {
            let failure = LaunchFailure::StartupSupervision(error.clone());
            launch.fail(&svc, failure);
            set_booting(&launch, false);
            if let Some(tx) = ready_tx {
                let _ = tx.send(Err(error));
            }
            return;
        }
        drop(pty_claim);
        let mut wrapped_launch_cmd = command_with_binding_nonce(
            &launch_cmd,
            launch
                .binding_nonce
                .as_deref()
                .expect("launch binding nonce"),
            &launch.pty_id,
        );

        let shell_ready_from_exit =
            exited_provider_recovery_has_shell_ready_evidence(origin, reused_live_pty, || {
                svc.ext::<crate::ShellCapabilityState>()
                    .lock()
                    .prompt_idle_after_command(&pty_id)
            });
        if shell_ready_from_exit {
            log::info!(
                "launch_agent: Tab {} exit recovery reuses the shell proven available by provider exit on PTY {}",
                tab_id,
                pty_id
            );
        }
        if !shell_ready_from_exit
            && !crate::service::pty_io::await_bare_shell_prompt(
                &svc.screen_state,
                &pty_id,
                std::time::Duration::from_secs(30),
            )
        {
            let error = LaunchFailure::ShellReadyTimeout.reason();
            launch.fail(&svc, LaunchFailure::ShellReadyTimeout);
            set_booting(&launch, false);
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
            set_booting(&launch, false);
            if let Some(tx) = ready_tx {
                let _ = tx.send(Err(error));
            }
            return;
        }
        // (#93 — keeps shell-level launch off the bracketed-paste path).
        let first_launch = match launch.write_if_current(|| {
            svc.send_launch_command_to_pty_after_ready(&pty_id, &wrapped_launch_cmd)
        }) {
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
                        let replacement_claim = svc.pty_ownership.acquire(&tab_id);
                        if !launch.replace(pty_id.clone()) {
                            return;
                        }
                        let replacement_reader = match start_backend_reader(&svc, &pty_id) {
                            Ok(reader) => reader,
                            Err(failure) => {
                                let error = failure.reason();
                                launch.fail(&svc, failure);
                                set_booting(&launch, false);
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
                                    set_booting(&launch, false);
                                    if let Some(tx) = ready_tx {
                                        let _ = tx.send(Err(error));
                                    }
                                    return;
                                }
                            };
                        if let Err(error) = launch.capture_binding_nonce(
                            &svc,
                            task_identity.as_ref(),
                            public_send_key,
                            origin,
                            resume_session_id.as_deref(),
                            None,
                        ) {
                            let failure = LaunchFailure::StartupSupervision(error.clone());
                            launch.fail(&svc, failure);
                            set_booting(&launch, false);
                            if let Some(tx) = ready_tx {
                                let _ = tx.send(Err(error));
                            }
                            return;
                        }
                        drop(replacement_claim);
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
                            set_booting(&launch, false);
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
                            svc.send_launch_command_to_pty_after_ready(&pty_id, &wrapped_launch_cmd)
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
            set_booting(&launch, false);
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
        if !launch.await_remote_attachment(&svc) {
            set_booting(&launch, false);
            if let Some(tx) = ready_tx {
                let _ = tx.send(Err(
                    "remote TUI attachment failed; sidecar retained for reattachment".into(),
                ));
            }
            return;
        }
        if let Some(tree) = svc.observe_provider_process_tree(&tab_id, &pty_id) {
            log::info!(
                "[launch-readiness] observed verdict_source=launch-written tree_evidence={} tab={} pty={}",
                tree.evidence(),
                tab_id,
                pty_id,
            );
        } else {
            log::info!(
                "[launch-readiness] observed verdict_source=launch-written tree_evidence=provider_tree_unavailable tab={} pty={}",
                tab_id,
                pty_id,
            );
        }

        if reader_monitor.closed() {
            let error = LaunchFailure::PtyEof.reason();
            launch.fail(&svc, LaunchFailure::PtyEof);
            set_booting(&launch, false);
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
        crate::service::launch_readiness::arm_with_origin(readiness_attempt.clone(), origin);
        if launch.await_provider_ack(std::time::Duration::ZERO) {
            crate::service::launch_readiness::note_acknowledged(
                &readiness_attempt.tab_id,
                &readiness_attempt.pty_id,
                &readiness_attempt.binding_nonce,
            );
        }
        let startup_binding_deadline = crate::service::worker_ack::STARTUP_BINDING_DEADLINE;
        let binding_deadline = std::time::Instant::now() + startup_binding_deadline;
        let readiness_receiver = (!structured).then(|| {
            svc.observe_launch_readiness(
                readiness_attempt.clone(),
                startup_binding_deadline,
                session_starts_on_first_turn,
            )
        });

        let (provider_readiness, composer_cursor_position) = svc
            .agent_registry
            .with_provider(&agent_cmd, |provider| {
                (
                    provider.provider_readiness_strategy(),
                    provider.composer_cursor_position(),
                )
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
        let user_composer_required = user_input::requires_composer(&svc, &agent_cmd, &tab_id);
        let launch_input_readiness = if !structured
            && (fallback_spec.is_some() || session_starts_on_first_turn || user_composer_required)
        {
            let readiness = owner_prompt_wait::await_input_readiness(
                &svc,
                &tab_id,
                &pty_id,
                &agent_cmd,
                &readiness_attempt,
                session_starts_on_first_turn || user_composer_required,
                input_readiness_timeout,
                provider_readiness,
                composer_cursor_position,
            );
            match readiness {
                Ok(readiness) => Some(readiness),
                Err(refusal) => {
                    let failure = LaunchFailure::ProviderInputNotReady(refusal.reason());
                    launch.fail(&svc, failure.clone());
                    set_booting(&launch, false);
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
            user_input::observe(&svc, &agent_cmd, &readiness_attempt, readiness);
        }
        if session_starts_on_first_turn && launch_input_readiness.is_none() {
            let failure = LaunchFailure::ProviderInputNotReady(
                "provider composer evidence was not observed before the readiness deadline".into(),
            );
            launch.fail(&svc, failure.clone());
            set_booting(&launch, false);
            if let Some(tx) = ready_tx {
                let _ = tx.send(Err(failure.reason()));
            }
            return;
        }
        if session_starts_on_first_turn {
            set_booting(&launch, false);
            if !launch.emit_first_turn_input_ready(&svc) {
                return;
            }
        }
        delivery_gate.mark_provider_ready();

        let fallback_spec = fallback_spec.filter(|_| {
            resume_binding::await_fallback_required(&launch, binding_turn_planned, binding_deadline)
        });
        let mut bootstrap_consumed = fallback_spec.is_none();
        let bootstrap_draft = fallback_spec.as_ref().map(|spec| spec.payload.clone());
        if let Some(spec) = fallback_spec {
            set_booting(&launch, false);
            let delivery_error = match bootstrap::consume(
                &svc,
                &readiness_attempt,
                &agent_cmd,
                spec,
                &delivery_gate,
                launch_input_readiness,
                binding_turn_planned,
            ) {
                Ok(consumed) => {
                    bootstrap_consumed = consumed;
                    None
                }
                Err(error) => Some(error),
            };
            if let Some(e) = delivery_error {
                let (title, body) = worker_brief_delivery_incident(&tab_id, &e);
                svc.log_system_notice(title, Some(body), Some("error"));
                log::error!(
                    "launch_agent: task delivery failed for Tab {} (pty {}): {}",
                    tab_id,
                    pty_id,
                    e
                );
                launch.fail(&svc, LaunchFailure::TaskDelivery(e.to_string()));
                set_booting(&launch, false);
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

        set_booting(&launch, false);
        let readiness = if structured {
            crate::service::launch_readiness::LaunchSettlement::Ready
        } else {
            readiness_receiver
                .expect("plain launch readiness observer")
                .recv()
                .unwrap_or_else(|_| {
                    crate::service::launch_readiness::LaunchSettlement::Provisional {
                        stage: BootStage::LaunchDeadlineElapsed,
                        reason: "launch readiness observer stopped before session binding".into(),
                    }
                })
        };
        if let Some(finished) =
            startup_readiness::finish_waiting_for_first_turn(&mut launch, &readiness, &svc, origin)
        {
            if finished {
                public_send_guard.complete();
                launch.complete_structured_startup(&svc, bootstrap_consumed);
                if let Some(tx) = ready_tx {
                    let _ = tx.send(Ok(()));
                }
            } else if let Some(tx) = ready_tx {
                let _ = tx.send(Err(
                    "launch did not establish its authenticated session binding".into(),
                ));
            }
            return;
        }
        if let Some(draft) = bootstrap_draft.as_deref() {
            if bootstrap::unconfirmed_draft_nudge_due(
                bootstrap_consumed,
                session_starts_on_first_turn,
                &readiness,
                launch.await_provider_ack(std::time::Duration::ZERO),
            ) {
                bootstrap::nudge_unconfirmed_draft(&svc, &readiness_attempt, draft, |wait| {
                    launch.await_provider_ack(wait)
                });
            }
        }
        if !session_starts_on_first_turn {
            let session_authenticated = launch.await_provider_ack(std::time::Duration::ZERO);
            if let Some(stage) =
                startup_readiness::provider_ready_stage(&readiness, session_authenticated)
            {
                if !launch.emit(&svc, stage) {
                    return;
                }
            }
        }
        if let Err(error) = startup_readiness::emit_agent_ready(&mut launch, &svc, readiness) {
            if let Some(tx) = ready_tx {
                let _ = tx.send(Err(error));
            }
            return;
        }
        LAUNCH_REGISTRY.lock().finish(&launch);
        public_send_guard.complete();
        launch.complete_structured_startup(&svc, bootstrap_consumed);
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

pub fn mark_tab_agent_launch_started(mux: &crate::multiplexer::commands::MuxState, tab_id: &str) {
    if let Some(tab) = mux.write().get_tab_mut(tab_id) {
        tab.restored_from_snapshot = false;
        tab.agent_booting = true;
        tab.restorable_launch.failed_provider = None;
    }
}

fn agent_role_label(
    role: crate::multiplexer::types::AgentRole,
    role_profile: Option<&str>,
) -> &'static str {
    codeterm_types::agent::role_kind::kind_label(Some(role), role_profile)
        .unwrap_or_else(|| role.label())
}
