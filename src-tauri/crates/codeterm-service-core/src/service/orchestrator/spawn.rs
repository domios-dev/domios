mod async_spawn;
#[path = "spawn/cwd.rs"]
mod cwd;
mod dispatch_rollback;
#[path = "spawn/preparation.rs"]
mod preparation;

pub(crate) mod runtime_prompt;
use dispatch_rollback::create_pending_delivery_or_rollback_dispatch;
pub use runtime_prompt::{
    compose_runtime_system_prompt, ensure_tab_runtime_prompt, ROLE_AUTHORITY,
};

use codeterm_types::agent::role_kind::builtins;
use codeterm_types::team::general_agent_membership::MembershipAttempt;

use super::launch::{launch_agent_in_background_with_task_turn, skip_local_worker_role};
use super::prompt_compose::{
    agent_system_prompt, role_onboarding_task, spawn_launch_task_for_provider,
};
use super::types::{ReasoningEffortProvenance, SpawnAgentParams, SpawnResult};
use crate::multiplexer::types::TabCloseReason;
use crate::service::general_agent::{
    refuse_general_agent_team_membership, refuse_parenting_under_general_agent, shape_for_tab,
};
use crate::service::{spawn_title, MuxEmitCause, ServiceContext};
pub(crate) use preparation::resolved_effort_provenance;
use preparation::{
    apply_startup_handshake, bootstrap_bound_team_id, build_or_refuse_orchestrated_launch_command,
    compose_chatbackend_system_prompt, planner_startup_payload, prepare_manager_bootstrap,
    prepare_standard_starter_prompt, spawn_fallback_spec, team_bootstrap_payload,
};

impl ServiceContext {
    /// Legacy wrapper for callers still on `SpawnAgentParams` (CLI/REST/batch/cron/socket).
    pub fn spawn_agent_sync(&self, params: SpawnAgentParams) -> Result<SpawnResult, String> {
        self.spawn_agent_sync_with_ready(params, None)
    }

    /// Variant of `spawn_agent_sync` with a readiness oneshot for echo+submit verification.
    pub fn spawn_agent_sync_with_ready(
        &self,
        params: SpawnAgentParams,
        ready_tx: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
    ) -> Result<SpawnResult, String> {
        self.spawn_agent_sync_with_runtime_fork(params, ready_tx, None)
    }

    pub(crate) fn spawn_agent_sync_for_runtime_fork(
        &self,
        params: SpawnAgentParams,
        source_thread_id: String,
        turn_id: String,
    ) -> Result<SpawnResult, String> {
        self.spawn_agent_sync_with_runtime_fork(
            params,
            None,
            Some(crate::service::spawn::RuntimeForkLaunch {
                source_thread_id,
                turn_id,
            }),
        )
    }

