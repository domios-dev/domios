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

pub(crate) use crate::agent::provider::{AgentLaunchParams, TaskDelivery};
pub(crate) use crate::agent::registry::AgentRegistry;

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

fn spawn_backend_pty(svc: &ServiceContext, pane_id: &str) -> Result<String, String> {
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
        .map_err(|error| error.to_string())?;

    let svc_arc = std::sync::Arc::new(svc.clone());
    let prepared = crate::service::pty_lifecycle::prepare_backend_reader(&svc_arc, &pty_id);
    if let Some(orphan) = svc.mux.write().link_pty(pane_id, pty_id.clone()) {
        svc.kill_ptys(&[orphan]);
    }
    super::pane_relink_signal::notify_pane_relinked();

    match prepared {
        Ok(Some(prepared)) => {
            let reader_pty_id = pty_id.clone();
            std::thread::spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        log::error!(
                            "launch_agent: backend reader runtime failed for {}: {}",
                            reader_pty_id,
                            error
                        );
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
                    let _ = reader.await;
                });
            });
        }
        Ok(None) => {}
        Err(error) => {
            log::warn!(
                "launch_agent: backend reader prepare failed for {}: {}",
                pane_id,
                error
            );
        }
    }

    Ok(pty_id)
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
        let set_booting = |value: bool| {
            if let Some(p) = svc.mux.write().get_pane_mut(&pane_id) {
                p.agent_booting = value;
            }
        };
        let mut pty_id = match existing_live_pty(&svc, &pane_id) {
            Some(pty_id) => pty_id,
            None => match spawn_backend_pty(&svc, &pane_id) {
                Ok(pty_id) => pty_id,
                Err(error) => {
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
        svc.emit_boot_stage(&pane_id, BootStage::PtySpawned, None);
        log::info!(
            "launch_agent: pty ready for pane {} after {}ms",
            pane_id,
            launch_started.elapsed().as_millis()
        );

        let delivery = registry
            .with_provider(&agent_cmd, |provider| provider.task_delivery())
            .unwrap_or(TaskDelivery::PtyInput);
        if !crate::service::pty_io::await_bare_shell_prompt(
            &svc.screen_state,
            &pty_id,
            std::time::Duration::from_secs(30),
        ) {
            let error = "bare shell prompt did not become ready".to_string();
            svc.emit_boot_stage(&pane_id, BootStage::SpawnFailed, Some(&error));
            set_booting(false);
            if let Some(tx) = ready_tx {
                let _ = tx.send(Err(error));
            }
            return;
        }
        svc.emit_boot_stage(&pane_id, BootStage::ShellReady, None);
        log::info!(
            "launch_agent: shell ready for pane {} after {}ms",
            pane_id,
            launch_started.elapsed().as_millis()
        );

        // 3. Send launch command after the shell readiness event from step 2.
        let launch_opts = crate::service::inject_verify::VerifyOpts {
            require_prompt_first: false,
            verify_echo: true,
            verify_submit: true,
            prompt_timeout: std::time::Duration::from_secs(0),
            echo_timeout: std::time::Duration::from_secs(3),
            submit_timeout: std::time::Duration::from_secs(15),
        };
        // Use the provider's submit strategy: TUI providers (claude/codex/opencode) hand the shell off to a TUI on launch, so a "new shell prompt"
        // never appears — TuiInputDrained is the right signal there.
        let launch_strategy = registry
            .with_provider(&agent_cmd, |p| p.verify_submit_strategy())
            .unwrap_or(crate::agent::provider::VerifySubmitStrategy::ShellPrompt {
                timeout: std::time::Duration::from_secs(15),
            });
        svc.emit_boot_stage(&pane_id, BootStage::AgentLaunching, None);
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
            svc.emit_boot_stage(
                &pane_id,
                BootStage::SpawnFailed,
                Some(&format!("launch_cmd failed: {}", e)),
            );
            set_booting(false);
            if let Some(tx) = ready_tx {
                let _ = tx.send(Err(format!("launch_cmd failed: {}", e)));
            }
            return;
        }

        if !svc.await_agent_tui_on_screen(&pty_id, &agent_cmd, std::time::Duration::from_secs(30)) {
            let error = format!("{} TUI did not become ready", agent_cmd);
            svc.emit_boot_stage(&pane_id, BootStage::SpawnFailed, Some(&error));
            set_booting(false);
            if let Some(tx) = ready_tx {
                let _ = tx.send(Err(error));
            }
            return;
        }

        // 4. For PtyInput delivery, send task after the provider reports a ready TUI.
        let is_pty_input = matches!(delivery, TaskDelivery::PtyInput);
        if is_pty_input {
            if let Some(ref task_text) = task {
                set_booting(false);
                // TUI is visible; use the fast message path without waiting for
                // this launch thread's own boot flag to clear.
                if let Err(e) = svc.send_text_to_pty_smart(&pty_id, task_text, true) {
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
    use super::{replacement_pty_candidate, spawn_onboarding_task};
    use crate::chatbackend::{ChatBackendError, ChatBackendProvider, ChatBackendRegistry};
    use crate::multiplexer::state::MultiplexerState;
    use crate::multiplexer::types::AgentRole;
    use crate::service::orchestrator::types::SpawnAgentParams;
    use crate::service::plan::PlanStore;
    use crate::service::spawn::{PaneCommand, SpawnPaneSpec};
    use crate::service::test_support::test_harness;
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
    fn plain_orchestrator_no_task_spawn_submits_nothing() {
        assert_eq!(spawn_onboarding_task(&orchestrator_params(None)), None);
    }

    #[test]
    fn plain_orchestrator_no_task_attaches_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let (mut mux, _) = MultiplexerState::new();
        let workspace_id = mux.list_workspaces()[0].id.clone();
        let pane_id = mux.create_tab(&workspace_id).unwrap().1;
        mux.set_pane_role(&pane_id, AgentRole::Orchestrator, None)
            .unwrap();
        let plan_store = PlanStore::new_for_test(dir.path());

        assert!(plan_store.task_binding_for_pane(&pane_id).is_none());
        assert_eq!(mux.team_count(), 0);
    }

    #[test]
    fn plain_orchestrator_role_metadata_uses_the_non_task_channel() {
        let dir = tempfile::tempdir().unwrap();
        let harness = test_harness(dir.path(), "test-provider");
        let pane_id = {
            let mut mux = harness.service.mux.write();
            let workspace_id = mux.list_workspaces()[0].id.clone();
            let pane_id = mux.create_tab(&workspace_id).unwrap().1;
            mux.set_pane_role(&pane_id, AgentRole::Orchestrator, None)
                .unwrap();
            pane_id
        };

        assert_eq!(
            harness.service.pane_env(&pane_id).get("CODETERM_ROLE"),
            Some(&"Orchestrator".to_string())
        );
        std::mem::forget(harness);
    }

    #[test]
    fn explicit_orchestrator_task_is_submitted_exactly_once() {
        assert_eq!(
            spawn_onboarding_task(&orchestrator_params(Some("inspect launch"))),
            Some("inspect launch".into())
        );
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
            if let Some(effort_args) =
                registry.with_provider(agent_cmd, |p| p.reasoning_effort_launch_args(effort))
            {
                args.extend(effort_args);
            }
        }
    }

    let launch_params = AgentLaunchParams {
        agent_name: params.agent_name.clone(),
        task: task.map(|t| t.to_string()),
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
    crate::service::launch_diagnostics::log_launch("visible_agent", "-", agent_cmd, &cmd, task);
    cmd
}
