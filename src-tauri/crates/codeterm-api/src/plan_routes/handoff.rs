use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    extract::{Path, Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use serde_json::Value;

use crate::routes::{check_auth, AppState};
use codeterm_service_core::service::plan::{handoff::HandoffCreateRequest, PlanError};

type HandoffRouteResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn text_err(status: StatusCode, message: impl ToString) -> (StatusCode, Json<Value>) {
    (
        status,
        Json(serde_json::json!({ "error": message.to_string() })),
    )
}

fn plan_err(e: PlanError) -> (StatusCode, Json<Value>) {
    let status = match e {
        PlanError::NotFound | PlanError::TaskNotFound => StatusCode::NOT_FOUND,
        _ => StatusCode::BAD_REQUEST,
    };
    (
        status,
        Json(serde_json::json!({
            "error": e.to_string(),
            "plan_error": e.to_json(),
        })),
    )
}

fn emit_plan_changed(state: &AppState, plan_id: &str) {
    crate::events::emit_everywhere(
        &state.svc.app_handle,
        "plan-changed",
        serde_json::json!({ "plan_id": plan_id }),
    );
}

#[derive(Deserialize, Default)]
pub struct ShowQuery {
    selector: Option<String>,
    latest: Option<bool>,
}

pub async fn create(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<HandoffCreateRequest>,
) -> HandoffRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let result = state
        .svc
        .plan_store
        .create_handoff(&id, body, epoch_ms())
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::to_value(result).unwrap_or_default()))
}

pub async fn list(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> HandoffRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let result = state.svc.plan_store.list_handoffs(&id).map_err(plan_err)?;
    Ok(Json(serde_json::to_value(result).unwrap_or_default()))
}

pub async fn show(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<ShowQuery>,
) -> HandoffRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let selector = if query.latest.unwrap_or(false) {
        None
    } else {
        query.selector.as_deref()
    };
    let result = state
        .svc
        .plan_store
        .show_handoff(&id, selector)
        .map_err(plan_err)?;
    Ok(Json(serde_json::to_value(result).unwrap_or_default()))
}
