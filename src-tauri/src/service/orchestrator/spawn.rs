use super::launch::{
    agent_system_prompt, build_agent_command, launch_agent_in_background, role_onboarding_task,
    skip_local_worker_role, spawn_launch_task,
};
use super::types::{ReasoningEffortProvenance, SpawnAgentParams, SpawnResult};
use crate::service::{spawn_title, ServiceContext};

/// Build effort provenance for a launch where the requested effort has already
/// passed capability validation. No silent downgrade ⇒ `resolved == requested`.
/// `actual` stays `None` — the launch flag is a request, not runtime proof.
pub(crate) fn resolved_effort_provenance(
    planned: Option<String>,
    requested: Option<String>,
) -> ReasoningEffortProvenance {
    let has_effort = planned.is_some() || requested.is_some();
    ReasoningEffortProvenance {
        planned,
        requested: requested.clone(),
        resolved: requested,
        actual: None,
        actual_source: has_effort.then(|| "provider-default-unverified".into()),
    }
}

fn prepare_manager_bootstrap(params: &mut SpawnAgentParams) -> Option<String> {
    let has_assignment = params
        .task
        .as_deref()
        .is_some_and(|task| !task.trim().is_empty());
    let bootstrap = params.manager_bootstrap.as_ref()?.compose();
    params.task = Some(match params.task.take() {
        Some(task) if !task.trim().is_empty() => format!("{bootstrap}\n\n{task}"),
        None => bootstrap.clone(),
        Some(_) => bootstrap.clone(),
    });
    params.starter_prompt = if has_assignment {
        crate::agent::provider::StarterPrompt::AssignedTask
    } else {
        crate::agent::provider::StarterPrompt::TeamBootstrap
    };
    Some(bootstrap)
}

fn prepare_standard_starter_prompt(params: &mut SpawnAgentParams) {
    params.starter_prompt = crate::service::spawn_params::starter_prompt_for_creation(
        params.role.as_deref(),
        params.task.as_deref(),
    );
}

fn compose_chatbackend_system_prompt(params: &mut SpawnAgentParams) -> Option<String> {
    let assignment = params.system_prompt.take().or_else(|| params.task.take());
    let assignment = match params.manager_bootstrap.as_ref() {
        Some(manager) => {
            let bootstrap = manager.compose();
            Some(match assignment {
                Some(assignment) if !assignment.trim().is_empty() => {
                    format!("{bootstrap}\n\n{assignment}")
                }
                _ => bootstrap,
            })
        }
        None => assignment,
    };
    role_onboarding_task(
        assignment.as_deref(),
        params.role.as_deref(),
        params.role_profile.as_deref(),
    )
}

impl ServiceContext {
    /// Resolve agent cwd against workspace cwd:
    /// - If no cwd specified → inherit from workspace
    /// - If cwd differs from workspace → return warning
    /// - If nothing is specified → pin the daemon's own cwd, the dir the PTY
    ///   would inherit anyway. Leaving it `None` desyncs the tab record from
    ///   where the agent actually runs, so provider session-file resolution
    ///   (`~/.claude/projects/<encoded-cwd>/<sid>.jsonl`) looks in the wrong
    ///   dir and the chat pipeline reads empty.
    pub(crate) fn resolve_agent_cwd(
        agent_cwd: &Option<String>,
        workspace_cwd: &Option<String>,
    ) -> (Option<String>, Option<String>) {
        match (agent_cwd, workspace_cwd) {
            (None, Some(ws_cwd)) => {
                log::info!("[spawn] agent cwd inherited from workspace: {}", ws_cwd);
                (Some(ws_cwd.clone()), None)
            }
            (Some(agent_dir), Some(ws_dir)) if !Self::paths_equal(agent_dir, ws_dir) => {
                let warning = format!(
                    "Agent cwd '{}' differs from workspace cwd '{}'. \
                     The agent will run in '{}', but the workspace is at '{}'.",
                    agent_dir, ws_dir, agent_dir, ws_dir
                );
                log::warn!("[spawn] {}", warning);
                (Some(agent_dir.clone()), Some(warning))
            }
            (Some(dir), _) => (Some(dir.clone()), None),
            (None, None) => (
                std::env::current_dir()
                    .ok()
                    .and_then(|p| p.to_str().map(String::from)),
                None,
            ),
        }
    }

    /// Compare paths ignoring trailing slashes and case on Windows
    pub(crate) fn paths_equal(a: &str, b: &str) -> bool {
        let norm = |s: &str| {
            let s = s.replace('\\', "/");
            s.trim_end_matches('/').to_string()
        };
        #[cfg(target_os = "windows")]
        {
            norm(a).eq_ignore_ascii_case(&norm(b))
        }
        #[cfg(not(target_os = "windows"))]
        {
            norm(a) == norm(b)
        }
    }

