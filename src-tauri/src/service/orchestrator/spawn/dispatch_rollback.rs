use super::SpawnAgentParams;
use crate::service::ServiceContext;

fn revert_task_dispatch_after_launch_failure(
    service: &ServiceContext,
    params: &SpawnAgentParams,
    failure: &str,
) -> String {
    let Some(task_ref) = crate::service::spawn_plan_task::worker_spawn_plan_task_ref(params) else {
        return failure.to_string();
    };
    let resolved = match service
        .plan_store
        .resolve_task_ref(params.plan_id.as_deref(), task_ref)
    {
        Ok(resolved) => resolved,
        Err(error) => {
            return format!(
                "{failure}; task dispatch rollback target could not be resolved: {error}"
            )
        }
    };
    match service.plan_store.revert_task_dispatch(
        &resolved.plan_id,
        &resolved.task_id,
        crate::service::wall_now_ms(),
    ) {
        Ok(()) => {
            service.emit_plan_changed(&resolved.plan_id);
            failure.to_string()
        }
        Err(error) => format!("{failure}; task dispatch rollback failed: {error}"),
    }
}

pub(super) fn create_pending_delivery_or_rollback_dispatch(
    service: &ServiceContext,
    params: &SpawnAgentParams,
    tab_id: &str,
    spec: &crate::service::launch_delivery::FallbackDeliverySpec,
) -> Result<(), String> {
    let Err(error) = service.create_pending_delivery(tab_id, spec.channel.clone(), &spec.payload)
    else {
        return Ok(());
    };
    let failure = revert_task_dispatch_after_launch_failure(service, params, &error.to_string());
    Err(service.rollback_failed_agent_spawn_if_created(
        tab_id,
        &failure,
        params.existing_tab_id.is_none(),
    ))
}
