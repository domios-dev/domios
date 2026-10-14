use axum::{
    extract::{Path, State as AxumState},
    http::HeaderMap,
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, AppState};

use super::{emit_plan_changed, epoch_ms, plan_err, text_err, PlanRouteResult};

#[derive(Deserialize)]
pub(crate) struct LinkBody {
    to: String,
}

pub async fn link(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<LinkBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    state
        .svc
        .plan_store
        .link_plan(&id, &body.to, epoch_ms())
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn unlink(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((id, to)): Path<(String, String)>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    state
        .svc
        .plan_store
        .unlink_plan(&id, &to, epoch_ms())
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}
