use crate::args::agent::SpawnRole;

pub(super) fn validate_spawn_context(
    role: Option<&SpawnRole>,
    has_workspace: bool,
    has_parent: bool,
    has_plan: bool,
    has_bound_task: bool,
    has_plan_task: bool,
) -> Result<(), &'static str> {
    if matches!(role, Some(SpawnRole::Orchestrator)) && !has_workspace && !has_parent {
        return Err("--workspace is required when spawning --role Orchestrator");
    }
    if matches!(role, Some(SpawnRole::Planner)) && !has_plan && !has_parent {
        return Err("--plan <id> is required when spawning --role Planner (or pass --parent)");
    }
    if matches!(role, Some(SpawnRole::Worker)) && has_plan && !has_bound_task && !has_plan_task {
        return Err(
            "--role Worker with --plan requires --task-id or --plan-task; omit --role/--plan for a roleless solo agent",
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_bound_worker_requires_an_exact_task_identity() {
        assert!(
            validate_spawn_context(Some(&SpawnRole::Worker), false, false, true, false, false)
                .is_err()
        );
        assert!(
            validate_spawn_context(Some(&SpawnRole::Worker), false, false, true, true, false)
                .is_ok()
        );
        assert!(
            validate_spawn_context(Some(&SpawnRole::Worker), false, false, true, false, true)
                .is_ok()
        );
        assert!(validate_spawn_context(
            Some(&SpawnRole::Worker),
            false,
            false,
            false,
            false,
            false
        )
        .is_ok());
        assert!(validate_spawn_context(None, false, false, true, false, false).is_ok());
    }
}
