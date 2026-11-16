use axum::{extract::State, http::StatusCode, Json};
use codeterm_service_core::service::team_template_start::{self, TemplateCallError};
use codeterm_team_package::contracts::TemplateStartResult;
use serde_json::{json, Value};

use crate::routes::AppState;

type StartRouteResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

fn encode(value: Result<Value, serde_json::Error>) -> StartRouteResult {
    value.map(Json).map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        )
    })
}

pub(super) async fn list(State(state): State<AppState>) -> StartRouteResult {
    let list = tokio::task::spawn_blocking(move || team_template_start::list(&state.svc))
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": error.to_string() })),
            )
        })?;
    encode(serde_json::to_value(list))
}

/// A refused preview answers 422 with the typed result the start form already reads.
pub(super) async fn preview(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> StartRouteResult {
    let svc = state.svc.clone();
    let outcome =
        tokio::task::spawn_blocking(move || team_template_start::preview(&svc, Some(&body)))
            .await
            .map_err(|error| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": error.to_string() })),
                )
            })?;
    match outcome {
        Ok(preview) => encode(serde_json::to_value(preview)),
        Err(TemplateCallError::InvalidParams(message)) => {
            Err((StatusCode::BAD_REQUEST, Json(json!({ "error": message }))))
        }
        Err(TemplateCallError::Refused(refusal)) => Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!(TemplateStartResult::Refused { refusal })),
        )),
    }
}
