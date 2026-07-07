use super::launch::{
    build_agent_command, launch_agent_in_background, planner_profile_task, role_onboarding_task,
    skip_local_worker_role,
};
use super::types::{SpawnAgentParams, SpawnResult};
use crate::service::{spawn_title, ServiceContext};
use tauri::Manager;

impl ServiceContext {
    /// Resolve agent cwd against workspace cwd:
    /// - If no cwd specified → inherit from workspace
    /// - If cwd differs from workspace → return warning
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
            (None, None) => (None, None),
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
        let checkout_exception = self.worker_checkout_guard(&params)?;
        let role = params.role.clone();
        let role_profile = params.role_profile.clone();
        let plan_id = params.plan_id.clone();
        let agent = params
            .agent
            .clone()
            .unwrap_or_else(crate::config::default_agent_provider);
        let title = params
            .title
            .clone()
            .or_else(|| params.agent_name.clone())
            .unwrap_or_else(|| agent.clone());
        let model = params.model.clone();
        self.classify_spawn_target_for_params(&agent, &params)?;
        let spec = crate::service::spawn::SpawnPaneSpec::from_agent_params_with_chatbackends(
            self.chatbackend_registry.as_ref(),
            params,
        );
        let handle = self.spawn_pane_with_ready(spec, ready_tx)?;
        if let Some(exc) = checkout_exception {
            self.record_shared_checkout_exception(&exc, &handle.pane_id);
        }
        self.bind_planner_spawn(plan_id.as_deref(), role_profile.as_deref(), &handle.pane_id);
        Ok(SpawnResult {
            pane_id: handle.pane_id,
            tab_id: handle.tab_id,
            pty_id: handle.pty_id.unwrap_or_default(),
            agent,
            model,
            title,
            description: handle.description,
            role,
            role_profile,
            warning: handle.warning,
            budget_notice: handle.budget_notice,
        })
    }

    pub async fn spawn_agent(
        &self,
        mut params: SpawnAgentParams,
        ready_tx: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
    ) -> Result<SpawnResult, String> {
        params.title = spawn_title::normalize_agent_title(params.title.take());
        params.normalize_role_profile()?;
        let checkout_exception = self.worker_checkout_guard(&params)?;
        // Orchestrators must be pinned to an explicit workspace.
        // Active-workspace fallback can silently land them in the wrong plan context.
        if params.role.as_deref() == Some("Orchestrator")
            && params.workspace_id.is_none()
            && params.parent_pane_id.is_none()
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
            let spec = crate::service::spawn::SpawnPaneSpec::from_agent_params_with_chatbackends(
                self.chatbackend_registry.as_ref(),
                params,
            );
            let handle = self.spawn_pane_with_ready(spec, ready_tx)?;
            return Ok(SpawnResult {
                pane_id: handle.pane_id,
                tab_id: handle.tab_id,
                pty_id: handle.pty_id.unwrap_or_default(),
                agent: agent_cmd,
                model,
                title,
                description: handle.description,
                role,
                role_profile,
                warning: handle.warning,
                budget_notice: handle.budget_notice,
            });
        }

        let cli_warning =
            crate::service::spawn::provider_cli_install_warning(&self.agent_registry, &agent_cmd)?;
        if let Some(ref warning) = cli_warning {
            log::warn!("[spawn] {}", warning);
        }
        params.model = crate::service::spawn::resolve_fresh_agent_model(
            &self.agent_registry,
            &agent_cmd,
            params.model.take(),
            params.resume_session_id.as_deref(),
        );
        crate::service::spawn::validate_agent_model(
            &self.agent_registry,
            &agent_cmd,
            params.model.as_deref(),
        )?;
        // 1. Find workspace — explicit workspace_id wins over the parent's workspace.
        let (ws_id, ws_cwd) = {
            let mux = self.mux.write();
            let id = crate::service::spawn::resolve_spawn_workspace_id(
                params.workspace_id.clone(),
                params.parent_pane_id.as_deref(),
                |pid| mux.find_workspace_for_pane(pid),
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

        let default_chat_enabled = self
            .app_handle
            .state::<crate::config::ConfigState>()
            .lock()
            .ui
            .default_new_agent_pane_chat;
        let default_chat_view = crate::service::spawn::should_default_agent_pane_to_chat(
            default_chat_enabled,
            crate::service::spawn::PaneVisibility::Visible,
            self.agent_registry
                .with_provider(&agent_cmd, |p| p.supports_chat_ui())
                .unwrap_or(false),
        );

        let (tab_id, pane_id) = {
            let mut mux = self.mux.write();
            let result = match params.existing_tab_id.as_deref() {
                Some(existing) => mux.claim_existing_tab(&ws_id, existing)?,
                None => mux.create_tab(&ws_id)?,
            };
            if let Some(ref t) = params.title {
                if !t.trim().is_empty() {
                    let _ = mux.rename_pane_at_level(
                        &result.1,
                        t,
                        crate::multiplexer::types::TitleLevel::User,
                    );
                }
            }
            if let Some(ref dir) = params.cwd {
                if let Some(pane) = mux.get_pane_mut(&result.1) {
                    pane.cwd = Some(dir.clone());
                }
            }
            if let Some(pane) = mux.get_pane_mut(&result.1) {
                pane.role_profile = params.role_profile.clone();
            }
            if params.activate.unwrap_or(false) {
                let _ = mux.set_active_tab(&ws_id, &result.0);
            }
            if default_chat_view {
                if let Some(pane) = mux.get_pane_mut(&result.1) {
                    pane.chat_view = true;
                }
            }
            if let Some(ref rtid) = params.report_target_pane_id {
                if let Some(pane) = mux.get_pane_mut(&result.1) {
                    pane.report_target_pane_id = Some(rtid.clone());
                }
            }
            if let Some(ref peer) = params.report_target_mesh_peer {
                if !peer.is_empty() {
                    if let Some(pane) = mux.get_pane_mut(&result.1) {
                        pane.report_target_mesh_peer = Some(peer.clone());
                    }
                }
            }
            // Record the spawning agent for report routing fallback, independent of explicit --parent / role hierarchy.
            if let Some(ref sb) = params.spawned_by_pane_id {
                if !sb.is_empty() {
                    if let Some(pane) = mux.get_pane_mut(&result.1) {
                        pane.spawned_by_pane_id = Some(sb.clone());
                    }
                }
            }
            result
        };
        self.emit_state_changed();

        // (its hierarchy lives on the origin) — skip_local_worker_role avoids a 500.
        if let Some(ref role_str) = params.role {
            let role = Self::parse_role(role_str)?;
            if skip_local_worker_role(
                &role,
                params.parent_pane_id.as_deref(),
                params.report_target_mesh_peer.as_deref(),
            ) {
                log::info!(
                    "[spawn] cross-mesh worker {} → {}@{}: skipping local Worker hierarchy (no local parent)",
                    pane_id,
                    params.report_target_pane_id.as_deref().unwrap_or("?"),
                    params.report_target_mesh_peer.as_deref().unwrap_or("?"),
                );
            } else {
                self.set_pane_role(
                    &pane_id,
                    role_str,
                    params.parent_pane_id.as_deref(),
                    params.can_spawn,
                )?;
                if role_str == "Worker" || role_str == "Manager" {
                    if let Some(ref parent) = params.parent_pane_id {
                        self.expect_spawn_parent_link(&pane_id, parent)?;
                    }
                }
            }
        }

        if params.skip_permissions.unwrap_or(true) {
            let mut mux = self.mux.write();
            if let Some(pane) = mux.get_pane_mut(&pane_id) {
                pane.auto_approve = true;
            }
        }

        if let Some((title, level)) = crate::service::spawn_title::creation_title_override(
            params.title.as_deref(),
            params.agent_name.as_deref(),
        ) {
            match level {
                crate::multiplexer::types::TitleLevel::User => {
                    self.rename_pane(&pane_id, &title)?
                }
                _ => self.set_pane_title(&pane_id, &title)?,
            }
        }

        // Pin agent_type before launch so detect_from_output can start session-id polling.
        // Without this tag, restarts can lose session_id and resume fails silently.
        self.set_and_lock_pane_agent_type(&pane_id, agent_cmd.to_string());
        if params
            .role
            .as_deref()
            .and_then(|role| Self::parse_role(role).ok())
            == Some(crate::multiplexer::types::AgentRole::Orchestrator)
            && !params.skip_orchestrator_onboarding.unwrap_or(false)
        {
            self.maybe_deliver_orchestrator_promotion_onboarding(&pane_id, false);
        }

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
            self.set_agent_session_id(&pane_id, Some(sid.clone()));
        } else if let Some(ref sid) = launch_session_id {
            self.set_agent_session_id(&pane_id, Some(sid.clone()));
        }
        if let Some(model) = params.model.as_ref().filter(|m| !m.trim().is_empty()) {
            if let Some(pane) = self.mux.write().get_pane_mut(&pane_id) {
                pane.agent_session_model = Some(model.trim().to_string());
            }
        }

        params.structured_prompt_server = crate::service::structured_prompt::ensure_for_spawn(
            self,
            &agent_cmd,
            &pane_id,
            &params.cwd.clone().unwrap_or_default(),
        );

        let bg_task = role_onboarding_task(
            params.task.as_deref(),
            params.role.as_deref(),
            params.role_profile.as_deref(),
        );
        // this pane and tells the Planner who its Orchestrator peer is.
        let bg_task = if params.role_profile.as_deref() == Some("planner") {
            match params.plan_id.as_deref() {
                Some(plan_id) => {
                    self.bind_planner_spawn(Some(plan_id), Some("planner"), &pane_id);
                    let orch_pane = self.plan_store.owner_of(plan_id).and_then(|o| o.pane_id);
                    // assistant message so a bound Planner never boots silent.
                    let orch_label = orch_pane
                        .as_deref()
                        .map(|p| self.mux.read().display_label_for_pane(p))
                        .unwrap_or_else(|| "your Orchestrator".to_string());
                    if let Some(pane) = self.mux.write().get_pane_mut(&pane_id) {
                        pane.pending_greeting = Some(crate::prompts::peer_planner_greeting(
                            &orch_label,
                            orch_pane.as_deref().unwrap_or("unknown"),
                        ));
                    }
                    let intro = crate::service::plan::peer_mandate::planner_peer_intro_line(
                        plan_id,
                        orch_pane.as_deref(),
                    );
                    Some(planner_profile_task(intro, bg_task))
                }
                None => bg_task,
            }
        } else {
            bg_task
        };
        // Planner-start injection point: pull machine-local orchestration preferences off the hot path.
        if params.role_profile.as_deref() == Some("planner") {
            self.deliver_orchestration_preferences(&pane_id);
        }
        // Tag with the spawner's origin prefix uniformly for every provider; delivery handles the marker per provider (TUI-typed for PtyInput, positional after `--` for inline-flag).
        let bg_task = bg_task.map(|t| match self.spawn_task_prefix(&pane_id) {
            Some(pfx) => format!("{}{}", pfx, t),
            None => t,
        });
        let launch_cmd = build_agent_command(
            &agent_cmd,
            &params,
            bg_task.as_deref(),
            launch_session_id.as_deref(),
            &self.agent_registry,
        );
        launch_agent_in_background(
            self.clone(),
            pane_id.clone(),
            tab_id.clone(),
            ws_id.clone(),
            launch_cmd,
            agent_cmd.clone(),
            bg_task,
            ready_tx,
        );

        // (subscriber already started inside ensure_for_spawn above)

        if let Some(exc) = checkout_exception {
            self.record_shared_checkout_exception(&exc, &pane_id);
        }

        let title = self
            .mux
            .read()
            .get_pane(&pane_id)
            .map(|p| p.title.clone())
            .unwrap_or_default();

        let budget_notice = self.workspace_pane_budget_notice(&ws_id);
        Ok(SpawnResult {
            pane_id,
            tab_id,
            pty_id: String::new(), // PTY not yet assigned — happens async
            agent: agent_cmd,
            model: params.model,
            title,
            description: params.description,
            role: params.role,
            role_profile: params.role_profile,
            warning,
            budget_notice,
        })
    }

    fn bind_planner_spawn(&self, plan_id: Option<&str>, role_profile: Option<&str>, pane_id: &str) {
        if role_profile != Some("planner") {
            return;
        }
        let Some(plan_id) = plan_id.filter(|id| !id.trim().is_empty()) else {
            return;
        };
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        match self.plan_store.set_planner_ref(
            plan_id,
            Some(crate::plan::PlannerRef::Pane {
                pane_id: pane_id.to_string(),
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

    /// Origin prefix for a spawned agent's first task, derived from the target pane's recorded spawner (spawned_by/parent) — same canonical attribution as cross-pane sends. None for raw spawns (no spawner).
    pub(crate) fn spawn_task_prefix(&self, target_pane: &str) -> Option<String> {
        let mux = self.mux.write();
        let src = mux.get_pane(target_pane).and_then(|p| {
            p.spawned_by_pane_id
                .clone()
                .or_else(|| p.parent_pane_id.clone())
        })?;
        if src.is_empty() {
            return None;
        }
        let label = mux.display_label_for_pane(&src);
        let model = mux
            .get_pane(&src)
            .and_then(|p| p.agent_session_model.clone());
        crate::service::messenger::delivery_prefix(
            true,
            &src,
            target_pane,
            &label,
            model.as_deref(),
            None,
        )
    }
}

/// Deterministic hierarchy defaults: a local Worker with no explicit --parent
/// belongs to its CALLER, and reports route to its parent — never to whichever
/// other orchestrator happens to share the workspace.
fn apply_worker_hierarchy_defaults(params: &mut SpawnAgentParams) {
    if params.role.as_deref() != Some("Worker") || params.report_target_mesh_peer.is_some() {
        return;
    }
    if params.parent_pane_id.is_none() {
        params.parent_pane_id = params
            .spawned_by_pane_id
            .clone()
            .filter(|sb| !sb.is_empty());
    }
    if params.report_target_pane_id.is_none() {
        params.report_target_pane_id = params.parent_pane_id.clone();
    }
}

#[cfg(test)]
mod hierarchy_defaults_tests {
    use super::*;

    fn worker_params(spawned_by: Option<&str>) -> SpawnAgentParams {
        SpawnAgentParams {
            role: Some("Worker".into()),
            spawned_by_pane_id: spawned_by.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn worker_without_parent_defaults_to_caller_and_reports_to_parent() {
        let mut p = worker_params(Some("caller-pane"));
        apply_worker_hierarchy_defaults(&mut p);
        assert_eq!(p.parent_pane_id.as_deref(), Some("caller-pane"));
        assert_eq!(p.report_target_pane_id.as_deref(), Some("caller-pane"));
    }

    #[test]
    fn explicit_parent_and_report_target_are_untouched() {
        let mut p = worker_params(Some("caller-pane"));
        p.parent_pane_id = Some("boss".into());
        p.report_target_pane_id = Some("boss".into());
        apply_worker_hierarchy_defaults(&mut p);
        assert_eq!(p.parent_pane_id.as_deref(), Some("boss"));
        assert_eq!(p.report_target_pane_id.as_deref(), Some("boss"));
    }

    #[test]
    fn cross_mesh_and_non_worker_are_skipped() {
        let mut mesh = worker_params(Some("caller"));
        mesh.report_target_mesh_peer = Some("peer".into());
        apply_worker_hierarchy_defaults(&mut mesh);
        assert_eq!(mesh.parent_pane_id, None);

        let mut orch = SpawnAgentParams {
            role: Some("Orchestrator".into()),
            spawned_by_pane_id: Some("caller".into()),
            ..Default::default()
        };
        apply_worker_hierarchy_defaults(&mut orch);
        assert_eq!(orch.parent_pane_id, None);
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
