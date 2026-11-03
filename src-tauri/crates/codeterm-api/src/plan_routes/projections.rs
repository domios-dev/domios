use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    Json,
};
use codeterm_service_core::service::plan::ui_projections::PlanViewProjection;
use codeterm_service_core::service::plan::PlanError;
use serde::Deserialize;

use super::{plan_err, PlanRouteResult};
use crate::routes::AppState;

#[derive(Default, Deserialize)]
pub struct ProjectionQuery {
    #[serde(default)]
    view: PlanViewProjection,
    project: Option<String>,
}

pub async fn get(
    State(state): State<AppState>,
    _headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<ProjectionQuery>,
) -> PlanRouteResult {
    super::run_blocking(move || {
        codeterm_orchestration::validate_plan_team_binding(&state.svc, &id)
            .map_err(super::plans::team_plan_err)?;
        let projection = state
            .svc
            .plan_store
            .ui_plan_projection(&id, query.view)
            .map_err(plan_err)?;
        if let Some(project) = query.project {
            let root = codeterm_service_core::service::plan::paths::project_root(
                std::path::Path::new(&project),
            );
            if projection["project_path"].as_str() != Some(root.to_string_lossy().as_ref()) {
                return Err(plan_err(PlanError::NotFound));
            }
        }
        Ok(Json(projection))
    })
    .await
}

pub async fn board_state(
    State(state): State<AppState>,
    _headers: HeaderMap,
    Path(id): Path<String>,
) -> PlanRouteResult {
    super::run_blocking(move || {
        codeterm_orchestration::validate_plan_team_binding(&state.svc, &id)
            .map_err(super::plans::team_plan_err)?;
        let snapshot = state
            .svc
            .build_plan_state(&id, super::epoch_ms())
            .map_err(plan_err)?;
        Ok(Json(serde_json::json!({
            "plan_id": snapshot.plan_id,
            "revision": snapshot.revision,
            "task_counts": snapshot.task_counts,
            "control_loop": snapshot.control_loop,
            "mission_control": snapshot.mission_control,
        })))
    })
    .await
}
