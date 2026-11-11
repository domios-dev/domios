use axum::{
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use codeterm_team_package::contracts::TemplateUnavailable;
use serde_json::Value;

use super::routes::AppState;

mod exchange;
mod template_start;

pub(crate) type TemplateRouteResult = Result<Json<Value>, (StatusCode, Json<TemplateUnavailable>)>;

/// Maps a service answer onto HTTP; an unimplemented operation is 501 with its typed body.
pub(crate) fn answer(outcome: Result<Value, TemplateUnavailable>) -> TemplateRouteResult {
    outcome
        .map(Json)
        .map_err(|unavailable| (StatusCode::NOT_IMPLEMENTED, Json(unavailable)))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/team-templates", get(template_start::list))
        .route("/api/team-templates/preview", post(template_start::preview))
        .route("/api/team-templates/recall", post(exchange::recall))
        .route("/api/team-templates/export", post(exchange::export))
        .route("/api/team-templates/import", post(exchange::import))
        .route("/api/team-templates/delete", post(exchange::delete))
        .route("/api/team-templates/card", post(exchange::card))
        .route("/api/teams/{team_id}/distill", post(exchange::distill))
}