    /// Legacy wrapper for callers still on `SpawnAgentParams` (CLI/REST/batch/cron/socket).
    pub fn spawn_agent_sync(&self, params: SpawnAgentParams) -> Result<SpawnResult, String> {
        self.spawn_agent_sync_with_ready(params, None)
    }

    /// Variant of `spawn_agent_sync` with a readiness oneshot for echo+submit verification.
    pub fn spawn_agent_sync_with_ready(
        &self,
        mut params: SpawnAgentParams,
        ready_tx: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
    ) -> Result<SpawnResult, String> {
        params.title = spawn_title::normalize_agent_title(params.title.take());
        params.normalize_role_profile()?;
        crate::service::spawn_plan_task::validate_worker_plan_task_spawn(&params)?;
        self.preflight_worker_plan_task_spawn(&params)?;
        self.prepare_worker_plan_task_spawn(&mut params)?;
        let worker_persona_reservation = self.reserve_worker_persona(&mut params)?;
        let role = params.role.clone();
        let role_profile = params.role_profile.clone();
        let plan_id = params.plan_id.clone();
        let agent = params
            .agent
            .clone()
            .unwrap_or_else(crate::config::default_agent_provider);
        self.move_chatbackend_task_to_system_prompt(&agent, &mut params)?;
        let title = params
            .title
            .clone()
            .or_else(|| params.agent_name.clone())
            .unwrap_or_else(|| agent.clone());
        let reasoning_effort = params.reasoning_effort.clone();
        self.classify_spawn_target_for_params(&agent, &params)?;
        let bind_params = params.clone();
        let spec = crate::service::spawn::SpawnTabSpec::from_agent_params_with_chatbackends(
            self.chatbackend_registry.as_ref(),
            params,
        );
        let handle = self.spawn_tab_with_ready(spec, ready_tx)?;
        if let Some(tab) = self.mux.write().get_tab_mut(&handle.tab_id) {
            crate::service::reasoning_effort::persist_launch_identity(
                tab,
                bind_params.planned_reasoning_effort.as_deref(),
                bind_params.reasoning_effort.as_deref(),
            );
        }
        self.persist_unresolved_plugin_model_request(&handle.tab_id, &bind_params);
        let model_identity = self.tab_model_identity(&handle.tab_id);
        self.bind_worker_plan_task_or_rollback_tab(&bind_params, &handle.tab_id)?;
        self.bind_planner_spawn(plan_id.as_deref(), role_profile.as_deref(), &handle.tab_id);
        if let Some(reservation) = worker_persona_reservation {
            reservation.commit_to_tab(&handle.tab_id);
        }
        self.record_kpi_spawn(
            &handle.tab_id,
            model_identity.2.as_deref(),
            bind_params.task.as_deref().unwrap_or_default().len(),
        );
        Ok(SpawnResult {
            tab_id: handle.tab_id,
            pty_id: handle.pty_id.unwrap_or_default(),
            agent,
            model: model_identity.2.clone(),
            planned_model: model_identity.0,
            requested_model: model_identity.1,
            provider_resolved_model: model_identity.2,
            actual_model: model_identity.3,
            reasoning_effort: reasoning_effort.clone(),
            reasoning_effort_provenance: resolved_effort_provenance(
                bind_params.planned_reasoning_effort,
                reasoning_effort,
            ),
            title,
            description: handle.description,
            role,
            role_profile,
            warning: handle.warning,
            model_fallback: handle.model_fallback,
            budget_notice: handle.budget_notice,
        })
    }

    pub async fn spawn_agent(
        &self,
        params: SpawnAgentParams,
        ready_tx: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
    ) -> Result<SpawnResult, String> {
        self.spawn_agent_with_state_change(params, ready_tx, true)
            .await
    }

