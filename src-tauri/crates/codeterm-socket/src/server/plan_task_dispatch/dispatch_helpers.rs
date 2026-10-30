use super::{JsonRpcError, ServiceState, SpawnAgentParams};
use serde_json::Value;

pub(in crate::server) use codeterm_service_core::service::task_tab_description;

pub(super) fn verify_dispatched_task_binding(
    store: &codeterm_service_core::service::plan::PlanStore,
    plan_id: &str,
    task_id: &str,
    tab_id: &str,
) -> Result<(), String> {
    match store.task_binding_for_tab(tab_id) {
        Some((bound_plan, bound_task)) if bound_plan == plan_id && bound_task == task_id => Ok(()),
        binding => Err(format!(
            "ORCHESTRATION PRODUCT INCIDENT: dispatched Worker tab '{tab_id}' has no matching \
             authoritative task binding. Impact: zero-flag reports cannot be attached safely. \
             Fallback: pass explicit --plan {plan_id} --task {task_id} and alert the Orchestrator; \
             no plan or task was guessed. Resolved binding: {binding:?}"
        )),
    }
}

pub(in crate::server) fn owned_str(params: Option<&Value>, key: &str) -> Option<String> {
    params
        .and_then(|params| params.get(key))
        .and_then(Value::as_str)
        .map(str::to_string)
}

pub(in crate::server) fn dispatch_reasoning_effort(
    params: Option<&Value>,
) -> (Option<String>, Option<String>) {
    let requested = owned_str(params, "reasoning_effort");
    (requested.clone(), requested)
}

pub(in crate::server) fn set_local_task_name_description(
    svc: &ServiceState,
    params: Option<&Value>,
    tab_id: &str,
    task_name: &str,
    update: bool,
) -> Result<(), JsonRpcError> {
    if params
        .and_then(|params| params.get("peer"))
        .and_then(Value::as_str)
        .is_some()
    {
        return Ok(());
    }
    let description = task_tab_description(task_name, None);
    let result = if update {
        svc.update_task_tab_description(tab_id, &description)
    } else {
        svc.assign_task_tab_description(tab_id, &description)
    };
    result.map(|_| ()).map_err(|message| JsonRpcError {
        code: -32602,
        message,
        data: None,
    })
}

pub(in crate::server) fn respawn_worker_params(
    draft: &codeterm_service_core::service::plan::task_attempts::TaskRespawnDraft,
    task_name: &str,
    lane_base: Option<String>,
    parent_tab_id: &str,
) -> SpawnAgentParams {
    SpawnAgentParams {
        agent: Some(draft.provider.clone()),
        cwd: Some(draft.cwd.clone()),
        title: draft.title.clone(),
        title_provenance: Some(codeterm_service_core::service::respawn_title_provenance(
            draft.explicit_title,
        )),
        description: Some(task_tab_description(task_name, None)),
        task_description_managed: true,
        role: Some("Worker".into()),
        execution_intent: Some(draft.execution_intent.clone()),
        parent_tab_id: Some(parent_tab_id.to_string()),
        task: Some(super::canonical_writer::compose_dispatch_brief(
            draft.task_prompt.clone(),
            draft.source_attempt.placement,
        )),
        workspace_id: draft.workspace_id.clone(),
        model: draft.model.clone(),
        speed: draft.speed.clone(),
        planned_model: draft.planned_model.clone(),
        planned_reasoning_effort: draft.reasoning_effort.clone(),
        reasoning_effort: draft.reasoning_effort.clone(),
        task_launch_authority: Some(codeterm_service_core::service::TaskLaunchAuthority {
            plan_id: draft.plan_id.clone(),
            task_id: draft.task_id.clone(),
            attempt_id: draft.attempt_id.clone(),
            lane_base,
        }),
        spawned_by_tab_id: Some(parent_tab_id.to_string()),
        ..Default::default()
    }
}

pub(super) fn rollback_failed_dispatch<R, C>(
    lane: Option<codeterm_service_core::service::ProvisionedWorkerLane>,
    revert_task: R,
    close_tab: C,
) -> Result<(), String>
where
    R: FnOnce() -> Result<(), String>,
    C: FnOnce() -> Result<(), String>,
{
    let mut errors = Vec::new();
    if let Err(error) = revert_task() {
        errors.push(format!("task restore failed: {error}"));
    }
    if let Err(error) = close_tab() {
        errors.push(format!("Tab close failed: {error}"));
    }
    if let Some(lane) = lane {
        if let Err(error) = lane.cleanup() {
            errors.push(format!("lane cleanup failed: {error}"));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codeterm_service_core::plan::ExecutionIntent;
    use codeterm_service_core::service::plan::task_attempts::TaskRespawnDraft;

    #[test]
    fn respawn_launch_inherits_source_identity_and_task_execution_intent() {
        let draft = TaskRespawnDraft {
            plan_id: "pl_opaque".into(),
            task_id: "task_opaque".into(),
            execution_intent: ExecutionIntent::Mutating,
            attempt_id: "attempt_new".into(),
            source_attempt: Default::default(),
            provider: "fixture-agent".into(),
            planned_model: Some("fixture-agent/model".into()),
            model: Some("gpt-6-luna".into()),
            speed: Some("opaque-tier".into()),
            reasoning_effort: None,
            parent_tab_id: None,
            workspace_id: None,
            title: None,
            explicit_title: false,
            cwd: ".".into(),
            task_prompt: "opaque-task".into(),
            report_command: "opaque-report".into(),
            plan_skill_manifest: vec![],
            authorized_gates: None,
        };

        let params = respawn_worker_params(&draft, "opaque task", None, "tab_parent");

        assert_eq!(params.speed.as_deref(), Some("opaque-tier"));
        assert_eq!(params.execution_intent, Some(ExecutionIntent::Mutating));
        assert_eq!(params.agent.as_deref(), Some("fixture-agent"));
        assert_eq!(params.model.as_deref(), Some("gpt-6-luna"));
        assert_eq!(
            params.task_launch_authority.unwrap().attempt_id,
            "attempt_new"
        );
    }
}
