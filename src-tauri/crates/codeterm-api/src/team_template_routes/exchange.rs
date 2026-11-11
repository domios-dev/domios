use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use codeterm_service_core::service::team_exchange::{
    self, ExchangeError, INVALID_REQUEST, NOT_FOUND,
};
use codeterm_service_core::service::team_template_recall;
use serde_json::{json, Value};

use crate::routes::AppState;

type ExchangeResult = Result<Json<Value>, (StatusCode, Json<ExchangeError>)>;

/// A bad request is 400, an unknown template 404 and a refused package 422 with diagnostics.
fn answer(outcome: Result<Value, ExchangeError>) -> ExchangeResult {
    outcome.map(Json).map_err(|error| {
        let status = match error.code {
            INVALID_REQUEST => StatusCode::BAD_REQUEST,
            NOT_FOUND => StatusCode::NOT_FOUND,
            _ => StatusCode::UNPROCESSABLE_ENTITY,
        };
        (status, Json(error))
    })
}

/// Package work reads and writes files; it runs off the async runtime.
async fn blocking(
    state: AppState,
    work: fn(
        &codeterm_service_core::service::ServiceContext,
        Option<&Value>,
    ) -> Result<Value, ExchangeError>,
    body: Value,
) -> ExchangeResult {
    let svc = state.svc.clone();
    let outcome = tokio::task::spawn_blocking(move || work(svc.as_ref(), Some(&body)))
        .await
        .unwrap_or_else(|error| Err(ExchangeError::invalid(error.to_string())));
    answer(outcome)
}

pub(super) async fn recall(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> ExchangeResult {
    blocking(state, team_template_recall::recall, body).await
}

pub(super) async fn export(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> ExchangeResult {
    blocking(state, team_exchange::export, body).await
}

pub(super) async fn import(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> ExchangeResult {
    blocking(state, team_exchange::import, body).await
}

pub(super) async fn delete(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> ExchangeResult {
    blocking(state, team_exchange::delete, body).await
}

pub(super) async fn card(State(state): State<AppState>, Json(body): Json<Value>) -> ExchangeResult {
    blocking(state, |_, params| team_exchange::share_card(params), body).await
}

pub(super) async fn distill(
    State(state): State<AppState>,
    Path(team_id): Path<String>,
    Json(mut body): Json<Value>,
) -> ExchangeResult {
    if let Some(object) = body.as_object_mut() {
        object.insert("team_id".into(), json!(team_id));
    }
    blocking(state, team_exchange::distill, body).await
}
