use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde_json::{json, Value};

use crate::api::routes::{check_auth, AppState};
use crate::plan::AgentRef;
use crate::service::plan::plan_skills::{list_plan_skills, show_plan_skill};

use super::{emit_plan_changed, epoch_ms, plan_err, text_err, PlanRouteResult};

pub async fn skills_list(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(plan_id): Path<String>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let store = state.svc.plan_store.clone();
    let skills = list_plan_skills(&store, &plan_id).map_err(plan_err)?;
    Ok(Json(json!({ "plan_id": plan_id, "skills": skills })))
}

pub async fn skills_show(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((plan_id, slug)): Path<(String, String)>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let store = state.svc.plan_store.clone();
    let skill = show_plan_skill(&store, &plan_id, &slug).map_err(plan_err)?;
    Ok(Json(json!({ "plan_id": plan_id, "skill": skill })))
}

pub async fn retrospective_write(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(plan_id): Path<String>,
    Json(body): Json<Value>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let text = body
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if text.is_empty() {
        return Err(text_err(StatusCode::BAD_REQUEST, "body is required"));
    }
    let now = epoch_ms();
    let from = body
        .get("from")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_else(|| AgentRef::from_tab("unknown"));
    let store = state.svc.plan_store.clone();
    let stride_id = store
        .add_retrospective(&plan_id, &text, from, now)
        .map_err(plan_err)?;
    emit_plan_changed(&state, &plan_id);
    Ok(Json(json!({ "plan_id": plan_id, "stride_id": stride_id })))
}
