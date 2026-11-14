use axum::{extract::State, http::StatusCode, Json};
use codeterm_service_core::service::team_channels::{self, TeamChannelAction};
use serde_json::{json, Value};

use crate::routes::AppState;

type Response = Result<Json<Value>, (StatusCode, Json<Value>)>;

async fn run(state: AppState, action: TeamChannelAction) -> Response {
    tokio::task::spawn_blocking(move || team_channels::command(&state.svc, action))
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": error.to_string() })),
            )
        })?
        .map(Json)
        .map_err(|error| (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))))
}

pub(super) async fn list(State(state): State<AppState>) -> Response {
    run(state, TeamChannelAction::List).await
}

pub(super) async fn command(
    State(state): State<AppState>,
    Json(action): Json<TeamChannelAction>,
) -> Response {
    run(state, action).await
}
