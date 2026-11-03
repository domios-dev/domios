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
    #[serde(flatten)]
    window: codeterm_service_core::service::plan::ui_projections::ListWindowQuery,
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
        let projection = if query.view == PlanViewProjection::TaskMetadata {
            state
                .svc
                .plan_store
                .ui_plan_metadata_window(&id, &query.window)
        } else if query.view == PlanViewProjection::List {
            state.svc.plan_store.ui_plan_list_window(&id, &query.window)
        } else {
            state.svc.plan_store.ui_plan_projection(&id, query.view)
        }
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

#[derive(Default, Deserialize)]
pub struct BoardStateQuery {
    event_cursor: Option<usize>,
    events: Option<bool>,
}

pub async fn board_state(
    State(state): State<AppState>,
    _headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<BoardStateQuery>,
) -> PlanRouteResult {
    super::run_blocking(move || {
        board_state_core(
            &state.svc,
            &id,
            query.event_cursor.unwrap_or(0),
            query.events.unwrap_or(true),
        )
    })
    .await
}

#[derive(Deserialize)]
pub struct TaskReferenceQuery {
    reference: String,
    project: Option<String>,
    team: Option<String>,
}

pub async fn task_reference(
    State(state): State<AppState>,
    Query(query): Query<TaskReferenceQuery>,
) -> PlanRouteResult {
    super::run_blocking(move || {
        let value = state
            .svc
            .plan_store
            .task_reference(
                &query.reference,
                query.project.as_deref(),
                query.team.as_deref(),
            )
            .map_err(plan_err)?;
        let id = value["plan_id"]
            .as_str()
            .ok_or_else(|| plan_err(PlanError::NotFound))?;
        codeterm_orchestration::validate_plan_team_binding(&state.svc, id)
            .map_err(super::plans::team_plan_err)?;
        Ok(Json(value))
    })
    .await
}

pub(super) fn board_state_core(
    svc: &codeterm_service_core::service::ServiceContext,
    id: &str,
    event_cursor: usize,
    include_events: bool,
) -> PlanRouteResult {
    codeterm_orchestration::validate_plan_team_binding(svc, id)
        .map_err(super::plans::team_plan_err)?;
    svc.build_plan_board_state(id, super::epoch_ms(), event_cursor, include_events)
        .map(Json)
        .map_err(plan_err)
}