    pub(crate) async fn spawn_agent_with_state_change(
        &self,
        mut params: SpawnAgentParams,
        ready_tx: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
        emit_state_change: bool,
    ) -> Result<SpawnResult, String> {
        params.title = spawn_title::normalize_agent_title(params.title.take());
        params.normalize_role_profile()?;
        crate::service::spawn_plan_task::validate_worker_plan_task_spawn(&params)?;
        self.preflight_worker_plan_task_spawn(&params)?;
        self.prepare_worker_plan_task_spawn(&mut params)?;
        let worker_persona_reservation = self.reserve_worker_persona(&mut params)?;
        // Orchestrators must be pinned to an explicit workspace.
        // Active-workspace fallback can silently land them in the wrong plan context.
        if params.spawns_anchor()
            && params.workspace_id.is_none()
            && params.parent_tab_id.is_none()
        {
            return Err("--workspace is required when spawning --role Orchestrator \
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
        let default_provider = crate::config::default_agent_provider();
        let agent_cmd = params
            .agent
            .clone()
            .unwrap_or_else(|| default_provider.clone());
        self.move_chatbackend_task_to_system_prompt(&agent_cmd, &mut params)?;
        let is_shell = self.classify_spawn_target_for_params(&agent_cmd, &params)?;
        if is_shell {
            let role = params.role.clone();
            let role_profile = params.role_profile.clone();
            let model = params.model.clone();
            let title = params
                .title
                .clone()
                .or_else(|| params.agent_name.clone())
                .unwrap_or_else(|| agent_cmd.clone());
            let bind_params = params.clone();
            let spec = crate::service::spawn::SpawnTabSpec::from_agent_params_with_chatbackends(
                self.chatbackend_registry.as_ref(),
                params,
            );
            let svc = self.clone();
            let handle = tokio::task::spawn_blocking(move || {
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
            self.bind_worker_plan_task_or_rollback_tab(&bind_params, &handle.tab_id)?;
            if let Some(reservation) = worker_persona_reservation {
                reservation.commit_to_tab(&handle.tab_id);
            }
            self.record_kpi_spawn(
                &handle.tab_id,
                requested_model.as_deref(),
                bind_params.task.as_deref().unwrap_or_default().len(),
            );
            return Ok(SpawnResult {
                tab_id: handle.tab_id,
                pty_id: handle.pty_id.unwrap_or_default(),
                agent: agent_cmd,
                model: None,
                planned_model: bind_params.planned_model.clone(),
                requested_model,
                provider_resolved_model: None,
                actual_model: None,
                reasoning_effort: None,
                reasoning_effort_provenance: ReasoningEffortProvenance::default(),
                title,
                description: handle.description,
                role,
                role_profile,
                warning: handle.warning,
                model_fallback: handle.model_fallback,
                budget_notice: handle.budget_notice,
            });
        }

        let cli_warning =
            crate::service::spawn::provider_cli_install_warning(&self.agent_registry, &agent_cmd)?;
        if let Some(ref warning) = cli_warning {
            log::warn!("[spawn] {}", warning);
        }
        let mut model_resolution = crate::service::spawn::resolve_fresh_agent_model_with_notice(
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
        crate::service::spawn::validate_agent_model(
            &self.agent_registry,
            &agent_cmd,
            params.model.as_deref(),
        )?;
        crate::service::spawn::validate_agent_reasoning_effort(
            &self.agent_registry,
            &agent_cmd,
            params.reasoning_effort.as_deref(),
        )?;
        // 1. Find workspace — explicit workspace_id wins over the parent's workspace.
        let (ws_id, ws_cwd) = {
            let mux = self.mux.write();
            let id = crate::service::spawn::resolve_spawn_workspace_id(
                params.workspace_id.clone(),
                params.parent_tab_id.as_deref(),
                |pid| mux.find_workspace_for_tab(pid),
                mux.get_active_workspace().map(|w| w.id.clone()),
                mux.list_workspaces().first().map(|w| w.id.clone()),
            )?;
            let cwd = mux.get_workspace(&id).and_then(|ws| ws.cwd.clone());
            (id, cwd)
        };

        let (resolved_cwd, cwd_warning) = Self::resolve_agent_cwd(&params.cwd, &ws_cwd);
        let warning = match (cwd_warning, cli_warning) {
            (Some(a), Some(b)) => Some(format!("{}\n{}", a, b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };
        params.cwd = resolved_cwd;

        let default_chat_view = crate::service::spawn::should_default_agent_tab_to_chat(
            crate::service::spawn::TabVisibility::Visible,
            self.agent_registry
                .with_provider(&agent_cmd, |p| p.supports_chat_ui())
                .unwrap_or(false),
        );

        let tab_id = {
            let mut mux = self.mux.write();
            let result = match params.existing_tab_id.as_deref() {
                Some(existing) => mux.claim_existing_tab(&ws_id, existing)?,
                None => mux.create_tab(&ws_id)?,
            };
            if let Some(ref t) = params.title {
                if !t.trim().is_empty() {
                    let _ = mux.rename_tab_at_level(
                        &result,
                        t,
                        crate::multiplexer::types::TitleLevel::User,
                    );
                }
            }
            if let Some(ref dir) = params.cwd {
                if let Some(tab) = mux.get_tab_mut(&result) {
                    tab.cwd = Some(dir.clone());
                }
            }
            if let Some(tab) = mux.get_tab_mut(&result) {
                initialize_ordinary_pty_agent_tab(tab, &params);
            }
            if params.activate.unwrap_or(false) {
                let _ = mux.set_active_tab(&ws_id, &result);
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
            if let Some(ref peer) = params.report_target_mesh_peer {
                if !peer.is_empty() {
                    if let Some(tab) = mux.get_tab_mut(&result) {
                        tab.report_target_mesh_peer = Some(peer.clone());
                    }
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
            if let Some(ref ownership) = params.ownership {
                if let Some(tab) = mux.get_tab_mut(&result) {
                    tab.ownership = Some(ownership.clone());
                }
            }
            result
        };
        if emit_state_change {
            self.emit_state_changed();
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
                let _ = self.close_tab(&tab_id);
                return Err(error);
            }
        }

        let permission_bypass_authorization =
            crate::service::permission_policy::configured_permission_bypass(
                &self.agent_registry,
                &agent_cmd,
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
                crate::multiplexer::types::TitleLevel::User => self.rename_tab(&tab_id, &title)?,
                _ => self.set_tab_title(&tab_id, &title)?,
            }
        }

        // Pin agent_type before launch so detect_from_output can start session-id polling.
        // Without this tag, restarts can lose session_id and resume fails silently.
        self.set_and_lock_tab_agent_type(&tab_id, agent_cmd.to_string());

        // 4c. Pin agent_session_id for resume up-front: we know the id from the user's request, so don't wait for detect_session_id polling.
        let launch_session_id = if params.resume_session_id.is_none() {
            self.agent_registry
                .with_provider(&agent_cmd, |p| p.session_id_injection())
                .filter(|mode| !matches!(mode, crate::agent::provider::SessionIdInjection::None))
                .map(|_| uuid::Uuid::new_v4().to_string())
        } else {
            None
        };
        if let Some(ref sid) = params.resume_session_id {
            self.set_agent_session_id(&tab_id, Some(sid.clone()));
        } else if let Some(ref sid) = launch_session_id {
            self.set_agent_session_id(&tab_id, Some(sid.clone()));
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

        params.structured_prompt_server = crate::service::structured_prompt::ensure_for_spawn(
            self,
            &agent_cmd,
            &tab_id,
            &params.cwd.clone().unwrap_or_default(),
        );

        let fallback_bootstrap = if params.manager_bootstrap.is_some() {
            self.emit_boot_stage(
                &tab_id,
                crate::service::boot_stage::BootStage::PreparingRoleInstructions,
                None,
            );
            prepare_manager_bootstrap(&mut params)
        } else {
            prepare_standard_starter_prompt(&mut params);
            None
        };

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
        let bg_task = if params.manager_bootstrap.is_some() {
            if system_prompt_capable {
                Some(agent_system_prompt(params.task.as_deref()))
            } else {
                params.task.clone()
            }
        } else {
            spawn_launch_task(&params, system_prompt_capable)
        };
        // Planner gets a short display hint (pending_greeting, user-facing only)
        // plus its FULL onboarding as real launch input attributed to CodeTerm —
        // never a FROM-<orchestrator> relay, mirroring the Orchestrator's onboarding.
        let is_planner = params.role_kind() == Some(builtins::PLANNER);
        // A pair-spawned Orchestrator carries its onboarding in the launch task and
        // skips the separate deliver — attribute that onboarding to CodeTerm too, so
        // BOTH roles get an emergent "FROM CodeTerm" message on any path to a role.
        let is_orch_onboarding =
            params.spawns_anchor() && params.skip_orchestrator_onboarding == Some(true);
        let codeterm_pfx = crate::service::messenger::Messenger::codeterm_prefix();
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
                    let onboarding = if system_prompt_capable {
                        agent_system_prompt(Some(&onboard))
                    } else {
                        onboard
                    };
                    Some(format!("{codeterm_pfx}{onboarding}"))
                }
                None => bg_task,
            }
        } else if is_orch_onboarding {
            bg_task.map(|t| format!("{codeterm_pfx}{t}"))
        } else {
            bg_task
        };
        // Tag with the spawner's origin prefix (FROM-<spawner>) for every provider —
        // but role onboarding is already CodeTerm-attributed, so skip it there.
        let bg_task = if is_planner || is_orch_onboarding {
            bg_task
        } else {
            bg_task.map(|t| match self.spawn_task_prefix(&tab_id) {
                Some(pfx) => format!("{}{}", pfx, t),
                None => t,
            })
        };
        self.bind_worker_plan_task_or_rollback_tab(&params, &tab_id)?;
        let fallback_delivery = self
            .agent_registry
            .with_provider(&agent_cmd, |provider| {
                let external = matches!(
                    crate::service::launch_delivery::mode_for_descriptor(
                        &provider.system_prompt_delivery(),
                    ),
                    crate::service::launch_delivery::LaunchDeliveryMode::ConfirmedFallback
                );
                external
                    && matches!(
                        provider.task_delivery(),
                        crate::agent::provider::TaskDelivery::PtyInput
                    )
            })
            .unwrap_or(true);
        let fallback_spec = fallback_delivery
            .then(|| {
                crate::service::launch_delivery::fallback_delivery_spec(
                    fallback_bootstrap.as_deref(),
                    bg_task.as_deref(),
                    self.agent_registry
                        .with_provider(&agent_cmd, |provider| {
                            provider.starter_prompt_text(params.starter_prompt)
                        })
                        .flatten()
                        .as_deref(),
                )
            })
            .flatten();
        if let Some(spec) = fallback_spec.as_ref() {
            if let Err(error) =
                self.create_pending_delivery(&tab_id, spec.channel.clone(), &spec.payload)
            {
                let _ = self.close_tab(&tab_id);
                return Err(error.to_string());
            }
        }
        let launch_cmd = build_agent_command(
            &agent_cmd,
            &params,
            bg_task.as_deref(),
            launch_session_id.as_deref(),
            &self.agent_registry,
        );
        launch_agent_in_background(
            self.clone(),
            tab_id.clone(),
            launch_cmd,
            agent_cmd.clone(),
            crate::service::launch_diagnostics::AgentLaunchOrigin::OrchestratedSpawn,
            bg_task,
            fallback_spec,
            ready_tx,
        );

        // (subscriber already started inside ensure_for_spawn above)

        let title = self
            .mux
            .read()
            .get_tab(&tab_id)
            .map(|p| p.title.clone())
            .unwrap_or_default();

        if let Some(reservation) = worker_persona_reservation {
            reservation.commit_to_tab(&tab_id);
        }
        let budget_notice = self.workspace_tab_budget_notice(&ws_id);
        let reasoning_effort = params.reasoning_effort.clone();
        self.record_kpi_spawn(
            &tab_id,
            model_resolution.model.as_deref(),
            params.task.as_deref().unwrap_or_default().len(),
        );
        Ok(SpawnResult {
            tab_id,
            pty_id: String::new(), // PTY not yet assigned — happens async
            agent: agent_cmd,
            model: model_resolution.model.clone(),
            planned_model: params.planned_model,
            requested_model: model_resolution.requested_model,
            provider_resolved_model: model_resolution.model,
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
    }

    fn assign_spawn_role_hierarchy(
        &self,
        tab_id: &str,
        role_str: &str,
        role_profile: Option<&str>,
        parent_tab_id: Option<&str>,
        plan_id: Option<&str>,
        can_spawn: Option<bool>,
        emit_state_change: bool,
    ) -> Result<(), String> {
        let (role, profile) = Self::parse_role_kind(role_str, role_profile)?;
        // A Planner names its plan, not a parent: the plan record owns the
        // anchor, and placement + planner binding commit as one transaction.
        if profile.as_deref() == Some(builtins::PLANNER) && parent_tab_id.is_none() {
            let plan_id = plan_id
                .map(str::trim)
                .filter(|plan_id| !plan_id.is_empty())
                .ok_or("Planner requires --plan <id> (or an explicit --parent)")?;
            self.place_planner_on_plan(tab_id, plan_id, false)?;
            return Ok(());
        }
        if emit_state_change {
            self.set_tab_role_with_profile(
                tab_id,
                role_str,
                role_profile,
                parent_tab_id,
                can_spawn,
            )?;
        } else {
            let mut mux = self.mux.write();
            mux.set_tab_role_kind(tab_id, role, profile.as_deref(), parent_tab_id)?;
            if let Some(can_spawn) = can_spawn {
                mux.set_can_spawn(tab_id, can_spawn)?;
            }
            drop(mux);
            self.record_role_transition(tab_id);
        }
        if let Some(parent) = parent_tab_id {
            self.expect_spawn_parent_link(tab_id, parent)?;
        }
        Ok(())
    }

    fn tab_model_identity(
        &self,
        tab_id: &str,
    ) -> (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    ) {
        self.mux
            .read()
            .get_tab(tab_id)
            .map(|tab| {
                (
                    tab.agent_planned_model.clone(),
                    tab.agent_requested_model.clone(),
                    tab.agent_provider_resolved_model.clone(),
                    tab.agent_session_model.clone(),
                )
            })
            .unwrap_or_default()
    }

    fn persist_unresolved_plugin_model_request(&self, tab_id: &str, params: &SpawnAgentParams) {
        let mut mux = self.mux.write();
        let Some(tab) = mux.get_tab_mut(tab_id) else {
            return;
        };
        if tab.agent_requested_model.is_some() {
            return;
        }
        tab.agent_planned_model = params
            .planned_model
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        tab.agent_requested_model = params
            .model
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
    }

    fn bind_planner_spawn(&self, plan_id: Option<&str>, role_profile: Option<&str>, tab_id: &str) {
        if role_profile != Some(builtins::PLANNER) {
            return;
        }
        let Some(plan_id) = plan_id.filter(|id| !id.trim().is_empty()) else {
            return;
        };
        // The atomic Planner placement already bound this tab; re-writing the
        // ref here would replace a committed binding outside its transaction.
        let already_bound = matches!(
            self.plan_store.get(plan_id).and_then(|plan| plan.planner),
            Some(crate::plan::PlannerRef::Tab { tab_id: bound, .. }) if bound == tab_id
        );
        if already_bound {
            return;
        }
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        match self.plan_store.set_planner_ref(
            plan_id,
            Some(crate::plan::PlannerRef::Tab {
                tab_id: tab_id.to_string(),
                session_id: None,
                title: None,
            }),
            now_ms,
        ) {
            Ok(()) => crate::api::events::emit_everywhere(
                &self.app_handle,
                "plan-changed",
                serde_json::json!({ "plan_id": plan_id }),
            ),
            Err(e) => log::warn!("[spawn] failed to bind planner ref for plan {plan_id}: {e}"),
        }
    }

    /// Origin prefix for a spawned agent's first task, derived from the target tab's recorded spawner (spawned_by/parent) — same canonical attribution as cross-tab sends. None for raw spawns (no spawner).
    pub(crate) fn spawn_task_prefix(&self, target_tab: &str) -> Option<String> {
        let mux = self.mux.write();
        let src = mux.get_tab(target_tab).and_then(|p| {
            p.spawned_by_tab_id
                .clone()
                .or_else(|| p.parent_tab_id.clone())
        })?;
        if src.is_empty() {
            return None;
        }
        let label = mux.display_label_for_tab(&src);
        let model = mux
            .get_tab(&src)
            .and_then(|p| p.agent_session_model.clone());
        crate::service::messenger::delivery_prefix(
            true,
            &src,
            target_tab,
            &label,
            model.as_deref(),
            None,
        )
    }
}

impl ServiceContext {
    fn move_chatbackend_task_to_system_prompt(
        &self,
        agent: &str,
        params: &mut SpawnAgentParams,
    ) -> Result<(), String> {
        if !self.provider_registry.is_spawnable_chat_backend(agent) {
            return Ok(());
        }
        params.system_prompt = compose_chatbackend_system_prompt(params);
        Ok(())
    }
}

/// Every params-owned field on a fresh ordinary-PTY agent tab is applied here;
/// a second init site is how a declared field (`audience`) once got dropped.
pub(crate) fn initialize_ordinary_pty_agent_tab(
    tab: &mut crate::multiplexer::types::Tab,
    params: &SpawnAgentParams,
) {
    tab.role_profile = params.role_profile.clone();
    tab.audience = params.audience;
    tab.agent_booting = true;
    crate::service::boot_stage::apply_boot_stage(
        tab,
        crate::service::boot_stage::BootStage::PtyStarting,
        None,
    );
}

/// Deterministic hierarchy defaults: a local Worker with no explicit --parent
/// belongs to its CALLER, and reports route to its parent — never to whichever
/// other orchestrator happens to share the workspace.
fn apply_worker_hierarchy_defaults(params: &mut SpawnAgentParams) {
    if params.role.as_deref() != Some("Worker") || params.report_target_mesh_peer.is_some() {
        return;
    }
    if params.parent_tab_id.is_none() {
        params.parent_tab_id = params.spawned_by_tab_id.clone().filter(|sb| !sb.is_empty());
    }
    if params.report_target_tab_id.is_none() {
        params.report_target_tab_id = params.parent_tab_id.clone();
    }
}

#[cfg(test)]
mod hierarchy_defaults_tests {
    use super::*;

    #[test]
    fn effort_provenance_keeps_plan_and_request_distinct() {
        let planned = resolved_effort_provenance(Some("high".into()), Some("medium".into()));
        assert_eq!(planned.planned.as_deref(), Some("high"));
        assert_eq!(planned.requested.as_deref(), Some("medium"));
        assert_eq!(planned.resolved.as_deref(), Some("medium"));

        let ordinary = resolved_effort_provenance(None, Some("high".into()));
        assert_eq!(ordinary.planned, None);
        assert_eq!(ordinary.requested.as_deref(), Some("high"));
    }

    fn worker_params(spawned_by: Option<&str>) -> SpawnAgentParams {
        SpawnAgentParams {
            role: Some("Worker".into()),
            spawned_by_tab_id: spawned_by.map(str::to_string),
            ..Default::default()
        }
    }

    // Live bounce oracle (dev aa793130a, notification 958a78b0): POST
    // /api/agents/spawn with audience=automation still minted a user bell.
    #[test]
    fn ordinary_pty_tab_creation_carries_automation_audience_from_params() {
        use crate::multiplexer::types::TabAudience;
        let mut state = crate::multiplexer::state::MultiplexerState::empty();
        let ws_id = state.create_workspace("ws".to_string(), None).id;
        let tab_id = state.create_tab(&ws_id).expect("tab created");
        let params = SpawnAgentParams {
            agent: Some("claude".into()),
            audience: TabAudience::Automation,
            ..Default::default()
        };

        initialize_ordinary_pty_agent_tab(state.get_tab_mut(&tab_id).expect("tab"), &params);

        assert_eq!(
            state.get_tab(&tab_id).expect("tab").audience,
            TabAudience::Automation,
            "audience declared on SpawnAgentParams must survive into the ordinary PTY tab record"
        );
    }

    // Negative control: an undeclared spawn stays user-facing on the same path.
    #[test]
    fn ordinary_pty_tab_creation_defaults_to_user_audience() {
        use crate::multiplexer::types::TabAudience;
        let mut state = crate::multiplexer::state::MultiplexerState::empty();
        let ws_id = state.create_workspace("ws".to_string(), None).id;
        let tab_id = state.create_tab(&ws_id).expect("tab created");
        let params = SpawnAgentParams {
            agent: Some("claude".into()),
            ..Default::default()
        };

        initialize_ordinary_pty_agent_tab(state.get_tab_mut(&tab_id).expect("tab"), &params);

        assert_eq!(
            state.get_tab(&tab_id).expect("tab").audience,
            TabAudience::User,
            "an undeclared spawn keeps the user audience and still bells"
        );
    }

    #[test]
    fn worker_without_parent_defaults_to_caller_and_reports_to_parent() {
        let mut p = worker_params(Some("caller-tab"));
        apply_worker_hierarchy_defaults(&mut p);
        assert_eq!(p.parent_tab_id.as_deref(), Some("caller-tab"));
        assert_eq!(p.report_target_tab_id.as_deref(), Some("caller-tab"));
    }

    #[test]
    fn explicit_parent_and_report_target_are_untouched() {
        let mut p = worker_params(Some("caller-tab"));
        p.parent_tab_id = Some("boss".into());
        p.report_target_tab_id = Some("boss".into());
        apply_worker_hierarchy_defaults(&mut p);
        assert_eq!(p.parent_tab_id.as_deref(), Some("boss"));
        assert_eq!(p.report_target_tab_id.as_deref(), Some("boss"));
    }

    #[test]
    fn cross_mesh_and_non_worker_are_skipped() {
        let mut mesh = worker_params(Some("caller"));
        mesh.report_target_mesh_peer = Some("peer".into());
        apply_worker_hierarchy_defaults(&mut mesh);
        assert_eq!(mesh.parent_tab_id, None);

        let mut orch = SpawnAgentParams {
            role: Some("Orchestrator".into()),
            spawned_by_tab_id: Some("caller".into()),
            ..Default::default()
        };
        apply_worker_hierarchy_defaults(&mut orch);
        assert_eq!(orch.parent_tab_id, None);
    }

    #[test]
    fn chatbackend_worker_receives_identity_context_contract_and_assignment_once() {
        let mut params = SpawnAgentParams {
            role: Some("Worker".into()),
            task: Some("repair transport".into()),
            ..Default::default()
        };

        let prompt = compose_chatbackend_system_prompt(&mut params).unwrap();

        assert!(prompt.starts_with("You are a CodeTerm Worker."));
        assert_eq!(
            prompt
                .matches(crate::prompts::WORKER_CODETERM_CONTEXT.trim())
                .count(),
            1
        );
        assert!(!prompt.contains(crate::prompts::AGENT_CODETERM_PRIMER.trim()));
        assert_eq!(
            prompt.matches(crate::prompts::WORKER_SUFFIX.trim()).count(),
            1
        );
        assert_eq!(prompt.matches("repair transport").count(), 1);
        assert_eq!(params.task, None);
    }

    #[test]
    fn idle_manager_bootstrap_is_one_team_launch_turn_with_coordination() {
        let coordination = "Your Planner counterpart is Euler (tab planner-1).";
        let mut params = SpawnAgentParams {
            manager_bootstrap: Some(crate::service::role_identity::ManagerBootstrapInputs {
                role: "Orchestrator".into(),
                team_id: "not yet established".into(),
                team_name: "not yet established".into(),
                plan_id: "no plan bound yet".into(),
                project: "CodeTerm".into(),
                coordination: Some(coordination.into()),
            }),
            ..Default::default()
        };

        let bootstrap = prepare_manager_bootstrap(&mut params).unwrap();
        assert_eq!(
            params.starter_prompt,
            crate::agent::provider::StarterPrompt::TeamBootstrap
        );
        assert_eq!(params.task.as_deref(), Some(bootstrap.as_str()));
        assert_eq!(bootstrap.matches(coordination).count(), 1);
    }

    #[test]
    fn assigned_manager_bootstrap_keeps_the_assigned_task_starter() {
        let mut params = SpawnAgentParams {
            task: Some("Audit the delivery path.".into()),
            manager_bootstrap: Some(crate::service::role_identity::ManagerBootstrapInputs {
                role: "Orchestrator".into(),
                team_id: "team-1".into(),
                team_name: "Delivery".into(),
                plan_id: "plan-1".into(),
                project: "CodeTerm".into(),
                coordination: None,
            }),
            ..Default::default()
        };

        let bootstrap = prepare_manager_bootstrap(&mut params).unwrap();
        let expected = format!("{bootstrap}\n\nAudit the delivery path.");
        assert_eq!(
            params.starter_prompt,
            crate::agent::provider::StarterPrompt::AssignedTask
        );
        assert_eq!(params.task.as_deref(), Some(expected.as_str()));
    }

    #[test]
    fn roleless_agent_starts_without_a_visible_turn() {
        let mut params = SpawnAgentParams::default();

        prepare_standard_starter_prompt(&mut params);

        assert_eq!(
            params.starter_prompt,
            crate::agent::provider::StarterPrompt::NoStarter
        );
    }

    #[test]
    fn roleless_task_is_system_context_without_a_visible_turn() {
        let mut params = SpawnAgentParams {
            task: Some("Inspect the launch path.".into()),
            ..Default::default()
        };

        prepare_standard_starter_prompt(&mut params);

        assert_eq!(
            params.starter_prompt,
            crate::agent::provider::StarterPrompt::NoStarter
        );
    }

    #[test]
    fn role_assignment_keeps_the_assigned_task_turn() {
        let mut params = SpawnAgentParams {
            task: Some("Inspect the launch path.".into()),
            role: Some("Worker".into()),
            ..Default::default()
        };

        prepare_standard_starter_prompt(&mut params);

        assert_eq!(
            params.starter_prompt,
            crate::agent::provider::StarterPrompt::AssignedTask
        );
    }

    // No agent cwd and no workspace cwd must still pin a concrete dir (the
    // daemon's own cwd, what the PTY inherits). Leaving it `None` desyncs the
    // tab record from where the agent writes its session store, so the chat
    // pipeline resolves the wrong `~/.claude/projects/<encoded>` dir.
    #[test]
    fn no_cwd_and_no_workspace_pins_the_daemon_cwd() {
        let (resolved, warning) = ServiceContext::resolve_agent_cwd(&None, &None);
        assert!(warning.is_none());
        let expected = std::env::current_dir()
            .ok()
            .and_then(|p| p.to_str().map(String::from));
        assert_eq!(resolved, expected);
        assert!(
            resolved.as_deref().is_none_or(|d| !d.is_empty()),
            "resolved cwd must never be an empty string"
        );
    }
}

#[cfg(unix)]
fn free_disk_gb(path: &str) -> Option<u64> {
    let c_path = std::ffi::CString::new(path).ok()?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) } != 0 {
        return None;
    }
    Some((stat.f_bavail as u64).saturating_mul(stat.f_frsize as u64) / 1_073_741_824)
}

#[cfg(not(unix))]
fn free_disk_gb(_path: &str) -> Option<u64> {
    None
}
