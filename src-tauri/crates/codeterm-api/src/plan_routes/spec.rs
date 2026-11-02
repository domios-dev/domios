use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    extract::{Path, Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use serde_json::Value;

use crate::routes::{check_auth, AppState};
use codeterm_service_core::plan::{EffortLinkRel, PlanStatus};
use codeterm_service_core::service::plan::PlanError;

type RouteResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

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
    state
        .svc
        .ui
        .emit("plan-changed", serde_json::json!({ "plan_id": plan_id }));
}

fn emit_spec_changed(state: &AppState, spec_id: &str) {
    state
        .svc
        .ui
        .emit("spec-changed", serde_json::json!({ "spec_id": spec_id }));
}

fn de_truthy<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    let s = <Option<String> as serde::Deserialize>::deserialize(d)?;
    Ok(s.map(|v| matches!(v.as_str(), "1" | "true" | "yes" | "on")))
}

#[derive(Deserialize, Default)]
pub struct CreateSpecBody {
    title: String,
    #[serde(default)]
    project_path: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct ListSpecsQuery {
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default, deserialize_with = "de_truthy")]
    full: Option<bool>,
}

#[derive(Deserialize, Default)]
pub struct ShowSpecQuery {
    #[serde(default, deserialize_with = "de_truthy")]
    full: Option<bool>,
}

#[derive(Deserialize)]
pub struct SectionAddBody {
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

#[derive(Deserialize, Default)]
pub struct SectionUpdateBody {
    #[serde(default)]
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

#[derive(Deserialize, Default)]
pub struct SpecLinkBody {
    spec_id: String,
    spec_section_id: String,
    plan_section_id: String,
}

#[derive(Deserialize, Default)]
pub struct SpecLinkDeleteQuery {
    #[serde(default)]
    spec_section_id: Option<String>,
    #[serde(default)]
    plan_section_id: Option<String>,
}

#[derive(Deserialize)]
pub struct EffortLinkBody {
    to: String,
    rel: String,
}

pub async fn create(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateSpecBody>,
) -> RouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let project = body.project_path.unwrap_or_else(|| {
        std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default()
    });
    let spec = state
        .svc
        .plan_store
        .create_spec(body.title, project, epoch_ms())
        .map_err(plan_err)?;
    emit_spec_changed(&state, &spec.id);
    Ok(Json(
        serde_json::json!({ "spec_id": spec.id, "spec": spec }),
    ))
}

pub async fn list(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(q): Query<ListSpecsQuery>,
) -> RouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let want = if let Some(project) = &q.project {
        Some(
            state
                .svc
                .ensure_plan_project_loaded(std::path::Path::new(project))
                .map_err(plan_err)?
                .display()
                .to_string(),
        )
    } else {
        None
    };
    let mut entries = state.svc.plan_store.list_spec_index();
    if let Some(want) = &want {
        entries.retain(|e| e.project_path == *want);
    }
    if let Some(status) = &q.status {
        entries.retain(|e| match status.as_str() {
            "active" => e.status == PlanStatus::Active,
            "archived" => e.status == PlanStatus::Archived,
            _ => true,
        });
    }
    if q.full == Some(true) {
        let specs = entries
            .iter()
            .filter_map(|e| state.svc.plan_store.spec_full(&e.spec_id))
            .collect::<Vec<_>>();
        return Ok(Json(serde_json::json!({ "specs": specs })));
    }
    Ok(Json(serde_json::json!({ "specs": entries })))
}

pub async fn get(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<ShowSpecQuery>,
) -> RouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    if q.full == Some(true) {
        return state
            .svc
            .plan_store
            .spec_full(&id)
            .map(Json)
            .ok_or_else(|| plan_err(PlanError::NotFound));
    }
    let spec = state
        .svc
        .plan_store
        .get_spec(&id)
        .ok_or_else(|| plan_err(PlanError::NotFound))?;
    Ok(Json(serde_json::json!({ "spec": spec })))
}

pub async fn section_add(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<SectionAddBody>,
) -> RouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let sid = state
        .svc
        .plan_store
        .add_spec_section(
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
    emit_spec_changed(&state, &id);
    Ok(Json(serde_json::json!({ "section_id": sid })))
}

pub async fn section_update(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((id, sid)): Path<(String, String)>,
    Json(body): Json<SectionUpdateBody>,
) -> RouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    state
        .svc
        .plan_store
        .update_spec_section(
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
    emit_spec_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn section_rm(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((id, sid)): Path<(String, String)>,
) -> RouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    state
        .svc
        .plan_store
        .rm_spec_section(&id, &sid, epoch_ms())
        .map_err(plan_err)?;
    emit_spec_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn link_add(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<SpecLinkBody>,
) -> RouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    state
        .svc
        .plan_store
        .link_spec_section(
            &id,
            &body.spec_id,
            &body.spec_section_id,
            &body.plan_section_id,
            epoch_ms(),
        )
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn link_rm(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((id, spec_id)): Path<(String, String)>,
    Query(q): Query<SpecLinkDeleteQuery>,
) -> RouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    state
        .svc
        .plan_store
        .unlink_spec_section(
            &id,
            &spec_id,
            q.spec_section_id.as_deref(),
            q.plan_section_id.as_deref(),
            epoch_ms(),
        )
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn link_list(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> RouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    Ok(Json(serde_json::json!({
        "links": state.svc.plan_store.list_spec_links(&id)
    })))
}

pub async fn effort_show(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> RouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    state
        .svc
        .plan_store
        .effort_bundle(&id)
        .map(Json)
        .ok_or_else(|| plan_err(PlanError::NotFound))
}

pub async fn effort_link(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<EffortLinkBody>,
) -> RouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let rel = match body.rel.as_str() {
        "follows" => EffortLinkRel::Follows,
        "supersedes" => EffortLinkRel::Supersedes,
        _ => {
            return Err(text_err(
                StatusCode::BAD_REQUEST,
                "rel must be follows or supersedes",
            ))
        }
    };
    state
        .svc
        .plan_store
        .link_effort(&id, &body.to, rel, epoch_ms())
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}
