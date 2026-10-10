use super::*;
use crate::service::general_agent::session_contract::{LaunchOwnership, TabLaunch};

pub(super) fn start(
    svc: &crate::ServiceState,
    mux: &crate::MuxState,
    pty: &crate::PtyState,
    screen_state: &crate::ScreenStateHandle,
    config: &crate::config::ConfigState,
    target: TabLaunch<'_>,
) {
    let TabLaunch {
        tab_id,
        session_id,
        provider: provider_name,
        permission_bypass,
        ownership,
    } = target;
    let session_id = ownership.select_session(session_id);
    let general_cwd = general_dir().to_string_lossy().to_string();
    let mut fresh_prompt = (ownership == LaunchOwnership::FreshSession)
        .then(|| instructions::render_system_prompt(provider_name, Some(&svc.mem)));
    let model_resolution = (ownership == LaunchOwnership::FreshSession).then(|| {
        crate::service::spawn::resolve_fresh_agent_model_with_notice(
            &svc.agent_registry,
            provider_name,
            None,
            None,
        )
    });
    let model_args = model_resolution
        .as_ref()
        .and_then(|resolution| resolution.model.as_deref())
        .and_then(|model| {
            svc.agent_registry
                .with_provider(provider_name, |provider| provider.model_launch_args(model))
        })
        .unwrap_or_default();
    if let Some(resolution) = model_resolution.as_ref() {
        if let Some(tab) = mux.write().get_tab_mut(&tab_id) {
            crate::service::spawn::persist_model_launch_identity(tab, None, resolution);
        }
        crate::service::restored_launch::capture_tab_launch_parameters(
            mux,
            &tab_id,
            &svc.agent_registry,
            crate::service::restored_launch::RestorableLaunchCapture {
                provider_id: provider_name,
                requested_model: None,
                model: resolution.model.clone(),
                reasoning_effort: None,
                args: None,
                agent_name: None,
                task: fresh_prompt.as_deref(),
                role: Some("Manager"),
                role_profile: Some(super::super::GENERAL_AGENT_KIND),
                permission_bypass_requested: permission_bypass,
            },
        );
    }
    let cols = 120u16;
    let rows = 40u16;
    crate::service::codex_rpc::lifecycle::stop_tab(&tab_id);
    let pad = match svc.ensure_agent_pad(&tab_id) {
        Ok(pad) => pad,
        Err(error) => {
            fail_general_agent_start(svc, mux, &tab_id, &error);
            return;
        }
    };
    let mut env_vars = general_agent_env_vars(
        &tab_id,
        pad.env_value(),
        svc.rotate_session_binding_nonce(&tab_id),
        config,
    );
    match svc.model_provider_launch_env(provider_name) {
        Ok(keys) => env_vars.extend(keys),
        Err(error) => {
            fail_general_agent_start(svc, mux, &tab_id, &error.to_string());
            return;
        }
    }
    crate::service::codex_rpc::lifecycle::launch_env_for_provider(
        svc,
        &tab_id,
        provider_name,
        &mut env_vars,
    );
    crate::service::launch_diagnostics::log_launch_environment("general_agent", &tab_id, &env_vars);

    let pty_claim = svc.pty_ownership.acquire(&tab_id);
    let pty_id = {
        let mut manager = pty.lock();
        match manager.spawn_with_id(None, None, Some(general_cwd.clone()), cols, rows, env_vars) {
            Ok((id, _)) => id,
            Err(e) => {
                log::error!("[general-agent] Failed to spawn PTY: {}", e);
                let _ = svc.revoke_agent_pad(&tab_id);
                fail_general_agent_start(svc, mux, &tab_id, &e);
                return;
            }
        }
    };

    let selection = svc.link_pty_with_claim(&pty_claim, &tab_id, pty_id, true);
    drop(pty_claim);
    if !selection.linked {
        fail_general_agent_start(
            svc,
            mux,
            &tab_id,
            "General Agent Tab closed before PTY link",
        );
        return;
    }
    let pty_id = selection.pty_id;
    if let Some(tab) = mux.write().get_tab_mut(&tab_id) {
        tab.auto_approve = permission_bypass;
    }
    svc.set_and_lock_tab_agent_type_from(
        &tab_id,
        provider_name.to_string(),
        crate::session::lifecycle::AgentTypeWriteOrigin::Spawn,
        None,
    );
    svc.emit_state_changed();
    log::info!("[general-agent] PTY {} linked to Tab {}", pty_id, tab_id);

    // Start backend reader on a tokio runtime (start_backend_reader uses tokio::spawn, but we're on a plain std::thread — no tokio runtime here)
    log::info!("[general-agent] Starting backend reader for PTY {}", pty_id);
    {
        let svc_clone = svc.clone();
        let pty_id_clone = pty_id.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create backend reader runtime");
            rt.block_on(async {
                match crate::service::pty_lifecycle::start_backend_reader(
                    &svc_clone,
                    &pty_id_clone,
                    true,
                ) {
                    Ok(handle) => {
                        let _ = handle.await;
                    }
                    Err(e) => log::error!("[general-agent] Failed to start backend reader: {}", e),
                }
            });
        });
    }
    log::info!(
        "[general-agent] Backend reader started, waiting for shell prompt on PTY {}...",
        pty_id
    );

    let shell_wait_started = std::time::Instant::now();
    let ready = crate::service::pty_io::await_shell_prompt(
        screen_state,
        &pty_id,
        std::time::Duration::from_secs(60),
    );
    if !ready {
        let waited_ms = shell_wait_started.elapsed().as_millis();
        log::warn!(
            "[general-agent] Shell did not report ready for PTY {} after {}ms",
            pty_id,
            waited_ms
        );
        fail_general_agent_start(
            svc,
            mux,
            &tab_id,
            &format!("shell readiness gave up after {}ms", waited_ms),
        );
        return;
    }
    log::info!("[general-agent] Shell ready, building launch command...");

    let use_resume = session_id.is_some();
    let mut startup_message = None;
    let mut structured_starter = None;
    let cmd = if use_resume {
        let sid = session_id.as_ref().unwrap();
        match super::super::launch_files::command(
            svc,
            &tab_id,
            provider_name,
            &general_cwd,
            &mut crate::agent::provider::AgentLaunchParams {
                task: Some(instructions::render_system_prompt(
                    provider_name,
                    Some(&svc.mem),
                )),
                starter_prompt: crate::agent::provider::StarterPrompt::NoStarter,
                skip_permissions: permission_bypass,
                ..Default::default()
            },
            Some(sid),
        ) {
            Some(cmd) => cmd,
            None => {
                let reason = format!(
                    "Provider '{}' not registered or resume unsupported",
                    provider_name
                );
                log::error!("[general-agent] {reason}");
                fail_general_agent_start(svc, mux, &tab_id, &reason);
                return;
            }
        }
    } else {
        // For providers supporting session-id injection, pre-generate and pin it before launch.
        // This avoids guessing by mtime after start, especially when multiple tabs share a cwd.
        let injection = svc
            .agent_registry
            .with_provider(provider_name, |p| p.session_id_injection())
            .unwrap_or(crate::agent::provider::SessionIdInjection::None);
        let pre_session_id = if injection != crate::agent::provider::SessionIdInjection::None {
            let id = uuid::Uuid::new_v4().to_string();
            svc.set_agent_session_id(&tab_id, Some(id.clone()));
            ownership.persist_session_id(provider_name, &id);
            log::info!(
                "[general-agent] Pre-assigned session id {} for fresh launch",
                id
            );
            Some(id)
        } else {
            None
        };
        let ga_cwd = svc
            .mux
            .read()
            .get_tab(&tab_id)
            .and_then(|p| p.cwd.clone())
            .unwrap_or_default();
        let system_prompt = fresh_prompt
            .take()
            .unwrap_or_else(|| instructions::render_system_prompt(provider_name, Some(&svc.mem)));
        let system_prompt_delivery = svc
            .agent_registry
            .with_provider(provider_name, |provider| provider.system_prompt_delivery())
            .unwrap_or(crate::agent::provider::SystemPromptDelivery::External);
        let bootstrap_message = ownership
            .bootstrap()
            .then(|| compose_startup_message(&svc.mem, &general_dir().join("bootstrap.md")))
            .flatten();
        let launch_instructions =
            compose_launch_instructions(system_prompt, bootstrap_message, &system_prompt_delivery);
        startup_message = launch_instructions.startup_message;
        if let Some(message) = startup_message.as_deref() {
            if let Err(error) = svc.create_pending_delivery(
                &tab_id,
                crate::multiplexer::types::PendingDeliveryChannel::InteractiveInput,
                message,
            ) {
                fail_general_agent_start(svc, mux, &tab_id, &error.to_string());
                return;
            }
        }
        let mut params = crate::agent::provider::AgentLaunchParams {
            context_files: None,
            tool_less_instructions_path: None,
            agent_name: None,
            task: launch_instructions.task,
            starter_prompt: launch_instructions.starter_prompt,
            args: (!model_args.is_empty()).then_some(model_args),
            skip_permissions: permission_bypass,
            session_id: pre_session_id,
            structured_prompt_server: crate::service::structured_prompt::ensure_for_spawn(
                svc,
                provider_name,
                &tab_id,
                &ga_cwd,
            ),
            launch_marker: None,
            instance_id: Some(crate::paths::codeterm_instance_id().to_string()),
        };
        structured_starter =
            super::super::launch_files::starter(svc, provider_name, params.starter_prompt);
        let command = match super::super::launch_files::command_with_selection(
            svc,
            &tab_id,
            provider_name,
            &ga_cwd,
            &mut params,
            crate::service::codex_rpc::launch::Selection {
                model: model_resolution
                    .as_ref()
                    .and_then(|resolution| resolution.model.clone()),
                ..Default::default()
            },
        ) {
            Some(cmd) => cmd,
            None => {
                let reason = format!("Provider '{}' not registered", provider_name);
                log::error!("[general-agent] {reason}");
                fail_general_agent_start(svc, mux, &tab_id, &reason);
                return;
            }
        };
        crate::service::launch_diagnostics::log_launch(
            crate::service::launch_diagnostics::AgentLaunchOrigin::GeneralAgent,
            &tab_id,
            provider_name,
            &command,
            params.task.as_deref().or(startup_message.as_deref()),
            Some("General Agent"),
            Some("machine_general"),
            launch_instructions.prompt_delivery,
        );
        command
    };

    crate::service::task_files::retire_settings_for_launch(svc, &tab_id, &cmd);

    if use_resume {
        crate::service::launch_diagnostics::log_launch(
            crate::service::launch_diagnostics::AgentLaunchOrigin::GeneralAgent,
            &tab_id,
            provider_name,
            &cmd,
            None,
            Some("General Agent"),
            Some("machine_general"),
            "resumed_session",
        );
    }
    log::info!("[general-agent] Launching on PTY {}", pty_id);
    let payload: Vec<u8> = if cfg!(windows) {
        // Bracketed paste + sacrificial space absorbs PSReadLine/ConPTY first-byte race on Windows.
        format!("\x1b[200~ {}\x1b[201~\r", cmd).into_bytes()
    } else {
        // No bracket-paste: zsh ZLE binds the handler only after the first interactive cycle, so the markers leak as literal text on launch.
        format!("{}\r", cmd).into_bytes()
    };
    let launch_write = pty.lock().write(&pty_id, &payload);
    if let Err(e) = launch_write {
        log::error!("[general-agent] Failed to write launch command: {}", e);
        fail_general_agent_start(svc, mux, &tab_id, &e.to_string());
        return;
    }

    match super::super::launch_files::finish_structured(
        svc,
        &tab_id,
        &pty_id,
        startup_message.as_deref().or(structured_starter.as_deref()),
    ) {
        Ok(true) => startup_message = None,
        Ok(false) => {}
        Err(error) => {
            fail_general_agent_start(svc, mux, &tab_id, &error);
            return;
        }
    }

    let launched = await_agent_launch_evidence(
        screen_state,
        &svc.agent_registry,
        mux,
        &tab_id,
        &pty_id,
        std::time::Duration::from_secs(15),
    );
    if !tab_still_owns_pty(mux, &tab_id, &pty_id) {
        log::info!(
            "[general-agent] Launch on stale PTY {} for Tab {} ignored; Tab moved on",
            pty_id,
            tab_id
        );
        return;
    }
    if !launched {
        defer_general_agent_start_to_late_readiness(svc, mux, &tab_id, &pty_id);
        return;
    }
    svc.emit_boot_stage(&tab_id, BootStage::ProviderReady, None);
    let bound_session = match deliver_before_binding(
        || {
            if let Some(message) = startup_message {
                deliver_confirmed_general_message(svc, &tab_id, &pty_id, provider_name, &message)?;
            }
            Ok(())
        },
        || {
            if use_resume {
                return mux
                    .read()
                    .get_tab(&tab_id)
                    .and_then(|tab| tab.agent_session_id.clone());
            }
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            binding_wait::wait_for_session_binding(deadline, || {
                mux.read()
                    .get_tab(&tab_id)
                    .and_then(|tab| tab.agent_session_id.clone())
            })
        },
    ) {
        Ok(bound_session) => bound_session,
        Err(error) => {
            fail_general_agent_start(svc, mux, &tab_id, &error);
            return;
        }
    };
    let Some(bound_session) = bound_session else {
        defer_general_agent_start_to_late_binding(svc, mux, &tab_id, &pty_id);
        return;
    };
    if !use_resume {
        ownership.persist_session_id(provider_name, &bound_session);
        log::info!(
            "[general-agent] Persisted hook-bound session {}",
            bound_session
        );
    }
    if let Some(tab) = mux.write().get_tab_mut(&tab_id) {
        tab.agent_booting = false;
    }
    svc.emit_boot_stage(&tab_id, BootStage::AgentReady, None);
    svc.observe_tab_state(
        &tab_id,
        crate::service::supporter_tab_state::ObservationTrigger::SpawnReady,
        "general_agent_launch_ready",
    );
    let settled_tab = { mux.read().get_tab(&tab_id).cloned() };
    if let Some(tab) = settled_tab {
        svc.general_agent_watchdog_event(&tab, super::super::WatchdogEvent::LaunchSettlement);
    }
    svc.emit_state_changed();
    log::info!(
        "[general-agent] Started successfully (tab={}, pty={})",
        tab_id,
        pty_id
    );
}
