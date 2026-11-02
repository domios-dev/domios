use axum::{
    extract::{Path, State as AxumState},
    http::HeaderMap,
    Json,
};
use serde::Deserialize;

use crate::routes::AppState;

use super::{
    authorize_draft_structure_mutation_http, emit_plan_changed, epoch_ms, plan_err, PlanRouteResult,
};

#[derive(Deserialize)]
pub struct SectionAddBody {
    #[serde(alias = "name")]
    title: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    parent: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    primary: Option<String>,
    #[serde(default)]
    handle: Option<String>,
}

pub async fn section_add(
    AxumState(state): AxumState<AppState>,
    headers: crate::auth::ActorHeaders,
    Path(id): Path<String>,
    Json(body): Json<SectionAddBody>,
) -> PlanRouteResult {
    authorize_draft_structure_mutation_http(
        &state.svc,
        &id,
        crate::tab_routes::authenticated_plan_caller(&headers),
    )
    .await?;
    let sid = state
        .svc
        .plan_store
        .add_section(
            &id,
            &body.title,
            &body.body,
            body.parent.as_deref(),
            &body.tags,
            body.primary.as_deref(),
            body.handle.as_deref(),
            epoch_ms(),
        )
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "section_id": sid })))
}

#[derive(Deserialize)]
pub struct IngestBody {
    markdown: String,
}

pub async fn ingest(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<IngestBody>,
) -> PlanRouteResult {
    let result = state
        .svc
        .plan_store
        .ingest_plan_sections(&id, &body.markdown, epoch_ms())
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(
        serde_json::to_value(result).unwrap_or_else(|_| serde_json::json!({ "sections": [] })),
    ))
}

#[derive(Deserialize, Default)]
pub struct SectionUpdateBody {
    #[serde(default)]
    #[serde(alias = "name")]
    title: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    primary: Option<String>,
    #[serde(default)]
    handle: Option<String>,
}

pub async fn section_update(
    AxumState(state): AxumState<AppState>,
    headers: crate::auth::ActorHeaders,
    Path((id, sid)): Path<(String, String)>,
    Json(body): Json<SectionUpdateBody>,
) -> PlanRouteResult {
    authorize_draft_structure_mutation_http(
        &state.svc,
        &id,
        crate::tab_routes::authenticated_plan_caller(&headers),
    )
    .await?;
    state
        .svc
        .plan_store
        .update_section(
            &id,
            &sid,
            body.title.as_deref(),
            body.body.as_deref(),
            &body.tags,
            body.primary.as_deref(),
            body.handle.as_deref(),
            epoch_ms(),
        )
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn section_rm(
    AxumState(state): AxumState<AppState>,
    headers: crate::auth::ActorHeaders,
    Path((id, sid)): Path<(String, String)>,
) -> PlanRouteResult {
    authorize_draft_structure_mutation_http(
        &state.svc,
        &id,
        crate::tab_routes::authenticated_plan_caller(&headers),
    )
    .await?;
    state
        .svc
        .plan_store
        .rm_section(&id, &sid, epoch_ms())
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}
