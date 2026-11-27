use super::*;

impl ServiceContext {
    pub async fn spawn_agent(
        &self,
        params: SpawnAgentParams,
        ready_tx: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
    ) -> Result<SpawnResult, String> {
        self.spawn_agent_with_state_change(params, ready_tx, true)
            .await
    }

    async fn bind_worker_plan_task_or_rollback_tab_async(
        &self,
        params: &SpawnAgentParams,
        tab_id: &str,
    ) -> Result<(), String> {
        let service = self.clone();
        let params = params.clone();
        let tab_id = tab_id.to_string();
        crate::service::blocking::run(move || {
            service.bind_worker_plan_task_or_rollback_tab(&params, &tab_id)
        })
        .await
    }

    pub async fn spawn_agent_with_state_change(
        &self,
        mut params: SpawnAgentParams,
        ready_tx: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
        emit_state_change: bool,
    ) -> Result<SpawnResult, String> {
        self.neighbors()
            .inherit_neighbor_agent_spawn(&mut params)
            .await?;
        self.validate_remote_spawn(&params)?;
        crate::service::execution_sandbox::enforce_delegation_ceiling(self, &params, None)?;
        params.title = spawn_title::normalize_agent_title(params.title.take());
        spawn_title::validate_agent_tab_title(
            params.title.as_deref(),
            params.agent_name.as_deref(),
        )?;
        params.description =
            crate::service::tab::normalize_optional_tab_description(params.description.take())?;
        params.normalize_role_profile()?;
        let mut admission = self.admit_spawn_role(&mut params)?;
        // Orchestrators must be pinned to an explicit workspace.
        // Active-workspace fallback can silently land them in the wrong plan context.
        if params.spawns_anchor() && params.workspace_id.is_none() && params.parent_tab_id.is_none()
        {
            return Err("--workspace is required when spawning --role orchestrator \
                 (orchestrators must be pinned to a workspace)"
                .into());
        }
        // Low disk turns lanes into silent stalls (workers idle, reports lost
        // to ENOSPC) — warn loudly at spawn time while there is still room.
        if let Some(free_gb) = free_disk_gb(params.cwd.as_deref().unwrap_or("/")) {
            if free_gb < 10 {
                log::warn!(
                    "[spawn] LOW DISK: {free_gb}GB free — worker lanes fill disk fast (cargo targets); reclaim finished-lane targets before spawning more"
                );
            }
        }
        apply_worker_hierarchy_defaults(&mut params);
        let owner = self.clone();
        let (mut params, invocation, account_reservation) =
            codeterm_plugin::js_runtime::spawn_blocking(move || {
                crate::service::provider_accounts::spawn_params(&owner, params)
            })
            .await
            .map_err(|error| error.to_string())??;
        codeterm_plugin::provider_account::scope(invocation, async move {
            let _account_reservation = account_reservation;
            let default_provider = self.configuration().general_agent.agent_provider;
            let agent_cmd = params
                .agent
                .clone()
                .unwrap_or_else(|| default_provider.clone());
            crate::service::execution_sandbox::preflight(
                &self.agent_registry,
                &agent_cmd,
                &params,
                crate::service::agent_runtime::engine::requested(self, &agent_cmd),
            )?;
            self.move_chatbackend_task_to_system_prompt(&agent_cmd, &mut params)?;
            {
                let svc = self.clone();
                let provider = agent_cmd.clone();
                params = codeterm_plugin::js_runtime::spawn_blocking(move || {
                    svc.apply_model_preset(&provider, &mut params)?;
                    svc.resolve_spawn_profile_options(&provider, &mut params)?;
                    Ok::<_, String>(params)
                })
                .await
                .map_err(|_| "preset resolution worker failed")??;
            }
            params.model = crate::service::spawn::resolve_agent_model_selection(
                &self.agent_registry,
                Some(&agent_cmd),
                params.model.as_deref(),
                &agent_cmd,
            )
            .map_err(|error| error.to_string())?
            .model;
            crate::service::spawn_speed::validate_agent_speed(
                &self.agent_registry,
                &agent_cmd,
                params.speed.as_deref(),
            )?;
            let svc = self.clone();
            let probe = params.clone();
            let probe_agent = agent_cmd.clone();
            let is_shell = codeterm_plugin::js_runtime::spawn_blocking(move || {
                svc.classify_spawn_target_for_params(&probe_agent, &probe)
            })
            .await
            .map_err(|error| error.to_string())??;
            let tab_created_by_spawn = params.existing_tab_id.is_none();
            if is_shell {
                let role = params.role.clone();
                let role_profile = params.role_profile.clone();
                let model = params.model.clone();
                let bind_params = params.clone();
                let spec = crate::service::spawn::SpawnTabSpec::from_agent_params_with_chatbackends(
                    self.chatbackend_registry.as_ref(),
                    params,
                );
                let svc = self.clone();
                let handle = codeterm_plugin::js_runtime::spawn_blocking(move || {
                    svc.spawn_tab_with_ready_and_state_change(spec, ready_tx, emit_state_change)
                })
                .await
                .map_err(|error| format!("chatBackend spawn task failed: {error}"))??;
                self.persist_unresolved_plugin_model_request(&handle.tab_id, &bind_params);
                let requested_model = model
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string);
                self.bind_worker_plan_task_or_rollback_tab_async(&bind_params, &handle.tab_id)
                    .await?;
                self.bind_planner_spawn(
                    bind_params.plan_id.as_deref(),
                    role_profile.as_deref(),
                    &handle.tab_id,
                );
                if let Some(reservation) = admission.worker_persona.take() {
                    reservation.commit_to_tab(&handle.tab_id);
                }
                let title = self.mux.read().display_label_for_tab(&handle.tab_id);
                let record_product_analytics = !is_remote_incoming_spawn(&bind_params);
                self.record_kpi_spawn_with_analytics(
                    &handle.tab_id,
                    requested_model.as_deref(),
                    bind_params.task.as_deref().unwrap_or_default().len(),
                    record_product_analytics,
                );
                return Ok(SpawnResult {
                    team_id: self
                        .mux
                        .read()
                        .get_tab(&handle.tab_id)
                        .and_then(|tab| tab.team_id.clone()),
                    tab_id: handle.tab_id,
                    pty_id: handle.pty_id.unwrap_or_default(),
                    agent: agent_cmd,
                    model: None,
                    planned_model: bind_params.planned_model.clone(),
                    requested_model,
                    provider_resolved_model: None,
                    actual_model: None,
                    reasoning_effort: bind_params.reasoning_effort.clone(),
                    reasoning_effort_provenance: resolved_effort_provenance(
                        bind_params.planned_reasoning_effort.clone(),
                        bind_params.reasoning_effort.clone(),
                    ),
                    title,
                    description: handle.description,
                    role,
                    role_profile,
                    warning: handle.warning,
                    model_fallback: handle.model_fallback,
                    budget_notice: handle.budget_notice,
                });
            }

            let clearance = crate::service::spawn::provider_launch_preflight(
                &self.agent_registry,
                &agent_cmd,
                params.permission_bypass_requested(&agent_cmd),
            )
            .map_err(|e| e.to_string())?;
            let cli_warning = clearance.warning.clone();
            if let Some(ref warning) = cli_warning {
                log::warn!("[spawn] {}", warning);
            }
            let mut model_resolution = crate::service::spawn::resolve_fresh_agent_model_with_notice(
                &self.configuration(),
                &self.agent_registry,
                &agent_cmd,
                params.model.take(),
                params.resume_session_id.as_deref(),
            );
            crate::service::spawn::attach_planned_model(
                &mut model_resolution,
                params.planned_model.as_deref(),
            );
            params.model = model_resolution.model.clone();
            crate::service::spawn::validate_agent_reasoning_effort(
                &self.agent_registry,
                &agent_cmd,
                params.model.as_deref(),
                params.reasoning_effort.as_deref(),
            )?;
            // 1. Find workspace — explicit workspace_id wins over the parent's workspace.
            let (ws_id, ws_cwd, ws_provisioned_cwd) = {
                let mux = self.mux.read();
                let id = crate::service::spawn::resolve_spawn_workspace_id(
                    params.workspace_id.clone(),
                    params.parent_tab_id.as_deref(),
                    crate::service::spawn::params_caller_tab_id(&params),
                    |pid| mux.find_workspace_for_tab(pid),
                    mux.get_active_workspace().map(|w| w.id.clone()),
                    mux.list_workspaces().first().map(|w| w.id.clone()),
                )?;
                let workspace = mux.get_workspace(&id);
                let cwd = workspace.and_then(|ws| ws.cwd.clone());
                let provisioned_cwd = workspace.and_then(|ws| ws.provisioned_cwd.clone());
                (id, cwd, provisioned_cwd)
            };

            let (resolved_cwd, cwd_warning) = Self::resolve_agent_cwd(&params.cwd, &ws_cwd);
            let warning = crate::service::spawn::merge_spawn_warnings(
                crate::service::spawn::merge_spawn_warnings(cwd_warning, cli_warning),
                model_resolution.warning.clone(),
            );
            params.cwd = resolved_cwd;
            {
                let service = self.clone();
                let provider = agent_cmd.clone();
                params = codeterm_plugin::js_runtime::spawn_blocking(move || {
                    crate::service::context_bundle::prepare(&service, &provider, &mut params)?;
                    Ok::<_, String>(params)
                })
                .await
                .map_err(|error| format!("context preparation failed: {error}"))??;
            }

            let default_chat_view = crate::service::spawn::should_default_agent_tab_to_chat(
                crate::service::spawn::TabVisibility::Visible,
                self.agent_registry
                    .with_provider(&agent_cmd, |p| p.supports_chat_ui())
                    .unwrap_or(false),
            );

            let gate = self.limit_gate();
            let tab_id = {
                let mut mux = self.mux.write();
                let result = match params.existing_tab_id.as_deref() {
                    Some(existing) => mux.claim_existing_tab(&ws_id, existing)?,
                    None => crate::service::spawn::create_cleared_agent_tab(
                        &mut mux, &ws_id, &clearance, &gate,
                    )?,
                };
                if let Some(ref dir) = params.cwd {
                    if let Some(tab) = mux.get_tab_mut(&result) {
                        tab.cwd = Some(dir.clone());
                    }
                }
                let provisioned_cwd = params.provisioned_cwd.clone().or_else(|| {
                    crate::service::directory_trust::inherited_workspace_provenance(
                        params.cwd.as_deref(),
                        ws_provisioned_cwd.as_deref(),
                    )
                });
                if let Some(provisioned_cwd) = provisioned_cwd {
                    if let Some(tab) = mux.get_tab_mut(&result) {
                        tab.provisioned_cwd = Some(provisioned_cwd);
                    }
                }
                if let Some(tab) = mux.get_tab_mut(&result) {
                    initialize_ordinary_pty_agent_tab(tab, &params);
                    tab.taskless_authorized_gates = params.authorized_gates.clone();
                }
                if params.activate.unwrap_or(false) {
                    mux.set_tab_focus_requested(&result, true);
                }
                if default_chat_view {
                    if let Some(tab) = mux.get_tab_mut(&result) {
                        tab.chat_view = true;
                    }
                }
                if let Some(ref rtid) = params.report_target_tab_id {
                    if let Some(tab) = mux.get_tab_mut(&result) {
                        tab.report_target_tab_id = Some(rtid.clone());
                    }
                }
                // Record the spawning agent for report routing fallback, independent of explicit --parent / role hierarchy.
                if let Some(ref sb) = params.spawned_by_tab_id {
                    if !sb.is_empty() {
                        if let Some(tab) = mux.get_tab_mut(&result) {
                            tab.spawned_by_tab_id = Some(sb.clone());
                        }
                    }
                }
                result
            };
            self.set_and_lock_tab_agent_type_from(
                &tab_id,
                agent_cmd.to_string(),
                crate::session::lifecycle::AgentTypeWriteOrigin::Spawn,
                params.creator_tab_id.as_deref(),
            );
            if emit_state_change {
                self.emit_state_changed(MuxEmitCause::Team);
            }

            // (its hierarchy lives on the origin) — skip_local_worker_role avoids a 500.
            if let Some(ref role_str) = params.role {
                let role = Self::parse_role(role_str)?;
                if skip_local_worker_role(
                    &role,
                    params.parent_tab_id.as_deref(),
                    params.report_target_mesh_peer.as_deref(),
                ) {
                    // Cross-mesh worker: semantic Worker identity, no target-local parent.
                    let mut mux = self.mux.write();
                    mux.set_detached_worker_role(&tab_id, params.role_profile.as_deref())?;
                } else if let Err(error) = self.assign_spawn_role_hierarchy(
                    &tab_id,
                    role_str,
                    params.role_profile.as_deref(),
                    params.parent_tab_id.as_deref(),
                    params.plan_id.as_deref(),
                    params.can_spawn,
                    emit_state_change,
                ) {
                    // A post-creation hierarchy failure must not leave an orphan tab.
                    if tab_created_by_spawn {
                        let _ = self
                            .close_tab_with_reason_async(&tab_id, TabCloseReason::SpawnRollback)
                            .await;
                    }
                    return Err(error);
                }
            }

            // Role placement can clear a Tab's Team; a bootstrapped manager must
            // launch with its reserved, host-resolved Team in env. Never mint.
            if let Some(team_id) = bootstrap_bound_team_id(params.manager_bootstrap.as_ref()) {
                let membership = {
                    let mut mux = self.mux.write();
                    let shape = shape_for_tab(&mux, &tab_id);
                    let membership = refuse_general_agent_team_membership(
                        &mux,
                        &tab_id,
                        MembershipAttempt::Join,
                        &shape,
                    );
                    if membership.is_ok() && mux.get_team(team_id).is_some() {
                        if let Some(tab) = mux.get_tab_mut(&tab_id) {
                            tab.team_id = Some(team_id.to_string());
                        }
                    }
                    membership
                };
                if let Err(refusal) = membership {
                    if tab_created_by_spawn {
                        let _ = self
                            .close_tab_with_reason_async(&tab_id, TabCloseReason::SpawnRollback)
                            .await;
                    }
                    return Err(refusal.to_string());
                }
            }

            runtime_prompt::bind_manager_launch_identity(self, &tab_id, &mut params)?;
            let worker =
                self.mux.read().get_tab(&tab_id).is_some_and(|tab| {
                    tab.role == Some(crate::multiplexer::types::AgentRole::Worker)
                });
            let permission_bypass_authorization =
                crate::service::permission_policy::spawn_permission_bypass_for_params(
                    self, &agent_cmd, worker, &params,
                )?;
            let permission_bypass = permission_bypass_authorization.is_some();
            params.skip_permissions = Some(permission_bypass);
            params.permission_bypass_authorization = permission_bypass_authorization;
            {
                let mut mux = self.mux.write();
                if let Some(tab) = mux.get_tab_mut(&tab_id) {
                    tab.auto_approve = permission_bypass;
                }
            }

            if let Some((title, level)) = crate::service::spawn_title::creation_title_override(
                params.title.as_deref(),
                params.agent_name.as_deref(),
            ) {
                match level {
                    crate::multiplexer::types::TitleLevel::User => {
                        let task_bound = self
                            .mux
                            .read()
                            .get_tab(&tab_id)
                            .is_some_and(|tab| tab.task_binding.is_some());
                        if !task_bound {
                            self.rename_tab(&tab_id, &title)?;
                        }
                    }
                    _ => self.set_tab_title(&tab_id, &title)?,
                }
            }

            let task_identity = params.task_launch_authority.as_ref().map(|authority| {
                crate::service::startup_lease::StartupTaskIdentity {
                    plan_id: authority.plan_id.clone(),
                    task_id: authority.task_id.clone(),
                    attempt_id: authority.attempt_id.clone(),
                }
            });
            let remote_task_bound = remote_task_binding_matches(
                params.remote_task_binding.as_ref(),
                task_identity.as_ref(),
            );
            let startup_supervised = !params.user_created
                && !remote_task_bound
                && self.prepare_agent_startup_supervision(
                    &tab_id,
                    params
                        .parent_tab_id
                        .as_deref()
                        .or(params.spawned_by_tab_id.as_deref())
                        .or(params.creator_tab_id.as_deref()),
                    task_identity.clone(),
                );
            if params.task_launch_authority.is_some() && !startup_supervised && !remote_task_bound {
                if tab_created_by_spawn {
                    self.cancel_agent_startup_supervision(&tab_id);
                    let _ = self
                        .close_tab_with_reason_async(&tab_id, TabCloseReason::SpawnRollback)
                        .await;
                }
                return Err(format!(
                    "task-bound dispatch for Tab {tab_id} could not register a startup supervisor"
                ));
            }

            if let Some(tab) = self.mux.write().get_tab_mut(&tab_id) {
                crate::service::spawn::persist_model_launch_identity(
                    tab,
                    params.planned_model.as_deref(),
                    &model_resolution,
                );
                crate::service::reasoning_effort::persist_launch_identity(
                    tab,
                    params.planned_reasoning_effort.as_deref(),
                    params.reasoning_effort.as_deref(),
                );
            }

            // 4c. Pin agent_session_id for resume up-front: we know the id from the user's request, so don't wait for detect_session_id polling.
            let launch_session_id = if params.resume_session_id.is_none() {
                self.agent_registry
                    .with_provider(&agent_cmd, |p| p.session_id_injection())
                    .filter(|mode| {
                        !matches!(mode, crate::agent::provider::SessionIdInjection::None)
                    })
                    .map(|_| uuid::Uuid::new_v4().to_string())
            } else {
                None
            };
            if params.fork_session {
                if let Some(tab) = self.mux.write().get_tab_mut(&tab_id) {
                    tab.fork_source_session_id = params.resume_session_id.clone();
                }
            } else if let Some(ref sid) = params.resume_session_id {
                self.set_agent_session_id(&tab_id, Some(sid.clone()));
            } else if let Some(ref sid) = launch_session_id {
                self.set_agent_session_id(&tab_id, Some(sid.clone()));
            }
            params.structured_prompt_server = crate::service::structured_prompt::ensure_for_spawn(
                self,
                &agent_cmd,
                &tab_id,
                &params.cwd.clone().unwrap_or_default(),
            );

            let system_prompt_capable = self
                .agent_registry
                .with_provider(&agent_cmd, |provider| {
                    matches!(
                        crate::service::launch_delivery::mode_for_descriptor(
                            &provider.system_prompt_delivery(),
                        ),
                        crate::service::launch_delivery::LaunchDeliveryMode::SystemPrompt
                    )
                })
                .unwrap_or(false);
            let fallback_bootstrap = if params.manager_bootstrap.is_some() {
                self.emit_boot_stage(
                    &tab_id,
                    crate::service::boot_stage::BootStage::PreparingRoleInstructions,
                    None,
                );
                prepare_manager_bootstrap(&mut params, system_prompt_capable)
            } else {
                prepare_standard_starter_prompt(&mut params);
                None
            };
            if !self
                .agent_registry
                .with_provider(&agent_cmd, |provider| provider.is_tool_less())
                .unwrap_or(false)
            {
                apply_startup_handshake(&mut params, startup_supervised, true);
            }
            let fallback_bootstrap =
                fallback_bootstrap.map(|body| team_bootstrap_payload(system_prompt_capable, body));
            let input_provider = self.agent_registry.provider_arc(&agent_cmd);
            let spawn_input = {
                let handshake = (params.starter_prompt
                    == crate::agent::provider::StarterPrompt::StartupHandshake)
                    .then_some(params.starter_prompt);
                crate::service::launch_delivery::prepare_spawn_input(
                    input_provider.as_deref(),
                    params.role.as_deref(),
                    params.task.as_deref(),
                    params.starter_prompt,
                    handshake,
                    |task| {
                        self.spawn_task_message(&tab_id, task)
                            .unwrap_or_else(|| task.to_string())
                    },
                )
            };
            params.starter_prompt = spawn_input.starter_prompt;
            let bg_task = if params.manager_bootstrap.is_some() {
                if system_prompt_capable {
                    Some(agent_system_prompt(params.task.as_deref()))
                } else {
                    params.task.clone()
                }
            } else {
                let mut launch_params = params.clone();
                launch_params.task = spawn_input.provider_task.map(str::to_string);
                spawn_launch_task_for_provider(
                    &self.agent_registry,
                    &launch_params,
                    system_prompt_capable,
                )
            };
            // Planner gets a short display hint plus its full onboarding as launch input.
            let is_planner = params.role_kind() == Some(builtins::PLANNER);
            // A pair-spawned Orchestrator carries its onboarding in the launch task.
            let is_orch_onboarding =
                params.spawns_anchor() && params.skip_orchestrator_onboarding == Some(true);
            let bg_task = if is_planner {
                match params.plan_id.as_deref() {
                    Some(plan_id) => {
                        self.bind_planner_spawn(Some(plan_id), Some("planner"), &tab_id);
                        let orch_tab = self.plan_store.owner_of(plan_id).and_then(|o| o.tab_id);
                        let orch_label = orch_tab
                            .as_deref()
                            .map(|p| self.mux.read().display_label_for_tab(p))
                            .unwrap_or_else(|| "your Orchestrator".to_string());
                        let orch_tab = orch_tab.as_deref().unwrap_or("unknown");
                        let onboard =
                            crate::prompts::planner_onboarding_task(&orch_label, orch_tab, plan_id);
                        let onboarding =
                            planner_startup_payload(system_prompt_capable, bg_task, &onboard);
                        Some(team_bootstrap_payload(system_prompt_capable, onboarding))
                    }
                    None => bg_task,
                }
            } else if is_orch_onboarding && !system_prompt_capable {
                bg_task.map(|task| team_bootstrap_payload(false, task))
            } else {
                bg_task
            };
            // Tag with the spawner's origin prefix (FROM-<spawner>) for every provider —
            // but role onboarding is already CodeTerm-attributed, so skip it there.
            let bg_task = if is_planner || is_orch_onboarding {
                bg_task
            } else {
                bg_task.map(|task| self.spawn_task_message(&tab_id, &task).unwrap_or(task))
            };
            if let Some(tab) = self.mux.write().get_tab_mut(&tab_id) {
                tab.restorable_launch.account_id = params.account_id.clone();
                tab.restorable_launch.account_namespace = params.account_namespace.clone();
            }
            crate::service::restored_launch::capture_tab_launch_parameters(
                &self.mux,
                &tab_id,
                &self.agent_registry,
                crate::service::restored_launch::RestorableLaunchCapture {
                    provider_id: &agent_cmd,
                    requested_model: model_resolution.requested_model.clone(),
                    model: params.model.clone(),
                    reasoning_effort: params.reasoning_effort.clone(),
                    args: params.args.clone(),
                    agent_name: params.agent_name.clone(),
                    task: params.task.as_deref(),
                    role: params.role.as_deref(),
                    role_profile: params.role_profile.as_deref(),
                    permission_bypass_requested: params.permission_bypass_requested(&agent_cmd),
                },
            );
            let launch_cmd = {
                let service = self.clone();
                let tab_id = tab_id.clone();
                let provider = agent_cmd.clone();
                let params = params.clone();
                let task = bg_task.clone();
                let session = launch_session_id.clone();
                codeterm_plugin::js_runtime::spawn_blocking(move || {
                    build_or_refuse_orchestrated_launch_command(
                        &service,
                        &tab_id,
                        &provider,
                        &params,
                        task.as_deref(),
                        session.as_deref(),
                    )
                })
                .await
                .map_err(|error| format!("Launch composition failed: {error}"))??
            };
            self.bind_worker_plan_task_or_rollback_tab_async(&params, &tab_id)
                .await?;
            let fallback_spec = spawn_fallback_spec(
                &spawn_input,
                is_planner,
                fallback_bootstrap.as_deref(),
                bg_task.as_deref(),
                self.agent_registry
                    .with_provider(&agent_cmd, |provider| {
                        provider.starter_prompt_text(params.starter_prompt)
                    })
                    .flatten()
                    .as_deref(),
            );
            if let Some(spec) = fallback_spec.as_ref() {
                let service = self.clone();
                let params = params.clone();
                let tab_id = tab_id.clone();
                let spec = spec.clone();
                crate::service::blocking::run(move || {
                    create_pending_delivery_or_rollback_dispatch(&service, &params, &tab_id, &spec)
                })
                .await?;
            }
            let startup_turn = crate::service::startup_turn::resolve(
                &self.agent_registry,
                &agent_cmd,
                params.starter_prompt,
                fallback_spec.as_ref(),
                bg_task.as_deref(),
                true,
                spawn_input.confirmed_first_turn.as_deref(),
            );
            launch_agent_in_background_with_task_turn(
                self.clone(),
                tab_id.clone(),
                launch_cmd,
                agent_cmd.clone(),
                crate::service::launch_diagnostics::AgentLaunchOrigin::OrchestratedSpawn,
                startup_turn.first_turn_will_be_delivered,
                startup_turn.expected_turn,
                bg_task,
                fallback_spec,
                task_identity,
                ready_tx,
            );

            // (subscriber already started inside ensure_for_spawn above)

            let title = self.mux.read().display_label_for_tab(&tab_id);

            if let Some(reservation) = admission.worker_persona.take() {
                reservation.commit_to_tab(&tab_id);
            }
            self.record_kpi_tab_created(&tab_id, crate::agent::kpi_ledger::TabCreatedKind::Agent);
            let budget_notice = self.workspace_tab_budget_notice(&ws_id);
            let reasoning_effort = params.reasoning_effort.clone();
            let record_product_analytics = !is_remote_incoming_spawn(&params);
            self.record_kpi_spawn_with_analytics(
                &tab_id,
                model_resolution.model.as_deref(),
                params.task.as_deref().unwrap_or_default().len(),
                record_product_analytics,
            );
            Ok(SpawnResult {
                team_id: self
                    .mux
                    .read()
                    .get_tab(&tab_id)
                    .and_then(|tab| tab.team_id.clone()),
                tab_id,
                pty_id: String::new(), // PTY not yet assigned — happens async
                agent: agent_cmd,
                model: model_resolution.model.clone(),
                planned_model: params.planned_model,
                requested_model: model_resolution.requested_model,
                provider_resolved_model: model_resolution.provider_resolved_model,
                actual_model: None,
                reasoning_effort: reasoning_effort.clone(),
                reasoning_effort_provenance: resolved_effort_provenance(
                    params.planned_reasoning_effort,
                    reasoning_effort,
                ),
                title,
                description: params.description,
                role: params.role,
                role_profile: params.role_profile,
                warning,
                model_fallback: model_resolution.fallback_notice,
                budget_notice,
            })
        })
        .await
    }
}
