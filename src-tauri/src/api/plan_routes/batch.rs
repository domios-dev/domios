use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde_json::Value;

use crate::api::routes::{check_auth, AppState};
use crate::service::plan::{batch::BatchRequest, PlanError};

type BatchRouteResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

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
    crate::api::events::emit_everywhere(
        &state.svc.app_handle,
        "plan-changed",
        serde_json::json!({ "plan_id": plan_id }),
    );
}

pub async fn batch(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<BatchRequest>,
) -> BatchRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let result = state
        .svc
        .plan_store
        .apply_batch(&id, body, epoch_ms())
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::to_value(result).unwrap_or_default()))
}
