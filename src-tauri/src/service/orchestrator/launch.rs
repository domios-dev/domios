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
        (Some("Orchestrator"), _) => Some(format!(
            "{}{}",
            task.unwrap_or_default(),
            crate::prompts::orchestrator_suffix()
        )),
        _ => task.map(str::to_string),
    }
}

pub(crate) use crate::agent::provider::{AgentLaunchParams, ReadinessStrategy, TaskDelivery};
pub(crate) use crate::agent::registry::AgentRegistry;

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
    // thread spawns: callers get the pane id immediately and may send right away.
    if let Some(p) = svc.mux.write().get_pane_mut(&pane_id) {
        p.agent_booting = true;
    }
    std::thread::spawn(move || {
        let set_booting = |value: bool| {
            if let Some(p) = svc.mux.write().get_pane_mut(&pane_id) {
                p.agent_booting = value;
            }
        };
        // 1. Wait for PTY (frontend creates it after mux-state-changed). If frontend doesn't create it (cron/headless tabs), spawn from backend.
        let frontend_deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let pty_id = loop {
            if let Some(pid) = svc
                .mux
                .try_write()
                .and_then(|m| m.get_pane(&pane_id).and_then(|p| p.pty_id.clone()))
            {
                break pid;
            }
            if std::time::Instant::now() > frontend_deadline {
                // Frontend didn't create PTY — spawn from backend (cron/headless)
                log::info!("launch_agent: backend PTY spawn for {}", pane_id);
                let cwd = svc
                    .mux
                    .write()
                    .get_pane(&pane_id)
                    .and_then(|p| p.cwd.clone());
                let mut env_vars: std::collections::HashMap<String, String> =
                    std::collections::HashMap::new();
                env_vars.insert("CODETERM_PANE_ID".into(), pane_id.clone());
                env_vars.insert(
                    "CODETERM_SESSION_BINDING_NONCE".into(),
                    svc.ensure_session_binding_nonce(&pane_id),
                );
                env_vars.insert(
                    "CODETERM_SOCKET_PATH".into(),
                    crate::socket::server::socket_display_path(),
                );
                let api_cfg = crate::config::load_config().api;
                let host = if api_cfg.host == "0.0.0.0" {
                    "127.0.0.1".to_string()
                } else {
                    api_cfg.host.clone()
                };
                env_vars.insert(
                    "CODETERM_API_URL".into(),
                    format!("http://{}:{}", host, api_cfg.port),
                );
                if let Some(ref tok) = api_cfg.token {
                    env_vars.insert("CODETERM_API_TOKEN".into(), tok.clone());
                }
                let spawn_result = svc.pty.lock().spawn(None, cwd, 120, 40, env_vars);
                match spawn_result {
                    Ok((pid, _)) => {
                        let svc_arc = std::sync::Arc::new(svc.clone());
                        let prepared =
                            crate::service::pty_lifecycle::prepare_backend_reader(&svc_arc, &pid);
                        if let Some(orphan) = svc.mux.write().link_pty(&pane_id, pid.clone()) {
                            svc.kill_ptys(&[orphan]);
                        }
                        match prepared {
                            Ok(Some(prepared)) => {
                                let pid2 = pid.clone();
                                std::thread::spawn(move || {
                                    let rt = match tokio::runtime::Builder::new_current_thread()
                                        .enable_all()
                                        .build()
                                    {
                                        Ok(rt) => rt,
                                        Err(e) => {
                                            log::error!(
                                                "launch_agent: backend reader runtime failed for {}: {}",
                                                pid2,
                                                e
                                            );
                                            return;
                                        }
                                    };
                                    rt.block_on(async {
                                        let h = crate::service::pty_lifecycle::spawn_prepared_backend_reader(
                                            &svc_arc, &pid2, prepared, true,
                                        );
                                        let _ = h.await;
                                    });
                                });
                            }
                            Ok(None) => {}
                            Err(e) => {
                                log::warn!(
                                    "launch_agent: backend reader prepare failed for {}: {}",
                                    pane_id,
                                    e
                                );
                            }
                        }
                        break pid;
                    }
                    Err(e) => {
                        log::error!(
                            "launch_agent: backend PTY spawn failed for pane {}: {}",
                            pane_id,
                            e
                        );
                        svc.emit_boot_stage(
                            &pane_id,
                            BootStage::SpawnFailed,
                            Some(&format!("backend PTY spawn failed: {}", e)),
                        );
                        let _ = svc.close_tab(&ws_id, &tab_id);
                        set_booting(false);
                        if let Some(tx) = ready_tx {
                            let _ = tx.send(Err(format!("backend PTY spawn failed: {}", e)));
                        }
                        return;
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(300));
        };
        svc.emit_boot_stage(&pane_id, BootStage::PtySpawned, None);

        let (readiness, delivery) = registry
            .with_provider(&agent_cmd, |p| (p.readiness_strategy(), p.task_delivery()))
            .unwrap_or((
                ReadinessStrategy::FixedDelay {
                    duration: std::time::Duration::from_secs(2),
                },
                TaskDelivery::PtyInput,
            ));

        match readiness {
            ReadinessStrategy::PollShellPrompt { timeout } => {
                crate::service::pty_io::poll_shell_prompt(&svc.screen_state, &pty_id, timeout);
            }
            ReadinessStrategy::FixedDelay { duration } => {
                std::thread::sleep(duration);
            }
        }
        svc.emit_boot_stage(&pane_id, BootStage::ShellReady, None);

        // 3. Send launch command (verified). Shell prompt was already polled in step 2, so require_prompt_first=false avoids a redundant wait.
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
        if let Err(e) =
            svc.send_shell_spawn_verified(&pty_id, &launch_cmd, launch_strategy, launch_opts)
        {
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

        // 4. For PtyInput delivery, send task immediately — verified send already gated on receiver-side completion of the launch command.
        let is_pty_input = matches!(delivery, TaskDelivery::PtyInput);
        if is_pty_input {
            if let Some(ref task_text) = task {
                let warmup = registry
                    .with_provider(&agent_cmd, |p| p.task_input_warmup())
                    .unwrap_or_default();
                if !warmup.is_zero() {
                    std::thread::sleep(warmup);
                }
                if !svc.await_agent_tui_on_screen(
                    &pty_id,
                    &agent_cmd,
                    std::time::Duration::from_secs(15),
                ) {
                    log::warn!(
                        "launch_agent: timed out waiting for {} TUI before task delivery in pane {}",
                        agent_cmd,
                        pane_id
                    );
                }
                let provider_submit_strategy = registry
                    .with_provider(&agent_cmd, |p| p.verify_submit_strategy())
                    .unwrap_or(crate::agent::provider::VerifySubmitStrategy::ShellPrompt {
                        timeout: std::time::Duration::from_secs(5),
                    });
                let task_opts = crate::service::inject_verify::VerifyOpts {
                    require_prompt_first: false,
                    verify_echo: true,
                    verify_submit: true,
                    prompt_timeout: std::time::Duration::from_secs(0),
                    echo_timeout: std::time::Duration::from_secs(10),
                    submit_timeout: std::time::Duration::from_secs(15),
                };
                let delivery = svc.send_text_verified(
                    &pty_id,
                    task_text,
                    true,
                    provider_submit_strategy,
                    task_opts,
                );
                let delivery =
                    delivery.and_then(|result| match initial_task_delivery_confirmed(&result) {
                        Ok(()) => Ok(result),
                        Err(message) => Err(
                            crate::service::inject_verify::InjectionError::SubmitNeverConfirmed {
                                waited: result.elapsed,
                                screen_tail: message,
                            },
                        ),
                    });
                if let Err(e) = delivery {
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
        log::info!("launch_agent: agent ready in pane {}", pane_id);
        if let Some(tx) = ready_tx {
            let _ = tx.send(Ok(()));
        }
    });
}

pub(crate) fn initial_task_delivery_confirmed(
    result: &crate::service::inject_verify::InjectionResult,
) -> Result<(), String> {
    if !result.echo_observed {
        return Err("task text was not observed in the agent input".into());
    }
    if !result.submit_observed {
        return Err("task submit was not confirmed by the agent input".into());
    }
    Ok(())
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
        .with_provider(agent_cmd, |p| p.build_launch_command(&launch_params))
        .unwrap_or_else(|| {
            let mut parts: Vec<String> = vec![agent_cmd.to_string()];
            parts.extend(args.iter().cloned());
            parts.join(" ")
        });
    log::info!(
        "[spawn] build_agent_command provider={} model={:?} resume={:?} cmd={}",
        agent_cmd,
        params.model,
        params.resume_session_id,
        cmd
    );
    cmd
}
