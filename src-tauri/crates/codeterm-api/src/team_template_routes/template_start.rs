use axum::{extract::State, Json};
use codeterm_service_core::service::team_template_start;
use serde_json::Value;

use super::{answer, TemplateRouteResult};
use crate::routes::AppState;

pub(super) async fn list(State(state): State<AppState>) -> TemplateRouteResult {
    answer(team_template_start::list(&state.svc))
}

pub(super) async fn preview(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> TemplateRouteResult {
    answer(team_template_start::preview(&state.svc, Some(&body)))
}
