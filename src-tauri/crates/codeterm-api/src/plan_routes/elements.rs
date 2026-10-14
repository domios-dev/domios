use axum::{
    extract::{Path, State as AxumState},
    http::HeaderMap,
    Json,
};
use serde::Deserialize;
use serde_json::Value;

use crate::routes::{check_auth, AppState};
use codeterm_service_core::service::plan::PlanError;

use super::{emit_plan_changed, epoch_ms, groups_from_csv, plan_err, text_err, PlanRouteResult};

#[derive(Deserialize)]
pub(crate) struct LayoutSetBody {
    element: String,
    #[serde(default)]
    x: Option<f64>,
    #[serde(default)]
    y: Option<f64>,
}

pub async fn layout_set(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<LayoutSetBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    state
        .svc
        .plan_store
        .set_layout(&id, &body.element, body.x, body.y, epoch_ms())
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize, Default)]
pub(crate) struct NoteAddBody {
    #[serde(default)]
    text: String,
    #[serde(default)]
    links: Vec<String>,
    #[serde(default)]
    x: Option<f64>,
    #[serde(default)]
    y: Option<f64>,
}

pub async fn note_add(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<NoteAddBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let eid = state
        .svc
        .plan_store
        .add_note(&id, &body.text, body.links, body.x, body.y, epoch_ms())
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "element_id": eid })))
}

#[derive(Deserialize, Default)]
pub(crate) struct NoteUpdateBody {
    #[serde(default)]
    text: String,
}

pub async fn note_update(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((id, element_id)): Path<(String, String)>,
    Json(body): Json<NoteUpdateBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    state
        .svc
        .plan_store
        .update_element(
            &id,
            &element_id,
            Some("note"),
            Some(serde_json::json!({ "text": body.text })),
            None,
            epoch_ms(),
        )
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub(crate) struct ElementAddBody {
    kind: String,
    #[serde(default)]
    content: Value,
    #[serde(default)]
    links: Vec<String>,
    #[serde(default)]
    x: Option<f64>,
    #[serde(default)]
    y: Option<f64>,
}

pub async fn element_add(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<ElementAddBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let eid = state
        .svc
        .plan_store
        .add_element(
            &id,
            &body.kind,
            body.content,
            body.links,
            body.x,
            body.y,
            epoch_ms(),
        )
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "element_id": eid })))
}

#[derive(Deserialize, Default)]
pub(crate) struct ElementUpdateBody {
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    content: Option<Value>,
    #[serde(default)]
    links: Option<Vec<String>>,
}

pub async fn element_update(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((id, element_id)): Path<(String, String)>,
    Json(body): Json<ElementUpdateBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    state
        .svc
        .plan_store
        .update_element(
            &id,
            &element_id,
            body.kind.as_deref(),
            body.content,
            body.links,
            epoch_ms(),
        )
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn element_rm(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((id, element_id)): Path<(String, String)>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    state
        .svc
        .plan_store
        .rm_element(&id, &element_id, epoch_ms())
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn coverage(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let report = state
        .svc
        .plan_store
        .coverage_report(&id)
        .ok_or_else(|| plan_err(PlanError::NotFound))?;
    Ok(Json(serde_json::to_value(report).unwrap_or_default()))
}

#[derive(Deserialize)]
pub(crate) struct GroupsSetBody {
    groups: String,
}

pub async fn groups_set(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<GroupsSetBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    state
        .svc
        .plan_store
        .set_task_groups(&id, groups_from_csv(&body.groups), epoch_ms())
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}
