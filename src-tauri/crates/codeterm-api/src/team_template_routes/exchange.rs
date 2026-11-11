use axum::{
    extract::{Path, State},
    Json,
};
use codeterm_service_core::service::{team_exchange, team_template_recall};
use serde_json::{json, Value};

use super::{answer, TemplateRouteResult};
use crate::routes::AppState;

pub(super) async fn recall(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> TemplateRouteResult {
    answer(team_template_recall::recall(&state.svc, Some(&body)))
}

pub(super) async fn export(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> TemplateRouteResult {
    answer(team_exchange::export(&state.svc, Some(&body)))
}

pub(super) async fn import(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> TemplateRouteResult {
    answer(team_exchange::import(&state.svc, Some(&body)))
}

pub(super) async fn distill(
    State(state): State<AppState>,
    Path(team_id): Path<String>,
    Json(mut body): Json<Value>,
) -> TemplateRouteResult {
    if let Some(object) = body.as_object_mut() {
        object.insert("team_id".into(), json!(team_id));
    }
    answer(team_exchange::distill(&state.svc, Some(&body)))
}
