use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde_json::Value;

use crate::api::routes::{check_auth, AppState};
use crate::service::plan::batch::{BatchRequest, BatchResult};
use crate::service::plan::{PlanError, PlanStore};
use crate::service::plan_batch_authority::{apply_batch_from_http, BatchWriteError};

type BatchRouteResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

/// Wall clock for the write, unrelated to the task owner-epoch mechanism: this
/// is not an authority check, and neither is `check_auth` below.
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
        PlanError::ReviewHoldRequired => StatusCode::CONFLICT,
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

/// The route's whole write: parse the ops, cross the authority boundary, apply.
/// The handler adds only authentication and the change event.
pub(crate) fn apply_authorized_batch(
    store: &PlanStore,
    plan_id: &str,
    body: &Value,
    now_ms: u64,
) -> Result<BatchResult, (StatusCode, Json<Value>)> {
    let request: BatchRequest = serde_json::from_value(body.clone())
        .map_err(|e| text_err(StatusCode::BAD_REQUEST, format!("invalid batch body: {e}")))?;
    apply_batch_from_http(store, plan_id, body, request, now_ms).map_err(|error| match error {
        BatchWriteError::Plan(e) => plan_err(e),
        BatchWriteError::Refused(refusal) => {
            let plan_error = refusal.to_json();
            (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({
                    "error": refusal.message,
                    "plan_error": plan_error,
                })),
            )
        }
    })
}

pub async fn batch(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> BatchRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let result = apply_authorized_batch(&state.svc.plan_store, &id, &body, epoch_ms())?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::to_value(result).unwrap_or_default()))
}
