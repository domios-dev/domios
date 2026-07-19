use super::types::SpawnAgentParams;
use crate::service::boot_stage::BootStage;
use crate::service::ServiceContext;

/// A cross-mesh worker (mesh report target set, no local parent) must NOT be
/// forced into the local Worker hierarchy — it has no local Orchestrator parent,
/// so `set_pane_role(Worker, None)` errors. Its hierarchy is represented on the
/// origin via the RemoteWorkerRegistry instead. Returns true to skip the local
/// role assignment. Orchestrator-role and locally-parented spawns are unaffected.
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

pub(crate) use crate::agent::provider::AgentLaunchParams;
pub(crate) use crate::agent::registry::AgentRegistry;

static NEXT_LAUNCH_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq)]
enum LaunchFailure {
    PtySpawn(String),
    ReaderSetup(String),
    ReaderRuntime(String),
    ReaderReadyTimeout,
    PtyEof,
    PaneClosed,
    LaunchWrite(String),
    ProviderReadyTimeout,
}

impl LaunchFailure {
    fn reason(&self) -> String {
        match self {
            Self::PtySpawn(error) => format!("PtySpawnFailed: {error}"),
            Self::ReaderSetup(error) => format!("ReaderSetupFailed: {error}"),
            Self::ReaderRuntime(error) => format!("ReaderRuntimeFailed: {error}"),
            Self::ReaderReadyTimeout => "ReaderReadyTimeout".into(),
            Self::PtyEof => "PtyEof".into(),
            Self::PaneClosed => "PaneClosedBeforePtyLink".into(),
            Self::LaunchWrite(error) => format!("LaunchWriteFailed: {error}"),
            Self::ProviderReadyTimeout => "ProviderReadyTimeout".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LaunchGeneration {
    pane_id: String,
    pty_id: String,
    generation: u64,
    stage: BootStage,
    reused: bool,
}

impl LaunchGeneration {
    fn new(pane_id: &str, pty_id: String) -> Self {
        Self {
            pane_id: pane_id.into(),
            pty_id,
            generation: NEXT_LAUNCH_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            stage: BootStage::PtySpawned,
            reused: false,
        }
    }

    fn transition(&mut self, stage: BootStage) {
        self.stage = stage;
    }

    fn reuse(&mut self) {
        self.reused = true;
        self.transition(BootStage::AgentReady);
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

/// Shared background thread logic for launching agents in a PTY.
/// Uses the AgentRegistry to determine readiness strategy and task delivery.
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
    svc.emit_boot_stage(&pane_id, BootStage::PtyStarting, None);
    std::thread::spawn(move || {
        let mut delivery_gate = crate::service::launch_delivery::LaunchDeliveryGate::new();
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
                    svc.emit_boot_stage(&pane_id, BootStage::SpawnFailed, Some(&error));
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
                    svc.emit_boot_stage(
                        &pane_id,
                        BootStage::SpawnFailed,
                        Some(&format!("backend PTY spawn failed: {}", error)),
                    );
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
            svc.emit_boot_stage(&pane_id, BootStage::SpawnFailed, Some(&error));
            set_booting(false);
            let _ = svc.close_tab(&ws_id, &tab_id);
            if let Some(tx) = ready_tx {
                let _ = tx.send(Err(error));
            }
            return;
        }
        let mut pty_id = selection.pty_id;
        svc.emit_boot_stage(&pane_id, BootStage::PtySpawned, None);
        let mut launch = LaunchGeneration::new(&pane_id, pty_id.clone());
        log::info!(
            "launch_agent: pty ready for pane {} after {}ms",
            pane_id,
            launch_started.elapsed().as_millis()
        );

        let reader_monitor = match reader_readiness.wait(std::time::Duration::from_secs(10)) {
            Ok(monitor) => monitor,
            Err(failure) => {
                let error = failure.reason();
                svc.emit_boot_stage(&pane_id, BootStage::SpawnFailed, Some(&error));
                set_booting(false);
                if let Some(tx) = ready_tx {
                    let _ = tx.send(Err(error));
                }
                return;
            }
        };
        launch.transition(BootStage::ReaderReady);
        svc.emit_boot_stage(&pane_id, launch.stage, None);
        launch.transition(BootStage::ShellReady);
        svc.emit_boot_stage(&pane_id, launch.stage, None);
        delivery_gate.mark_shell_ready();

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
            svc.emit_boot_stage(&pane_id, launch.stage, None);
            set_booting(false);
            if let Some(tx) = ready_tx {
                let _ = tx.send(Ok(()));
            }
            return;
        }

        let delivery = registry
            .with_provider(&agent_cmd, |provider| {
                crate::service::launch_delivery::mode_for_descriptor(
                    &provider.system_prompt_delivery(),
                )
            })
            .unwrap_or(crate::service::launch_delivery::LaunchDeliveryMode::ConfirmedFallback);
        // The launch write is independent of shell-prompt rendering. Reader ownership
        // proves the headless shell channel is primed; prompt heuristics cannot fail launch.
        let launch_opts = crate::service::inject_verify::VerifyOpts {
            require_prompt_first: false,
            verify_echo: true,
            verify_submit: true,
            prompt_timeout: std::time::Duration::from_secs(0),
            echo_timeout: std::time::Duration::from_secs(3),
            submit_timeout: std::time::Duration::from_secs(30),
        };
        // A TUI launch hands the shell off before a new shell prompt can appear.
        let launch_strategy = registry
            .with_provider(&agent_cmd, |p| p.verify_submit_strategy())
            .unwrap_or(crate::agent::provider::VerifySubmitStrategy::ShellPrompt {
                timeout: std::time::Duration::from_secs(15),
            });
        launch.transition(BootStage::AgentLaunching);
        svc.emit_boot_stage(&pane_id, launch.stage, None);
        if reader_monitor.closed() {
            let error = LaunchFailure::PtyEof.reason();
            svc.emit_boot_stage(&pane_id, BootStage::SpawnFailed, Some(&error));
            set_booting(false);
            if let Some(tx) = ready_tx {
                let _ = tx.send(Err(error));
            }
            return;
        }
        // (#93 — keeps shell-level launch off the bracketed-paste path).
        let first_launch = svc.send_shell_spawn_verified(
            &pty_id,
            &agent_cmd,
            &launch_cmd,
            launch_strategy.clone(),
            launch_opts.clone(),
        );
        let launch_result = match first_launch {
            Err(error @ crate::service::inject_verify::InjectionError::PtyClosed { .. }) => {
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
                        svc.send_shell_spawn_verified(
                            &pty_id,
                            &agent_cmd,
                            &launch_cmd,
                            launch_strategy,
                            launch_opts,
                        )
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
            let error = LaunchFailure::LaunchWrite(e.to_string()).reason();
            svc.emit_boot_stage(&pane_id, BootStage::SpawnFailed, Some(&error));
            set_booting(false);
            if let Some(tx) = ready_tx {
                let _ = tx.send(Err(error));
            }
            return;
        }
        launch.transition(BootStage::LaunchWritten);
        svc.emit_boot_stage(&pane_id, launch.stage, None);

        if !svc.await_agent_tui_frame_on_screen(
            &pty_id,
            &agent_cmd,
            std::time::Duration::from_secs(30),
        ) {
            let error = if reader_monitor.closed() {
                LaunchFailure::PtyEof.reason()
            } else {
                LaunchFailure::ProviderReadyTimeout.reason()
            };
            svc.emit_boot_stage(&pane_id, BootStage::SpawnFailed, Some(&error));
            set_booting(false);
            if let Some(tx) = ready_tx {
                let _ = tx.send(Err(error));
            }
            return;
        }
        launch.transition(BootStage::ProviderReady);
        svc.emit_boot_stage(&pane_id, launch.stage, None);
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
                    svc.emit_boot_stage(
                        &pane_id,
                        BootStage::SpawnFailed,
                        Some(&format!("task delivery failed: {}", e)),
                    );
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
        svc.emit_boot_stage(&pane_id, BootStage::AgentReady, None);
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
        spawn_onboarding_task, AgentRegistry, LaunchFailure, LaunchGeneration,
    };
    use crate::chatbackend::{ChatBackendError, ChatBackendProvider, ChatBackendRegistry};
    use crate::multiplexer::state::MultiplexerState;
    use crate::multiplexer::types::AgentRole;
    use crate::service::boot_stage::BootStage;
    use crate::service::orchestrator::types::SpawnAgentParams;
    use crate::service::pty_io::pane_identity_env;
    use crate::service::spawn::{PaneCommand, SpawnPaneSpec};
    use std::sync::Arc;

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
    fn launch_generation_preserves_reader_launch_and_provider_transitions() {
        let mut launch = LaunchGeneration::new("pane-a", "pty-a".into());
        let generation = launch.generation;

        launch.transition(BootStage::ReaderReady);
        launch.transition(BootStage::AgentLaunching);
        launch.transition(BootStage::LaunchWritten);
        launch.transition(BootStage::ProviderReady);
        launch.transition(BootStage::AgentReady);

        assert_eq!(launch.pane_id, "pane-a");
        assert_eq!(launch.pty_id, "pty-a");
        assert!(generation > 0);
        assert_eq!(launch.stage, BootStage::AgentReady);
        assert!(!launch.reused);
    }

    #[test]
    fn live_provider_tui_is_reused_without_a_spawn_failure() {
        let mut launch = LaunchGeneration::new("pane-a", "pty-a".into());

        launch.reuse();

        assert!(launch.reused);
        assert_eq!(launch.stage, BootStage::AgentReady);
        assert_ne!(launch.stage, BootStage::SpawnFailed);
    }

    #[test]
    fn reader_and_provider_failures_are_causal() {
        assert_eq!(
            LaunchFailure::ReaderSetup("closed".into()).reason(),
            "ReaderSetupFailed: closed"
        );
        assert_eq!(
            LaunchFailure::ProviderReadyTimeout.reason(),
            "ProviderReadyTimeout"
        );
        assert_eq!(LaunchFailure::PtyEof.reason(), "PtyEof");
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