    fn spawn_agent_sync_with_runtime_fork(
        &self,
        mut params: SpawnAgentParams,
        ready_tx: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
        runtime_fork: Option<crate::service::spawn::RuntimeForkLaunch>,
    ) -> Result<SpawnResult, String> {
        self.neighbors()
            .inherit_neighbor_agent_spawn_sync(&mut params)?;
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
        if params.role_profile.as_deref() == Some(builtins::PLANNER)
            && params.parent_tab_id.is_none()
        {
            let plan_id = params
                .plan_id
                .as_deref()
                .ok_or("Planner requires --plan <id>")?;
            params.parent_tab_id = Some(self.resolve_planner_parent_for_spawn(plan_id)?);
        }
        let admission = self.admit_spawn_role(&mut params)?;
        let (mut params, invocation, _account_reservation) =
            crate::service::provider_accounts::spawn_params(self, params)?;
        codeterm_plugin::provider_account::with_context(invocation, || {
            let role = params.role.clone();
            let role_profile = params.role_profile.clone();
            let plan_id = params.plan_id.clone();
            let agent = params
                .agent
                .clone()
                .unwrap_or_else(|| self.configuration().general_agent.agent_provider);
            if runtime_fork.is_some()
                && !crate::service::agent_runtime::engine::requested(self, &agent)
            {
                return Err("Runtime fork requires an agent runtime capability".into());
            }
            self.move_chatbackend_task_to_system_prompt(&agent, &mut params)?;
            self.apply_model_preset(&agent, &mut params)?;
            self.resolve_spawn_profile_options(&agent, &mut params)?;
            params.model = crate::service::spawn::resolve_agent_model_selection(
                &self.agent_registry,
                Some(&agent),
                params.model.as_deref(),
                &agent,
            )
            .map_err(|error| error.to_string())?
            .model;
            crate::service::spawn_speed::validate_agent_speed(
                &self.agent_registry,
                &agent,
                params.speed.as_deref(),
            )?;
            let reasoning_effort = params.reasoning_effort.clone();
            self.classify_spawn_target_for_params(&agent, &params)?;
            let bind_params = params.clone();
            let mut spec = crate::service::spawn::SpawnTabSpec::from_agent_params_with_chatbackends(
                self.chatbackend_registry.as_ref(),
                params,
            );
            if let Some(runtime_fork) = runtime_fork {
                let metadata = spec
                    .chatbackend_launch
                    .as_mut()
                    .ok_or("Runtime fork requires a chat backend spawn")?;
                metadata.runtime_fork = Some(runtime_fork);
            }
            let handle = self.spawn_tab_with_ready(spec, ready_tx)?;
            if let Some(tab) = self.mux.write().get_tab_mut(&handle.tab_id) {
                crate::service::reasoning_effort::persist_completed_launch_identity(
                    tab,
                    bind_params.planned_reasoning_effort.as_deref(),
                    bind_params.reasoning_effort.as_deref(),
                );
            }
            self.persist_unresolved_plugin_model_request(&handle.tab_id, &bind_params);
            let model_identity = self.tab_model_identity(&handle.tab_id);
            self.bind_worker_plan_task_or_rollback_tab(&bind_params, &handle.tab_id)?;
            self.bind_planner_spawn(plan_id.as_deref(), role_profile.as_deref(), &handle.tab_id);
            if let Some(reservation) = admission.worker_persona {
                reservation.commit_to_tab(&handle.tab_id);
            }
            let title = self.mux.read().display_label_for_tab(&handle.tab_id);
            self.record_kpi_spawn(
                &handle.tab_id,
                model_identity.2.as_deref(),
                bind_params.task.as_deref().unwrap_or_default().len(),
            );
            Ok(SpawnResult {
                team_id: self
                    .mux
                    .read()
                    .get_tab(&handle.tab_id)
                    .and_then(|tab| tab.team_id.clone()),
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
        // Membership door 7: role placement mints the parent's Team onto the
        // parent, so parenting anything under the General Agent would enrol it.
        refuse_parenting_under_general_agent(&self.mux.read(), parent_tab_id)?;
        // A Planner names its plan, not a parent: the plan record owns the
        // anchor, and placement + planner binding commit as one transaction.
        if profile.as_deref() == Some(builtins::PLANNER) && parent_tab_id.is_none() {
            let plan_id = plan_id
                .map(str::trim)
                .filter(|plan_id| !plan_id.is_empty())
                .ok_or("Planner requires --plan <id> (or an explicit --parent)")?;
            self.place_planner_on_plan(tab_id, plan_id, None, false)?;
            crate::service::team_roles::stamp_role_label(self, tab_id);
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
        crate::service::team_roles::stamp_role_label(self, tab_id);
        if let Some(parent) = parent_tab_id {
            self.orchestration()
                .expect_spawn_parent_link(tab_id, parent)?;
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
        let planner = match self.plan_store.hierarchy_planner_projection(plan_id) {
            Ok(planner) => planner,
            Err(crate::service::plan::PlanError::NotFound) => None,
            Err(error) => {
                log::warn!("[spawn] planner binding lookup failed for plan {plan_id}: {error}");
                return;
            }
        };
        let already_bound = matches!(
            planner,
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
            Ok(()) => self.emit_committed_plan_changed(&plan_id),
            Err(e) => log::warn!("[spawn] failed to bind planner ref for plan {plan_id}: {e}"),
        }
    }

    /// Origin frame for a spawned agent's first task, derived from its recorded spawner.
    pub(crate) fn spawn_task_message(&self, target_tab: &str, body: &str) -> Option<String> {
        if let Some(id) =
            crate::flow_trace::diagnostic_flow(crate::flow_trace::tab_flow(self, target_tab))
        {
            crate::flow_trace::record(&id, "spawn.task", target_tab, "", "prepared", None);
        }
        let friend = self.neighbors().neighbor_spawn_friend_name(target_tab);
        let (text, pending) = self
            .ext::<std::sync::Arc<crate::service::reply_relay::ReplyRelayRegistry>>()
            .spawn_task_message(
                &self.mux.read(),
                &self.agent_registry,
                target_tab,
                body,
                friend.as_deref(),
            )?;
        if let Some(pending) = pending {
            self.trace_spawn_reply_relay(target_tab, &pending);
        }
        Some(text)
    }
}

/// A chat backend has no interactive turn to paste a task into, so the launch
/// brief has to travel in its session config instead. Keyed off the same
/// spawnable predicate the routing seam uses, so the two cannot disagree.
pub fn move_chatbackend_task_to_system_prompt(
    chatbackends: &crate::chatbackend::ChatBackendRegistry,
    agent: &str,
    params: &mut SpawnAgentParams,
) {
    if chatbackends
        .descriptor(agent)
        .is_some_and(|descriptor| descriptor.agent_runtime.is_some())
    {
        params.system_prompt = compose_runtime_system_prompt(params);
        return;
    }
    let spawnable = chatbackends
        .get(agent)
        .map(|backend| backend.spawnable())
        .unwrap_or(false);
    if !spawnable {
        return;
    }
    params.system_prompt = compose_chatbackend_system_prompt(params);
}

impl ServiceContext {
    fn move_chatbackend_task_to_system_prompt(
        &self,
        agent: &str,
        params: &mut SpawnAgentParams,
    ) -> Result<(), String> {
        if params
            .execution_mode
            .as_deref()
            .is_some_and(|mode| mode != "local")
        {
            return Ok(());
        }
        if crate::service::agent_runtime::engine::requested(self, agent) {
            runtime_prompt::prepare_runtime_identity(self, params)?;
            params.system_prompt = compose_runtime_system_prompt(params);
            return Ok(());
        }
        if params.role_kind() == Some(builtins::PLANNER)
            && self
                .chatbackend_registry
                .get(agent)
                .is_some_and(|backend| backend.spawnable())
        {
            runtime_prompt::prepare_runtime_identity(self, params)?;
        }
        move_chatbackend_task_to_system_prompt(self.chatbackend_registry.as_ref(), agent, params);
        Ok(())
    }
}

/// Every params-owned field on a fresh ordinary-PTY agent tab is applied here;
/// a second init site is how a declared field (`audience`) once got dropped.
pub(crate) fn initialize_ordinary_pty_agent_tab(
    tab: &mut crate::multiplexer::types::Tab,
    params: &SpawnAgentParams,
) {
    crate::service::spawn::stamp_launch_authority(tab, params);
    tab.role_profile = params.role_profile.clone();
    tab.audience = params.audience;
    if params.description.is_some() {
        tab.description = params.description.clone();
    }
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

mod remote_binding;
use remote_binding::remote_task_binding_matches;
pub use remote_binding::remote_task_binding_matches_authority;

#[cfg(test)]
pub mod tests;

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
