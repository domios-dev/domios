use axum::{
    extract::{Path, Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, AppState};
use crate::plan::{OrchestratorRef, PlanStatus};
use crate::service::plan::PlanError;

use super::{emit_plan_changed, epoch_ms, kinds_from_csv, plan_err, text_err, PlanRouteResult};

#[derive(Deserialize, Default)]
pub(crate) struct CreatePlanBody {
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) project_path: Option<String>,
    #[serde(default)]
    pub(crate) owner: Option<OrchestratorRef>,
    #[serde(default)]
    pub(crate) kinds: Option<String>,
    #[serde(default)]
    pub(crate) from_spec: Option<String>,
}

fn de_truthy<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    let s = <Option<String> as serde::Deserialize>::deserialize(d)?;
    Ok(s.map(|v| matches!(v.as_str(), "1" | "true" | "yes" | "on")))
}

#[derive(Deserialize, Default)]
pub(crate) struct ListPlansQuery {
    #[serde(default)]
    pub(crate) project: Option<String>,
    #[serde(default)]
    pub(crate) status: Option<String>,
    #[serde(default)]
    pub(crate) orchestrator: Option<String>,
    #[serde(default, deserialize_with = "de_truthy")]
    pub(crate) unowned: Option<bool>,
    #[serde(default, deserialize_with = "de_truthy")]
    pub(crate) full: Option<bool>,
}

pub async fn create(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreatePlanBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let project = body.project_path.unwrap_or_else(|| {
        std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default()
    });
    let owner = body
        .owner
        .unwrap_or_else(|| OrchestratorRef::from_pane("unknown"));
    let kinds = body
        .kinds
        .as_deref()
        .map(kinds_from_csv)
        .unwrap_or_default();
    let now = epoch_ms();
    let from_spec = body.from_spec.filter(|s| !s.trim().is_empty());
    let plan = if let Some(spec_id) = from_spec.as_deref() {
        state
            .svc
            .plan_store
            .create_plan_from_spec(spec_id, body.title, owner, kinds, now)
            .map_err(plan_err)?
    } else {
        state
            .svc
            .plan_store
            .create_plan(body.title, project, owner, kinds, now)
            .map_err(plan_err)?
    };
    emit_plan_changed(&state, &plan.id);
    Ok(Json(serde_json::json!({
        "plan_id": plan.id,
        "plan": plan,
        "from_spec": from_spec.unwrap_or_default()
    })))
}

pub async fn list(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(q): Query<ListPlansQuery>,
) -> PlanRouteResult {
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
    let mut entries = state.svc.plan_store.list_index();
    if let Some(want) = &want {
        entries.retain(|e| {
            state
                .svc
                .plan_store
                .get(&e.plan_id)
                .map(|p| p.project_path == *want)
                .unwrap_or(false)
        });
    }
    if let Some(status) = &q.status {
        entries.retain(|e| match status.as_str() {
            "active" => e.status == PlanStatus::Active,
            "archived" => e.status == PlanStatus::Archived,
            _ => true,
        });
    }
    if q.unowned == Some(true) {
        entries.retain(|e| e.owner.pane_id.is_none() && e.owner.session_id.is_none());
    }
    if let Some(orch) = &q.orchestrator {
        entries.retain(|e| {
            e.owner.pane_id.as_deref() == Some(orch.as_str())
                || e.owner.session_id.as_deref() == Some(orch.as_str())
        });
    }
    if q.full == Some(true) {
        let plans: Vec<_> = entries
            .iter()
            .filter_map(|e| state.svc.plan_store.get(&e.plan_id))
            .collect();
        return Ok(Json(serde_json::json!({ "plans": plans })));
    }
    Ok(Json(serde_json::json!({ "plans": entries })))
}

pub async fn get(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let plan = state
        .svc
        .plan_store
        .get(&id)
        .ok_or_else(|| plan_err(PlanError::NotFound))?;
    Ok(Json(serde_json::to_value(plan).unwrap_or_default()))
}

#[derive(Deserialize, Default)]
pub(crate) struct ReconcileQuery {
    #[serde(default, deserialize_with = "de_truthy")]
    fix: Option<bool>,
    #[serde(default)]
    caller_pane_id: Option<String>,
}

pub async fn reconcile(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<ReconcileQuery>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let report = state
        .svc
        .reconcile_plan(
            &id,
            q.fix == Some(true),
            q.caller_pane_id.as_deref(),
            epoch_ms(),
        )
        .map_err(plan_err)?;
    Ok(Json(serde_json::to_value(report).unwrap_or_default()))
}

#[derive(Deserialize, Default)]
pub(crate) struct UpdatePlanBody {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    status: Option<String>,
}

pub async fn update(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<UpdatePlanBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let status = body
        .status
        .as_deref()
        .map(|s| serde_json::from_value::<PlanStatus>(serde_json::Value::String(s.into())))
        .transpose()
        .map_err(|_| plan_err(PlanError::BadStatus))?;
    let now = epoch_ms();
    state
        .svc
        .plan_store
        .update_plan(&id, body.title.as_deref(), status, now)
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn delete_plan(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    state
        .svc
        .plan_store
        .delete(&id)
        .map_err(|e| text_err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn archive(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    state
        .svc
        .plan_store
        .archive(&id, epoch_ms())
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub(crate) struct TransferBody {
    to: OrchestratorRef,
    #[serde(default)]
    reason: String,
}

pub async fn transfer(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<TransferBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    state
        .svc
        .plan_store
        .transfer(&id, body.to, &body.reason, epoch_ms())
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn disown(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    state
        .svc
        .plan_store
        .disown(&id, epoch_ms())
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub(crate) struct ClaimBody {
    owner: OrchestratorRef,
}

pub async fn claim(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<ClaimBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    state
        .svc
        .plan_store
        .claim(&id, body.owner, epoch_ms())
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub(crate) struct KindsSetBody {
    kinds: String,
}

pub async fn kinds_set(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<KindsSetBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    state
        .svc
        .plan_store
        .set_kinds(&id, kinds_from_csv(&body.kinds), epoch_ms())
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::json!({ "ok": true })))
}
