//! Worker spawn binds a named plan task (`--plan-task`) to the new tab.

use crate::service::plan::PlanStore;
use crate::service::{ServiceContext, SpawnAgentParams};

pub(crate) fn worker_spawn_plan_task_ref(params: &SpawnAgentParams) -> Option<&str> {
    params
        .plan_task
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

pub(crate) fn validate_worker_plan_task_spawn(params: &SpawnAgentParams) -> Result<(), String> {
    if worker_spawn_plan_task_ref(params).is_some() && params.role.as_deref() != Some("Worker") {
        return Err("--plan-task is only valid with --role Worker".into());
    }
    Ok(())
}

/// Reject a WIP-gate-blocked dispatch before any tab is provisioned, so
/// there is nothing left to roll back.
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
    if task.status != crate::plan::TaskStatus::Pending {
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
        return Ok(());
    };
    let resolved = store
        .resolve_task_ref(params.plan_id.as_deref(), task_ref)
        .map_err(|error| error.to_string())?;
    let detail = store
        .show_task(&resolved.task_id)
        .ok_or_else(|| "task not found".to_string())?;
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
    attempt_id: &str,
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
    let attempt_id = store
        .begin_task_dispatch_with_reserved_attempt(
            &plan_id,
            &task_id,
            tab_id,
            title,
            lane.clone(),
            intent,
            allowance,
            provider,
            attempt_id,
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
        let session_binding_nonce = session_binding_nonce
            .filter(|nonce| !nonce.trim().is_empty())
            .ok_or_else(|| "plan-task spawn is missing its session binding nonce".to_string())?;
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

fn bind_worker_plan_task_transaction(
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
    attempt_id: &str,
    manifest: Vec<crate::plan::PlanSkillManifestEntry>,
    now_ms: u64,
    close_tab: impl FnOnce(),
) -> Result<WorkerTaskBinding, String> {
    match bind_worker_task_to_tab(
        store,
        plan_id,
        task_ref,
        tab_id,
        title,
        session_id,
        session_binding_nonce,
        intent,
        allowance,
        provider,
        attempt_id,
        manifest,
        now_ms,
    ) {
        Ok(binding) => Ok(binding),
        Err(e) => {
            close_tab();
            Err(e)
        }
    }
}

impl ServiceContext {
    /// Preflight the review WIP gate before any tab gets provisioned.
    pub(crate) fn preflight_worker_plan_task_spawn(
        &self,
        params: &SpawnAgentParams,
    ) -> Result<(), String> {
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
        prepare_worker_plan_task_prompt(&self.plan_store, params)
    }

    pub(crate) fn bind_worker_plan_task_or_rollback_tab(
        &self,
        params: &SpawnAgentParams,
        tab_id: &str,
    ) -> Result<(), String> {
        let Some(task_ref) = worker_spawn_plan_task_ref(params) else {
            return Ok(());
        };
        let now_ms = epoch_ms();
        let (title, session_id, session_binding_nonce) = {
            let mux = self.mux.read();
            let title = mux.display_label_for_tab(tab_id);
            let binding = mux.get_tab(tab_id).map(|tab| {
                (
                    tab.agent_session_id.clone(),
                    tab.session_binding_nonce.clone(),
                )
            });
            let (session_id, session_binding_nonce) = binding.unwrap_or_default();
            (title, session_id, session_binding_nonce)
        };
        let binding = bind_worker_plan_task_transaction(
            &self.plan_store,
            params.plan_id.as_deref(),
            task_ref,
            tab_id,
            &title,
            session_id.as_deref(),
            session_binding_nonce.as_deref(),
            params
                .execution_intent
                .clone()
                .ok_or_else(|| "plan-task spawn requires explicit execution_intent".to_string())?,
            self.provider_allowance_for_spawn(params, now_ms)?,
            params.agent.as_deref(),
            &params
                .task_launch_authority
                .as_ref()
                .ok_or_else(|| "plan-task spawn is missing launch authority".to_string())?
                .attempt_id,
            params.plan_skill_manifest.clone().ok_or_else(|| {
                "plan-task spawn is missing its prepared plan skill manifest".to_string()
            })?,
            now_ms,
            || {
                let _ = self.close_tab(tab_id);
            },
        )?;
        crate::api::events::emit_everywhere(
            &self.app_handle,
            "plan-changed",
            serde_json::json!({
                "plan_id": binding.plan_id,
                "task_id": binding.task_id,
                "attempt_id": binding.attempt_id,
            }),
        );
        Ok(())
    }

    fn provider_allowance_for_spawn(
        &self,
        params: &SpawnAgentParams,
        now_ms: u64,
    ) -> Result<Option<crate::plan::ProviderAllowance>, String> {
        if params.execution_intent != Some(crate::plan::ExecutionIntent::Mutating) {
            return Ok(None);
        }
        let provider = params
            .agent
            .clone()
            .unwrap_or_else(crate::config::default_agent_provider);
        let capability = self
            .agent_registry
            .with_provider(&provider, |provider| {
                provider.provider_allowance_capability()
            })
            .flatten()
            .ok_or_else(|| "selected provider has no mutation allowance capability".to_string())?;
        let snapshot = self
            .usage_store
            .get_latest(&provider, None)?
            .ok_or_else(|| "provider allowance telemetry is unknown".to_string())?;
        let allowance = crate::plan::ProviderAllowance {
            remaining_pct: crate::usage::allowance_remaining_pct(&snapshot),
            observed_at: snapshot.captured_at.max(0) as u64,
            stale_after_ms: capability.telemetry_max_age_ms,
            reserve_pct: params
                .provider_reserve_pct
                .unwrap_or(capability.default_reserve_pct),
        };
        crate::service::plan::interruption::provider_allowance_preflight(
            &crate::plan::ExecutionIntent::Mutating,
            Some(&allowance),
            now_ms,
        )
        .map_err(|error| error.to_string())?;
        Ok(Some(allowance))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{OrchestratorRef, PlanMode, TaskStatus};
    use crate::service::plan::{NewTask, PlanStore};

    fn store_with_task(dir: &std::path::Path) -> (PlanStore, String, String) {
        let store = PlanStore::new_for_test(dir);
        let plan = store
            .create_plan_for_team(
                "Spawn bind".into(),
                dir.display().to_string(),
                OrchestratorRef::from_tab("orch"),
                "team-orch".into(),
                vec![],
                1,
            )
            .unwrap();
        let mut p = store.get(&plan.id).unwrap();
        p.mode = PlanMode::Approved;
        store.save(&mut p, 2).unwrap();
        let task_id = store
            .add_task(
                &plan.id,
                NewTask {
                    name: "bind-at-spawn".into(),
                    handle: Some("spawn-bind".into()),
                    personality: Some("Ada".into()),
                    role_guidance: Some("You are Ada. Keep the change bounded.".into()),
                    body: "Implement the task-owned behavior.".into(),
                    acceptance_criteria: vec!["The focused oracle passes.".into()],
                    ..Default::default()
                },
                3,
            )
            .unwrap();
        (store, plan.id, task_id)
    }

    #[test]
    fn bind_sets_executor_and_waits_for_ack() {
        let dir = tempfile::tempdir().unwrap();
        let (store, plan_id, task_id) = store_with_task(dir.path());
        let binding = bind_worker_task_to_tab(
            &store,
            Some(&plan_id),
            "spawn-bind",
            "tab-worker",
            "Ada",
            Some("session-worker"),
            Some("nonce-worker"),
            crate::plan::ExecutionIntent::ReadOnly,
            None,
            None,
            "attempt-prepared",
            vec![crate::plan::PlanSkillManifestEntry {
                slug: "prepared".into(),
                title: "Prepared".into(),
                purpose: "Prove exact snapshot retention.".into(),
                content_digest: "sha256:prepared".into(),
            }],
            10,
        )
        .unwrap();
        let plan = store.get(&plan_id).unwrap();
        let task = plan.tasks.iter().find(|t| t.id == task_id).unwrap();
        assert_eq!(task.status, TaskStatus::Pending);
        assert_eq!(task.status_label.as_deref(), Some("awaiting_worker_ack"));
        assert_eq!(task.assigned_tab_id.as_deref(), Some("tab-worker"));
        assert_eq!(task.lane.tab_id.as_deref(), Some("tab-worker"));
        assert_eq!(task.taken_by_role.as_deref(), Some("worker"));
        assert_eq!(binding.plan_id, plan_id);
        assert_eq!(binding.task_id, task_id);
        assert_eq!(
            task.execution_attempts.last().unwrap().plan_skill_manifest[0].slug,
            "prepared"
        );
        assert_eq!(
            task.execution_attempts
                .last()
                .unwrap()
                .session_id
                .as_deref(),
            Some("session-worker")
        );
    }

    #[test]
    fn direct_plan_task_spawn_uses_task_owned_prompt_before_commands() {
        let dir = tempfile::tempdir().unwrap();
        let (store, plan_id, _) = store_with_task(dir.path());
        let mut params = SpawnAgentParams {
            role: Some("Worker".into()),
            plan_id: Some(plan_id.clone()),
            plan_task: Some("spawn-bind".into()),
            execution_intent: Some(crate::plan::ExecutionIntent::ReadOnly),
            provider_reserve_pct: None,
            task: Some("Check the adjacent caller concern.".into()),
            ..Default::default()
        };

        prepare_worker_plan_task_prompt(&store, &mut params).unwrap();

        let authority = params.task_launch_env();
        assert_eq!(
            authority.get("CODETERM_PLAN_ID").map(String::as_str),
            Some(plan_id.as_str())
        );
        assert_eq!(
            authority.get("CODETERM_TASK_ID").map(String::as_str),
            params.task_id.as_deref()
        );
        let prompt = params.task.unwrap();
        assert!(prompt.starts_with("You are Ada. Keep the change bounded."));
        let body = prompt.find("Implement the task-owned behavior.").unwrap();
        let supplemental = prompt.find("## Orchestrator instruction").unwrap();
        let ack = prompt.find("codeterm agent ack").unwrap();
        assert!(body < supplemental);
        assert!(supplemental < ack);
    }

    #[test]
    fn direct_plan_task_spawn_records_host_owned_effort_intent() {
        let dir = tempfile::tempdir().unwrap();
        let (store, plan_id, _) = store_with_task(dir.path());
        let mut params = SpawnAgentParams {
            role: Some("Worker".into()),
            plan_id: Some(plan_id),
            plan_task: Some("spawn-bind".into()),
            reasoning_effort: Some("high".into()),
            ..Default::default()
        };

        prepare_worker_plan_task_prompt(&store, &mut params).unwrap();

        assert_eq!(params.planned_reasoning_effort.as_deref(), Some("high"));
        assert_eq!(params.reasoning_effort.as_deref(), Some("high"));
    }

    #[test]
    fn validate_rejects_non_worker() {
        let err = validate_worker_plan_task_spawn(&SpawnAgentParams {
            role: Some("Orchestrator".into()),
            plan_task: Some("spawn-bind".into()),
            execution_intent: Some(crate::plan::ExecutionIntent::ReadOnly),
            provider_reserve_pct: None,
            ..Default::default()
        })
        .unwrap_err();
        assert!(err.contains("--plan-task"));
    }

    fn block_review_wip(store: &PlanStore, plan_id: &str) {
        let mut p = store.get(plan_id).unwrap();
        p.review_policy.wip_limit = 0;
        store.save(&mut p, 4).unwrap();
    }

    #[test]
    fn preflight_rejects_before_any_tab_when_wip_gate_blocked() {
        let dir = tempfile::tempdir().unwrap();
        let (store, plan_id, _task_id) = store_with_task(dir.path());
        block_review_wip(&store, &plan_id);

        let err =
            preflight_worker_plan_task_wip_gate(&store, Some(&plan_id), "spawn-bind").unwrap_err();
        assert!(err.contains("review"), "got: {err}");
    }

    #[test]
    fn preflight_allows_dispatch_when_gate_is_clear() {
        let dir = tempfile::tempdir().unwrap();
        let (store, plan_id, _task_id) = store_with_task(dir.path());
        preflight_worker_plan_task_wip_gate(&store, Some(&plan_id), "spawn-bind").unwrap();
    }

    #[test]
    fn spawn_preflight_requires_daemon_owned_capacity_provenance() {
        let dir = tempfile::tempdir().unwrap();
        let (store, plan_id, task_id) = store_with_task(dir.path());
        let mut plan = store.get(&plan_id).unwrap();
        plan.review_policy.credit = -(plan.review_policy.wip_limit as i64);
        let task = plan
            .tasks
            .iter_mut()
            .find(|task| task.id == task_id)
            .unwrap();
        task.kind = "review".into();
        task.tags.push("review-capacity".into());
        store.save(&mut plan, 4).unwrap();
        let error =
            preflight_worker_plan_task_wip_gate(&store, Some(&plan_id), "spawn-bind").unwrap_err();
        assert!(error.contains("review"), "{error}");

        store
            .grant_review_capacity(
                &plan_id,
                &task_id,
                crate::plan::ReviewCapacityClass::Review,
                crate::plan::AgentRef::from_tab("orch"),
                5,
            )
            .unwrap();
        preflight_worker_plan_task_wip_gate(&store, Some(&plan_id), "spawn-bind").unwrap();
    }

    // Reproduces the incident: `assign_task_with_role` (step 1) already binds
    // the tab before `update_task(in_progress)` (step 2) hits the WIP gate.
    #[test]
    fn wip_gate_failure_after_tab_bind_leaves_zero_residual_task_lane_binding() {
        let dir = tempfile::tempdir().unwrap();
        let (store, plan_id, task_id) = store_with_task(dir.path());
        block_review_wip(&store, &plan_id);

        let err = bind_worker_task_to_tab(
            &store,
            Some(&plan_id),
            "spawn-bind",
            "tab-worker",
            "Ada",
            None,
            None,
            crate::plan::ExecutionIntent::ReadOnly,
            None,
            None,
            "attempt-review",
            vec![],
            10,
        )
        .unwrap_err();
        assert!(err.contains("review"), "got: {err}");

        let plan = store.get(&plan_id).unwrap();
        let task = plan.tasks.iter().find(|t| t.id == task_id).unwrap();
        assert_eq!(task.status, TaskStatus::Pending);
        assert_eq!(task.assigned_tab_id, None);
        assert_eq!(task.lane.tab_id, None);
    }

    #[test]
    fn ack_after_direct_spawn_enters_progress() {
        let dir = tempfile::tempdir().unwrap();
        let (store, plan_id, task_id) = store_with_task(dir.path());
        let binding = bind_worker_task_to_tab(
            &store,
            Some(&plan_id),
            "spawn-bind",
            "tab-worker",
            "Ada",
            Some("session-worker"),
            Some("nonce-worker"),
            crate::plan::ExecutionIntent::ReadOnly,
            None,
            None,
            "attempt-ack",
            vec![],
            10,
        )
        .unwrap();
        store
            .acknowledge_task_attempt(&plan_id, &task_id, "tab-worker", &binding.attempt_id, 11)
            .unwrap();

        let plan = store.get(&plan_id).unwrap();
        let task = plan.tasks.iter().find(|t| t.id == task_id).unwrap();
        assert_eq!(task.status, TaskStatus::InProgress);
        assert_eq!(task.status_label, None);
    }

    // Proven via the WIP-gate path — the failure reliably triggerable here
    // without dependency-injecting the identity step itself.
    #[test]
    fn transaction_closes_tab_and_reverts_state_on_any_post_provision_failure() {
        let dir = tempfile::tempdir().unwrap();
        let (store, plan_id, task_id) = store_with_task(dir.path());
        block_review_wip(&store, &plan_id);

        let mut tab_closed = false;
        let err = bind_worker_plan_task_transaction(
            &store,
            Some(&plan_id),
            "spawn-bind",
            "tab-worker",
            "Ada",
            None,
            None,
            crate::plan::ExecutionIntent::ReadOnly,
            None,
            None,
            "attempt-rollback",
            vec![],
            10,
            || tab_closed = true,
        )
        .unwrap_err();
        assert!(err.contains("review"), "got: {err}");
        assert!(
            tab_closed,
            "the just-created Tab must be closed on any post-provision failure"
        );

        let plan = store.get(&plan_id).unwrap();
        let task = plan.tasks.iter().find(|t| t.id == task_id).unwrap();
        assert_eq!(task.status, TaskStatus::Pending);
        assert_eq!(task.assigned_tab_id, None);
        assert_eq!(task.lane.tab_id, None);
    }
}
