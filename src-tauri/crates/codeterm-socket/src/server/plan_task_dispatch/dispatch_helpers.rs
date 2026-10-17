use super::{JsonRpcError, ServiceState, SpawnAgentParams};
use serde_json::Value;

pub(in crate::server) use codeterm_service_core::service::task_tab_description;

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
        parent_tab_id: Some(parent_tab_id.to_string()),
        task: Some(draft.task_prompt.clone()),
        workspace_id: draft.workspace_id.clone(),
        model: draft.model.clone(),
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
