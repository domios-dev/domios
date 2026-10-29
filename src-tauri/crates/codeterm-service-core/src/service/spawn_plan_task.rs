//! Worker spawn binds a named plan task (`--plan-task`) to the new tab. A
//! read-only Worker binds none: it verifies, owns no task, and carries only the
//! gate tokens it was spawned with.

use crate::plan::Task;
use crate::service::plan::PlanStore;
use crate::service::{ServiceContext, SpawnAgentParams};
mod runtime_binding;

fn resolved_worker_launch_effort(
    identity: &crate::multiplexer::types::ReasoningEffortIdentity,
) -> Option<String> {
    identity.resolved.clone()
}

pub fn planned_task_title(task: &Task) -> Option<String> {
    crate::service::plan::worker_identity::task_executor_name(task)
        .ok()
        .flatten()
}

pub(crate) fn worker_spawn_plan_task_ref(params: &SpawnAgentParams) -> Option<&str> {
    params
        .plan_task
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn is_worker_spawn(params: &SpawnAgentParams) -> bool {
    params.role.as_deref() == Some("Worker")
}

/// A Worker holds a task when the spawn names one, by either flag that can.
fn worker_spawn_is_task_bound(params: &SpawnAgentParams) -> bool {
    params.task_launch_authority.is_some()
        || worker_spawn_plan_task_ref(params).is_some()
        || params
            .task_id
            .as_deref()
            .map(str::trim)
            .is_some_and(|task_id| !task_id.is_empty())
}

fn worker_spawn_has_kind(params: &SpawnAgentParams) -> bool {
    is_worker_spawn(params)
        && params
            .role_profile
            .as_deref()
            .is_some_and(|kind| !kind.trim().is_empty())
}

/// Task binding governs lifecycle authority; Worker kinds add role guidance.
pub(crate) fn validate_worker_plan_task_spawn(params: &SpawnAgentParams) -> Result<(), String> {
    if worker_spawn_plan_task_ref(params).is_some() && !is_worker_spawn(params) {
        return Err("--plan-task is only valid with --role Worker".into());
    }
    if !params.authorized_gates.is_empty() {
        if !is_worker_spawn(params) {
            return Err("authorized gates are only valid with --role Worker".into());
        }
        if worker_spawn_is_task_bound(params) && params.task_launch_authority.is_none() {
            return Err(concat!(
                "authorized gates are not carried by a task-bound spawn; declare them ",
                "on the dispatch, where they are also enforced"
            )
            .into());
        }
        if params.task_launch_authority.is_none()
            && !worker_spawn_has_kind(params)
            && params.execution_intent != Some(crate::plan::ExecutionIntent::ReadOnly)
        {
            return Err("authorized gates require read-only execution intent".into());
        }
    }
    if worker_spawn_has_kind(params)
        && params
            .parent_tab_id
            .as_deref()
            .is_none_or(|parent| parent.trim().is_empty())
    {
        return Err("Worker kinds require a parent tab".into());
    }
    if is_worker_spawn(params) && worker_spawn_is_task_bound(params) {
        if !matches!(
            params.execution_intent,
            Some(crate::plan::ExecutionIntent::ReadOnly | crate::plan::ExecutionIntent::Mutating)
        ) {
            return Err(
                "A task-bound Worker requires --execution-intent read_only or mutating".into(),
            );
        }
        if let Some(title) = params.title.as_deref() {
            crate::service::plan::worker_identity::validate_executor_name(title)?;
        }
    }
    if is_worker_spawn(params)
        && !worker_spawn_is_task_bound(params)
        && !worker_spawn_has_kind(params)
        && params.execution_intent == Some(crate::plan::ExecutionIntent::Mutating)
    {
        return Err(concat!(
            "a mutating Worker must own a task: pass --plan-task <task> (or --plan <id> ",
            "--task-id <task>). A Worker that only verifies declares ",
            "--execution-intent read_only and owns none."
        )
        .into());
    }
    Ok(())
}

/// The declared gates are the whole scope a task-less verification Worker has,
/// so they ride into its brief through the writer the dispatch path already
/// uses rather than a second renderer that could drift from it.
fn append_authorized_gates(params: &mut SpawnAgentParams) {
    if params.authorized_gates.is_empty() || params.task_launch_authority.is_some() {
        return;
    }
    let gates = crate::service::plan::task_brief::render_authorized_gates(&params.authorized_gates);
    let existing = params.task.take().unwrap_or_default();
    let existing = existing.trim_end();
    params.task = Some(if existing.is_empty() {
        gates
    } else {
        format!("{existing}\n\n{gates}")
    });
}

/// Resolve task identity and surface advisory review pressure before provisioning.
pub(crate) fn preflight_worker_plan_task_wip_gate(
    store: &PlanStore,
    plan_id: Option<&str>,
    task_ref: &str,
) -> Result<(), String> {
    let resolved = store
        .resolve_task_ref(plan_id, task_ref)
        .map_err(|e| e.to_string())?;
    let plan = store
        .get(&resolved.plan_id)
        .ok_or_else(|| "plan not found".to_string())?;
    let task = plan
        .tasks
        .iter()
        .find(|t| t.id == resolved.task_id)
        .ok_or_else(|| "task not found".to_string())?;
    store
        .validate_task_startup_status(&resolved.plan_id, &resolved.task_id)
        .map_err(|error| error.to_string())?;
    if !matches!(
        &task.status,
        crate::plan::TaskStatus::Pending | crate::plan::TaskStatus::Blocked
    ) {
        return Ok(());
    }
    crate::service::plan::review_gate::validate_dispatch_review_capacity(&plan, task)
        .map_err(|error| error.to_string())
}

pub(crate) fn prepare_worker_plan_task_prompt(
    store: &PlanStore,
    params: &mut SpawnAgentParams,
) -> Result<(), String> {
    let Some(task_ref) = worker_spawn_plan_task_ref(params) else {
        append_authorized_gates(params);
        return Ok(());
    };
    let resolved = store
        .resolve_task_ref(params.plan_id.as_deref(), task_ref)
        .map_err(|error| error.to_string())?;
    let detail = store
        .show_task(&resolved.task_id)
        .ok_or_else(|| "task not found".to_string())?;
    store
        .validate_task_startup_status(&resolved.plan_id, &resolved.task_id)
        .map_err(|error| error.to_string())?;
    if let Some(title) = planned_task_title(&detail.task) {
        params.title = Some(title);
        params.title_provenance = Some(crate::service::SpawnIdentityProvenance::Suggested);
    }
    params.speed = params.speed.take().or_else(|| detail.task.speed.clone());
    if params.planned_reasoning_effort.is_none() {
        params.planned_reasoning_effort = params.reasoning_effort.clone();
    }
    params.task_launch_authority = Some(crate::service::TaskLaunchAuthority {
        plan_id: resolved.plan_id.clone(),
        task_id: resolved.task_id.clone(),
        attempt_id: crate::plan::ids::short_id("attempt"),
        lane_base: detail.task.lane.base.clone(),
    });
    let instruction = params.task.take();
    let manifest = crate::service::plan::plan_skills::plan_skill_manifest(store, &resolved.plan_id)
        .map_err(|error| error.to_string())?;
    let brief = crate::service::plan::task_brief::render_task_brief(
        &detail.task,
        &detail.sections,
        detail.task.personality.as_deref(),
        instruction.as_deref(),
        Some(&resolved.plan_id),
        &manifest,
        &[],
    );
    params.plan_skill_manifest = Some(manifest);
    params.task = Some(brief);
    params.plan_id = Some(resolved.plan_id);
    params.task_id = Some(resolved.task_id);
    Ok(())
}

#[derive(Debug)]
struct WorkerTaskBinding {
    plan_id: String,
    task_id: String,
    attempt_id: String,
}

fn bind_worker_task_to_tab(
    store: &PlanStore,
    plan_id: Option<&str>,
    task_ref: &str,
    tab_id: &str,
    title: &str,
    session_id: Option<&str>,
    session_binding_nonce: Option<&str>,
    intent: crate::plan::ExecutionIntent,
    allowance: Option<crate::plan::ProviderAllowance>,
    provider: Option<&str>,
    requested_model: Option<String>,
    model: Option<String>,
    reasoning_effort: Option<String>,
    attempt_id: &str,
    authorized_gates: Option<Vec<String>>,
    manifest: Vec<crate::plan::PlanSkillManifestEntry>,
    now_ms: u64,
) -> Result<WorkerTaskBinding, String> {
    let resolved = store
        .resolve_task_ref(plan_id, task_ref)
        .map_err(|e| e.to_string())?;
    let plan_id = resolved.plan_id;
    let task_id = resolved.task_id;
    let lane = store
        .show_task(&task_id)
        .ok_or_else(|| "task not found".to_string())?
        .task
        .lane;
    let task = store.show_task(&task_id).ok_or("task not found")?.task;
    if let Some(existing) = task
        .execution_attempts
        .iter()
        .find(|attempt| attempt.id == attempt_id)
    {
        if existing.executor_tab_id != tab_id
            || existing.closed_at.is_some()
            || existing.retired_at.is_some()
        {
            return Err("Prepared runtime task attempt does not own this tab".into());
        }
        if let Some(session) = session_id {
            let nonce = session_binding_nonce
                .filter(|nonce| !nonce.trim().is_empty())
                .ok_or("Task session nonce missing")?;
            if existing
                .session_id
                .as_deref()
                .is_some_and(|bound| bound != session)
                || existing
                    .session_binding_nonce
                    .as_deref()
                    .is_some_and(|bound| bound != nonce)
            {
                return Err("Prepared runtime task attempt rebound".into());
            }
            if existing.session_id.is_none() {
                store
                    .finish_task_dispatch(
                        &plan_id,
                        &task_id,
                        tab_id,
                        attempt_id,
                        session,
                        nonce,
                        lane.clone(),
                        now_ms,
                    )
                    .map_err(|reason| reason.to_string())?;
            }
        }
        return Ok(WorkerTaskBinding {
            plan_id,
            task_id,
            attempt_id: attempt_id.into(),
        });
    }
    let attempt_id = store
        .begin_task_dispatch_with_reserved_attempt_and_gates_and_observation(
            &plan_id,
            &task_id,
            tab_id,
            title,
            lane.clone(),
            intent,
            allowance,
            provider,
            attempt_id,
            authorized_gates,
            crate::plan::WorkerPlacement::IsolatedLane,
            model.clone(),
            requested_model,
            model,
            reasoning_effort,
            None,
            false,
            false,
            now_ms,
        )
        .map_err(|error| error.to_string())?;
    if let Err(error) =
        store.set_attempt_plan_skill_manifest(&plan_id, &task_id, &attempt_id, manifest, now_ms)
    {
        let _ = store.revert_task_dispatch(&plan_id, &task_id, now_ms);
        return Err(error.to_string());
    }
    if let Some(session_id) = session_id {
        let Some(session_binding_nonce) =
            session_binding_nonce.filter(|nonce| !nonce.trim().is_empty())
        else {
            let _ = store.revert_task_dispatch(&plan_id, &task_id, now_ms);
            return Err("plan-task spawn is missing its session binding nonce".into());
        };
        if let Err(error) = store.finish_task_dispatch(
            &plan_id,
            &task_id,
            tab_id,
            &attempt_id,
            session_id,
            session_binding_nonce,
            lane,
            now_ms,
        ) {
            let _ = store.revert_task_dispatch(&plan_id, &task_id, now_ms);
            return Err(error.to_string());
        }
    }
    Ok(WorkerTaskBinding {
        plan_id,
        task_id,
        attempt_id,
    })
}

fn epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl ServiceContext {
    pub fn persist_dispatched_attempt_reasoning_effort(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        attempt_id: &str,
        now_ms: u64,
    ) -> Result<(), crate::service::plan::PlanError> {
        let reasoning_effort = {
            let mux = self.mux.read();
            mux.get_tab(tab_id)
                .and_then(|tab| tab.reasoning_effort_identity.resolved.clone())
        };
        self.plan_store.update_dispatched_attempt_reasoning_effort(
            plan_id,
            task_id,
            tab_id,
            attempt_id,
            reasoning_effort,
            now_ms,
        )
    }

    pub fn resolved_worker_launch_effort_for_tab(&self, tab_id: &str) -> Option<String> {
        let mux = self.mux.read();
        mux.get_tab(tab_id)
            .and_then(|tab| resolved_worker_launch_effort(&tab.reasoning_effort_identity))
    }

    /// Preflight the review WIP gate before any tab gets provisioned.
    pub(crate) fn preflight_worker_plan_task_spawn(
        &self,
        params: &SpawnAgentParams,
    ) -> Result<(), String> {
        // Every task-bound mutating route reaches this before a tab exists, by
        // either flag that binds a task, so none can write canonical silently.
        self.refuse_unsafe_task_bound_mutating_spawn(params)?;
        let Some(task_ref) = worker_spawn_plan_task_ref(params) else {
            return Ok(());
        };
        preflight_worker_plan_task_wip_gate(&self.plan_store, params.plan_id.as_deref(), task_ref)?;
        let intent = params.execution_intent.as_ref().ok_or_else(|| {
            "plan-task spawn requires explicit execution_intent read_only or mutating".to_string()
        })?;
        let now_ms = epoch_ms();
        let allowance = self.provider_allowance_for_spawn(params, now_ms)?;
        crate::service::plan::interruption::provider_allowance_preflight(
            intent,
            allowance.as_ref(),
            now_ms,
        )
        .map_err(|error| error.to_string())
    }

    pub(crate) fn prepare_worker_plan_task_spawn(
        &self,
        params: &mut SpawnAgentParams,
    ) -> Result<(), String> {
        if worker_spawn_is_task_bound(params) {
            let (tool_less, relay) = self
                .agent_registry
                .with_provider(params.agent.as_deref().unwrap_or_default(), |provider| {
                    (provider.is_tool_less(), provider.supports_reply_relay())
                })
                .unwrap_or_default();
            super::reply_relay::worker::validate_worker_lifecycle_capabilities(tool_less, relay)?;
        }
        let instruction = params.task.clone();
        let owns_prompt = worker_spawn_plan_task_ref(params).is_some();
        prepare_worker_plan_task_prompt(&self.plan_store, params)?;
        if owns_prompt
            && self
                .agent_registry
                .with_provider(params.agent.as_deref().unwrap_or_default(), |provider| {
                    provider.is_tool_less()
                })
                .unwrap_or(false)
        {
            if let (Some(plan), Some(task_id)) =
                (params.plan_id.as_deref(), params.task_id.as_deref())
            {
                let detail = self.plan_store.show_task(task_id).ok_or("task not found")?;
                if detail.plan_id == plan {
                    params.task = Some(
                        crate::service::plan::task_brief::render_tool_less_task_brief(
                            &detail.task,
                            &detail.sections,
                            detail.task.personality.as_deref(),
                            instruction.as_deref(),
                        ),
                    );
                }
            }
        }
        Ok(())
    }

    pub(crate) fn bind_worker_plan_task_or_rollback_tab(
        &self,
        params: &SpawnAgentParams,
        tab_id: &str,
    ) -> Result<(), String> {
        let Some(task_ref) = worker_spawn_plan_task_ref(params) else {
            if let Some(binding) = params.remote_task_binding.as_ref() {
                let mut mux = self.mux.write();
                let tab = mux
                    .get_tab_mut(tab_id)
                    .ok_or_else(|| format!("remote task-bound Worker tab {tab_id} disappeared"))?;
                tab.task_binding = Some(crate::multiplexer::types::TaskBinding {
                    plan_id: Some(binding.plan_id.clone()),
                    task_id: Some(binding.task_id.clone()),
                    attempt_id: Some(binding.attempt_id.clone()),
                    spawn_handle: None,
                    bound_at: epoch_ms(),
                });
                tab.report_target_mesh_peer = Some(binding.origin_peer_id.clone());
            }
            return Ok(());
        };
        let now_ms = epoch_ms();
        let (
            title,
            session_id,
            session_binding_nonce,
            provider,
            requested_model,
            model,
            reasoning_effort,
        ) = {
            let mux = self.mux.read();
            let title = mux.display_label_for_tab(tab_id);
            let binding = mux.get_tab(tab_id).map(|tab| {
                (
                    tab.agent_session_id.clone(),
                    tab.session_binding_nonce.clone(),
                    tab.agent_type.clone(),
                    tab.agent_requested_model.clone(),
                    tab.agent_provider_resolved_model.clone(),
                    resolved_worker_launch_effort(&tab.reasoning_effort_identity),
                )
            });
            let (
                session_id,
                session_binding_nonce,
                provider,
                requested_model,
                model,
                reasoning_effort,
            ) = binding.unwrap_or_default();
            (
                title,
                session_id,
                session_binding_nonce,
                provider,
                requested_model,
                model,
                reasoning_effort,
            )
        };
        let binding = self.create_worker_task_binding(
            params,
            task_ref,
            tab_id,
            &title,
            session_id.as_deref(),
            session_binding_nonce.as_deref(),
            provider.as_deref(),
            requested_model,
            model,
            reasoning_effort,
            now_ms,
        );
        let binding = match binding {
            Ok(binding) => binding,
            Err(error) => {
                return Err(self.rollback_failed_agent_spawn_if_created(
                    tab_id,
                    &error,
                    params.existing_tab_id.is_none(),
                ))
            }
        };
        {
            let mut mux = self.mux.write();
            let target = mux
                .get_tab_mut(tab_id)
                .ok_or("Runtime task tab unavailable")?;
            target.task_binding = Some(crate::multiplexer::types::TaskBinding {
                plan_id: Some(binding.plan_id.clone()),
                task_id: Some(binding.task_id.clone()),
                attempt_id: Some(binding.attempt_id.clone()),
                spawn_handle: None,
                bound_at: now_ms,
            });
        }
        if let Some(speed) = params.speed.as_deref() {
            self.plan_store
                .update_task_speed(&binding.plan_id, &binding.task_id, Some(speed), now_ms)
                .map_err(|error| error.to_string())?;
        }
        self.persist_current_attempt_session_model(tab_id);
        self.ui.emit(
            "plan-changed",
            serde_json::to_value(serde_json::json!({
                "plan_id": binding.plan_id,
                "task_id": binding.task_id,
                "attempt_id": binding.attempt_id,
            }))
            .unwrap_or_default(),
        );
        Ok(())
    }

    fn create_worker_task_binding(
        &self,
        params: &SpawnAgentParams,
        task_ref: &str,
        tab_id: &str,
        title: &str,
        session_id: Option<&str>,
        session_binding_nonce: Option<&str>,
        provider: Option<&str>,
        requested_model: Option<String>,
        model: Option<String>,
        reasoning_effort: Option<String>,
        now_ms: u64,
    ) -> Result<WorkerTaskBinding, String> {
        let intent = params
            .execution_intent
            .clone()
            .ok_or_else(|| "plan-task spawn requires explicit execution_intent".to_string())?;
        let allowance = self.provider_allowance_for_spawn(params, now_ms)?;
        let authority = params
            .task_launch_authority
            .as_ref()
            .ok_or_else(|| "plan-task spawn is missing launch authority".to_string())?;
        let manifest = params.plan_skill_manifest.clone().ok_or_else(|| {
            "plan-task spawn is missing its prepared plan skill manifest".to_string()
        })?;
        let authorized_gates =
            (!params.authorized_gates.is_empty()).then(|| params.authorized_gates.clone());
        let existing_attempt = self
            .plan_store
            .show_task(&authority.task_id)
            .is_some_and(|detail| {
                detail
                    .task
                    .execution_attempts
                    .iter()
                    .any(|attempt| attempt.id == authority.attempt_id)
            });
        let checkout_path = if existing_attempt {
            None
        } else {
            let cwd = self.mux.read().get_tab(tab_id).and_then(|tab| tab.cwd.clone());
            cwd.and_then(|cwd| git2::Repository::discover(cwd).ok())
                .and_then(|repo| {
                    repo.workdir().map(|path| {
                        codeterm_paths::strip_verbatim_prefix(&path.to_string_lossy())
                    })
                })
        };
        let binding = bind_worker_task_to_tab(
            &self.plan_store,
            params.plan_id.as_deref(),
            task_ref,
            tab_id,
            title,
            session_id,
            session_binding_nonce,
            intent,
            allowance,
            provider,
            requested_model,
            model,
            reasoning_effort,
            &authority.attempt_id,
            authorized_gates,
            manifest,
            now_ms,
        )?;
        if let Some(checkout_path) = checkout_path {
            if let Err(error) = self.plan_store.record_attempt_checkout(
                &binding.plan_id,
                &binding.task_id,
                tab_id,
                &binding.attempt_id,
                checkout_path,
                now_ms,
            ) {
                let _ = self.plan_store.revert_task_dispatch(
                    &binding.plan_id,
                    &binding.task_id,
                    now_ms,
                );
                return Err(error.to_string());
            }
        }
        Ok(binding)
    }

    fn provider_allowance_for_spawn(
        &self,
        _params: &SpawnAgentParams,
        _now_ms: u64,
    ) -> Result<Option<crate::plan::ProviderAllowance>, String> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests;
