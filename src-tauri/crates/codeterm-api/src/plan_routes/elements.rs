use axum::{
    extract::{Path, State as AxumState},
    http::HeaderMap,
    Json,
};
use serde::Deserialize;
use serde_json::Value;

use crate::routes::AppState;
use codeterm_service_core::service::plan::PlanError;

use super::{emit_plan_changed, epoch_ms, groups_from_csv, plan_err, PlanRouteResult};

#[derive(Deserialize)]
pub struct LayoutSetBody {
    element: String,
    #[serde(default)]
    x: Option<f64>,
    #[serde(default)]
    y: Option<f64>,
}

pub async fn layout_set(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<LayoutSetBody>,
) -> PlanRouteResult {
    crate::plan_routes::run_blocking(move || {
        state
            .svc
            .plan_store
            .set_layout(&id, &body.element, body.x, body.y, epoch_ms())
            .map_err(plan_err)?;
        emit_plan_changed(&state, &id);
        Ok(Json(serde_json::json!({ "ok": true })))
    })
    .await
}

#[derive(Deserialize, Default)]
pub struct NoteAddBody {
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
    _headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<NoteAddBody>,
) -> PlanRouteResult {
    crate::plan_routes::run_blocking(move || {
        let eid = state
            .svc
            .plan_store
            .add_note(&id, &body.text, body.links, body.x, body.y, epoch_ms())
            .map_err(plan_err)?;
        emit_plan_changed(&state, &id);
        Ok(Json(serde_json::json!({ "element_id": eid })))
    })
    .await
}

#[derive(Deserialize, Default)]
pub struct NoteUpdateBody {
    #[serde(default)]
    text: String,
}

pub async fn note_update(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path((id, element_id)): Path<(String, String)>,
    Json(body): Json<NoteUpdateBody>,
) -> PlanRouteResult {
    crate::plan_routes::run_blocking(move || {
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
    })
    .await
}

#[derive(Deserialize)]
pub struct ElementAddBody {
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
    _headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<ElementAddBody>,
) -> PlanRouteResult {
    crate::plan_routes::run_blocking(move || {
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
    })
    .await
}

#[derive(Deserialize, Default)]
pub struct ElementUpdateBody {
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    content: Option<Value>,
    #[serde(default)]
    links: Option<Vec<String>>,
}

pub async fn element_update(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path((id, element_id)): Path<(String, String)>,
    Json(body): Json<ElementUpdateBody>,
) -> PlanRouteResult {
    crate::plan_routes::run_blocking(move || {
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
    })
    .await
}

pub async fn element_rm(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path((id, element_id)): Path<(String, String)>,
) -> PlanRouteResult {
    crate::plan_routes::run_blocking(move || {
        state
            .svc
            .plan_store
            .rm_element(&id, &element_id, epoch_ms())
            .map_err(plan_err)?;
        emit_plan_changed(&state, &id);
        Ok(Json(serde_json::json!({ "ok": true })))
    })
    .await
}

pub async fn coverage(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path(id): Path<String>,
) -> PlanRouteResult {
    crate::plan_routes::run_blocking(move || {
        let report = state
            .svc
            .plan_store
            .coverage_report(&id)
            .map_err(plan_err)?
            .ok_or_else(|| plan_err(PlanError::NotFound))?;
        Ok(Json(serde_json::to_value(report).unwrap_or_default()))
    })
    .await
}

#[derive(Deserialize)]
pub struct GroupsSetBody {
    groups: String,
}

pub async fn groups_set(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<GroupsSetBody>,
) -> PlanRouteResult {
    crate::plan_routes::run_blocking(move || {
        state
            .svc
            .plan_store
            .set_task_groups(&id, groups_from_csv(&body.groups), epoch_ms())
            .map_err(plan_err)?;
        emit_plan_changed(&state, &id);
        Ok(Json(serde_json::json!({ "ok": true })))
    })
    .await
}
