use tauri::State;

use crate::plan::Plan;
use crate::service::plan::IndexEntry;
use crate::service::{ServiceState, TeamPlanError};

fn team_plan_error(error: TeamPlanError) -> String {
    match error {
        TeamPlanError::Team(message) => message,
        TeamPlanError::Plan(error) => format!("{error:?}"),
    }
}

fn ensure_ready(svc: &ServiceState) -> Result<(), String> {
    svc.validate_active_team_plan_bindings()
        .map_err(team_plan_error)?;
    if !svc.plans_startup_loaded() {
        return Err("plans are still loading".to_string());
    }
    Ok(())
}

#[tauri::command]
pub fn list_plan_index(svc: State<'_, ServiceState>) -> Result<Vec<IndexEntry>, String> {
    ensure_ready(&svc)?;
    Ok(svc.plan_store.list_index())
}

#[tauri::command]
pub fn list_plans_full(
    svc: State<'_, ServiceState>,
    project: Option<String>,
) -> Result<Vec<Plan>, String> {
    if let Some(project) = project.as_deref() {
        svc.ensure_plan_project_loaded(std::path::Path::new(project))
            .map_err(|error| error.to_string())?;
    } else {
        ensure_ready(&svc)?;
    }
    let expected = project.as_deref();
    Ok(svc
        .plan_store
        .list_index()
        .into_iter()
        .filter_map(|entry| svc.plan_store.get(&entry.plan_id))
        .filter(|plan| expected.is_none_or(|path| plan.project_path == path))
        .collect())
}

#[tauri::command]
pub fn get_plan_full(svc: State<'_, ServiceState>, plan_id: String) -> Result<Plan, String> {
    ensure_ready(&svc)?;
    svc.plan_store
        .get(&plan_id)
        .ok_or_else(|| format!("plan not found: {plan_id}"))
}
