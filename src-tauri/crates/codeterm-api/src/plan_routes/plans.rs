use axum::{
    extract::{Path, Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use codeterm_plan::service_plan::state::window::StateWindowRequest;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::routes::AppState;
use codeterm_service_core::plan::{
    OrchestratorRef, PlanMode, PlanStatus, PlannerRef, PlanningAuthor, PlanningState,
};
use codeterm_service_core::service::plan::PlanError;
use codeterm_service_core::service::TeamPlanCreationError;
use codeterm_service_core::service::TeamPlanError;

use super::{
    authorize_execution_action_http, authorize_plan_orchestrator_http, emit_plan_changed, epoch_ms,
    kinds_from_csv, plan_err, tag_facets_from_body, text_err, PlanRouteResult,
};

fn push_hints(value: &mut Value, hints: Vec<String>) {
    let hints: Vec<_> = hints
        .into_iter()
        .filter(|hint| !hint.trim().is_empty())
        .collect();
    if !hints.is_empty() {
        value["_hints"] = serde_json::json!(hints);
    }
}

#[derive(Deserialize, Default)]
pub struct CreatePlanBody {
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) project_path: Option<String>,
    #[serde(default)]
    pub(crate) owner: Option<OrchestratorRef>,
    #[serde(default)]
    pub(crate) kinds: Option<String>,
    #[serde(default)]
    pub(crate) from_spec: Option<String>,
    #[serde(default)]
    pub(crate) session_id: Option<String>,
    #[serde(default)]
    pub(crate) session_binding_nonce: Option<String>,
}

fn de_truthy<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    let s = <Option<String> as serde::Deserialize>::deserialize(d)?;
    Ok(s.map(|v| matches!(v.as_str(), "1" | "true" | "yes" | "on")))
}

#[derive(Deserialize, Default)]
pub struct ListPlansQuery {
    pub(crate) view:
        Option<codeterm_service_core::service::plan::ui_projections::PlanViewProjection>,
    #[serde(default, deserialize_with = "de_truthy")]
    pub(crate) chat: Option<bool>,
    pub(crate) tab_id: Option<String>,
    pub(crate) query: Option<String>,
    pub(crate) team: Option<String>,
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
    headers: crate::auth::ActorHeaders,
    Json(body): Json<CreatePlanBody>,
) -> PlanRouteResult {
    let project = body.project_path.unwrap_or_else(|| {
        std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default()
    });
    let owner = crate::auth::actor_ref(&state, &headers, body.owner)
        .map_err(|(status, message)| text_err(status, message))?;
    let kinds = body
        .kinds
        .as_deref()
        .map(kinds_from_csv)
        .unwrap_or_default();
    let now = epoch_ms();
    let from_spec = body.from_spec.filter(|s| !s.trim().is_empty());
    let svc = state.svc.clone();
    let caller = codeterm_service_core::service::TeamPlanCaller {
        tab_id: crate::tab_routes::caller_tab_header(&headers),
        session_id: body.session_id,
        binding_nonce: body.session_binding_nonce,
    };
    let create_from_spec = from_spec.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        svc.create_team_plan_for_caller(
            body.title,
            project,
            owner,
            caller,
            kinds,
            create_from_spec,
            now,
        )
    })
    .await
    .map_err(|error| {
        text_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Team Plan creation worker failed: {error}"),
        )
    })?
    .map_err(|error| match error {
            TeamPlanCreationError::Plan(error) => plan_err(error),
            TeamPlanCreationError::Team(message) => text_err(StatusCode::BAD_REQUEST, message),
            TeamPlanCreationError::SessionBindingPending => (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "error": "session binding is pending for this Team Planner launch; retry after the binding event",
                    "type": "session_binding_pending",
                    "retryable": true,
                })),
            ),
            TeamPlanCreationError::SessionUnregistered => (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({
                    "error": "the Team Planner has no registered agent session; repair the provider integration",
                    "type": "plan_binding_session_unregistered",
                    "retryable": false,
                })),
            ),
        })?;
    let plan = outcome.plan.clone();
    emit_plan_changed(&state, &plan.id);
    let mut response = serde_json::json!({
        "plan_id": plan.id,
        "plan": plan,
        "from_spec": from_spec.unwrap_or_default()
    });
    outcome.apply_to_response(&mut response);
    Ok(Json(response))
}

pub async fn list(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Query(q): Query<ListPlansQuery>,
) -> PlanRouteResult {
    tokio::task::spawn_blocking(move || {
        if q.chat == Some(true) {
            if q.query.is_some()
                && q.project
                    .as_deref()
                    .is_none_or(|project| project.trim().is_empty())
            {
                return Err(text_err(
                    StatusCode::BAD_REQUEST,
                    "Mention search requires a project",
                ));
            }
            if q.query.as_ref().is_some_and(|query| query.len() > 256) {
                return Err(text_err(
                    StatusCode::BAD_REQUEST,
                    "Mention query is too long",
                ));
            }
            let plans = state
                .svc
                .plan_store
                .chat_snapshot_for_tab(
                    q.project.as_deref(),
                    q.team.as_deref(),
                    q.query.as_deref(),
                    q.tab_id.as_deref(),
                )
                .map_err(plan_err)?;
            return Ok(Json(serde_json::json!({ "plans": plans })));
        }
        codeterm_orchestration::validate_active_team_plan_bindings(&state.svc)
            .map_err(team_plan_err)?;
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
            if !state.svc.plans_startup_loaded() {
                return Err(text_err(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "plans are still loading; retry shortly",
                ));
            }
            None
        };
        if let Some(view) = q.view {
            use codeterm_service_core::service::plan::ui_projections::PlanViewProjection;
            if view != PlanViewProjection::Summary || q.full == Some(true) {
                return Err(text_err(
                    StatusCode::BAD_REQUEST,
                    "Plan lists support only the summary projection",
                ));
            }
            let mut plans = state
                .svc
                .plan_store
                .ui_plan_summaries(want.as_deref())
                .map_err(plan_err)?;
            if let Some(status) = q.status {
                plans.retain(|plan| plan["status"] == status);
            }
            if q.unowned == Some(true) {
                plans.retain(|plan| {
                    plan["owner"]["tab_id"].is_null() && plan["owner"]["session_id"].is_null()
                });
            }
            if let Some(owner) = q.orchestrator {
                plans.retain(|plan| {
                    plan["owner"]["tab_id"] == owner || plan["owner"]["session_id"] == owner
                });
            }
            return Ok(Json(serde_json::json!({"plans":plans})));
        }
        Ok(Json(
            plan_list_response(&state.svc.plan_store, want.as_deref(), &q).map_err(plan_err)?,
        ))
    })
    .await
    .map_err(|_| {
        text_err(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "blocking task failed",
        )
    })?
}

pub(crate) fn plan_list_response(
    store: &codeterm_service_core::service::plan::PlanStore,
    project: Option<&str>,
    query: &ListPlansQuery,
) -> Result<Value, PlanError> {
    let status = match query.status.as_deref() {
        Some("active") => Some(PlanStatus::Active),
        Some("archived") => Some(PlanStatus::Archived),
        _ => None,
    };
    let unowned = query.unowned == Some(true);
    let owner = query.orchestrator.as_deref();
    if query.full == Some(true) {
        let plans = store.export_plan_list(project, status, unowned, owner)?;
        Ok(serde_json::json!({"plans":plans}))
    } else {
        let plans = store.plan_list_index(project, status, unowned, owner)?;
        Ok(serde_json::json!({"plans":plans}))
    }
}

#[derive(Deserialize)]
pub struct ResolveRefsBody {
    refs: Vec<ResolveRefRequest>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ResolveRefRequest {
    plan_id: String,
    #[serde(default)]
    target_id: Option<String>,
    target_kind: ResolveTargetKind,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ResolveTargetKind {
    Plan,
    Task,
    Section,
}

#[derive(Debug, Serialize)]
struct ResolveRefsResponse {
    refs: Vec<ResolvedRef>,
}

#[derive(Debug, Serialize)]
struct ResolvedRef {
    plan_id: String,
    target_id: Option<String>,
    target_kind: ResolveTargetKind,
    state: &'static str,
    title: String,
    plan_title: String,
    task_title: Option<String>,
    task_key: Option<String>,
    section_title: Option<String>,
    file_path: Option<String>,
    canonical_project_path: Option<String>,
    status: Option<PlanStatus>,
    owner_tab_id: Option<String>,
    owner_session_id: Option<String>,
}

pub async fn resolve_refs(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Json(body): Json<ResolveRefsBody>,
) -> PlanRouteResult {
    crate::plan_routes::run_blocking(move || {
        let mut grouped = std::collections::BTreeMap::<String, (Vec<String>, Vec<String>)>::new();
        for item in &body.refs {
            let (tasks, sections) = grouped.entry(item.plan_id.clone()).or_default();
            if let Some(id) = &item.target_id {
                match item.target_kind {
                    ResolveTargetKind::Task => tasks.push(id.clone()),
                    ResolveTargetKind::Section => sections.push(id.clone()),
                    ResolveTargetKind::Plan => {}
                }
            }
        }
        let projections = grouped
            .into_iter()
            .map(|(id, (tasks, sections))| {
                let projection = state
                    .svc
                    .plan_store
                    .reference_projection(&id, &tasks, &sections);
                (id, projection)
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let refs = body
            .refs
            .into_iter()
            .map(|item| resolve_ref(projections.get(&item.plan_id), item))
            .collect::<Vec<_>>();
        Ok(Json(
            serde_json::to_value(ResolveRefsResponse { refs }).unwrap_or_default(),
        ))
    })
    .await
}

fn resolve_ref(
    projection: Option<
        &Result<
            codeterm_service_core::service::plan::ui_projections::ReferenceProjection,
            PlanError,
        >,
    >,
    item: ResolveRefRequest,
) -> ResolvedRef {
    let plan = projection.and_then(|value| value.as_ref().ok());
    let missing_state = match projection {
        Some(Err(PlanError::NotFound)) | None => "missing",
        Some(Err(error)) => classify_load_error(error),
        Some(Ok(_)) => "missing",
    };
    let file_path = None;
    let canonical_project_path = plan.map(|p| p.canonical_project_path.clone());
    let status = plan.map(|p| p.status.clone());
    let owner_tab_id = plan.and_then(|p| p.owner.tab_id.clone());
    let owner_session_id = plan.and_then(|p| p.owner.session_id.clone());
    let plan_title = plan
        .as_ref()
        .map(|entry| entry.title.trim())
        .filter(|title| !title.is_empty())
        .unwrap_or(&item.plan_id)
        .to_string();

    let Some(plan) = plan else {
        return ResolvedRef {
            plan_id: item.plan_id,
            target_id: item.target_id,
            target_kind: item.target_kind,
            state: missing_state,
            title: plan_title.clone(),
            plan_title,
            task_title: None,
            task_key: None,
            section_title: None,
            file_path,
            canonical_project_path,
            status,
            owner_tab_id,
            owner_session_id,
        };
    };

    let mut state_name = "resolved";
    if plan.status == PlanStatus::Archived {
        state_name = "archived";
    } else if plan.owner.tab_id.is_none() && plan.owner.session_id.is_none() {
        state_name = "unowned";
    }

    match item.target_kind {
        ResolveTargetKind::Plan => ResolvedRef {
            plan_id: item.plan_id,
            target_id: None,
            target_kind: ResolveTargetKind::Plan,
            state: state_name,
            title: plan_title.clone(),
            plan_title,
            task_title: None,
            task_key: None,
            section_title: None,
            file_path,
            canonical_project_path,
            status,
            owner_tab_id,
            owner_session_id,
        },
        ResolveTargetKind::Task => {
            let task = item
                .target_id
                .as_deref()
                .and_then(|task_id| plan.task(task_id));
            let task_title = task
                .map(|task| task.name.trim())
                .filter(|title| !title.is_empty())
                .map(str::to_string);
            let title = task_title.clone().unwrap_or_else(|| {
                item.target_id
                    .clone()
                    .unwrap_or_else(|| item.plan_id.clone())
            });
            ResolvedRef {
                plan_id: item.plan_id,
                target_id: item.target_id,
                target_kind: ResolveTargetKind::Task,
                state: if task.is_some() { state_name } else { "stale" },
                title,
                plan_title,
                task_title,
                task_key: task.map(|task| task.display_key()),
                section_title: None,
                file_path,
                canonical_project_path,
                status,
                owner_tab_id,
                owner_session_id,
            }
        }
        ResolveTargetKind::Section => {
            let section = item
                .target_id
                .as_deref()
                .and_then(|section_id| find_section(&plan.sections, section_id));
            let section_title = section
                .map(|section| section.title.trim())
                .filter(|title| !title.is_empty())
                .map(str::to_string);
            let title = section_title.clone().unwrap_or_else(|| {
                item.target_id
                    .clone()
                    .unwrap_or_else(|| item.plan_id.clone())
            });
            ResolvedRef {
                plan_id: item.plan_id,
                target_id: item.target_id,
                target_kind: ResolveTargetKind::Section,
                state: if section.is_some() {
                    state_name
                } else {
                    "stale"
                },
                title,
                plan_title,
                task_title: None,
                task_key: None,
                section_title,
                file_path,
                canonical_project_path,
                status,
                owner_tab_id,
                owner_session_id,
            }
        }
    }
}

fn classify_load_error(error: &PlanError) -> &'static str {
    let PlanError::Io(message) = error else {
        return "unreachable";
    };
    let msg = message.to_ascii_lowercase();
    if msg.contains("permission denied") || msg.contains("access is denied") {
        "permission"
    } else {
        "unreachable"
    }
}

fn find_section<'a>(
    sections: &'a [codeterm_service_core::service::plan::ui_projections::ReferenceSectionRead],
    section_id: &str,
) -> Option<&'a codeterm_service_core::service::plan::ui_projections::ReferenceSectionRead> {
    sections
        .iter()
        .find(|section| section.id == section_id || section.handle.as_deref() == Some(section_id))
}

#[cfg(test)]
mod tests;

pub async fn get(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path(id): Path<String>,
) -> PlanRouteResult {
    tokio::task::spawn_blocking(move || {
        codeterm_orchestration::validate_plan_team_binding(&state.svc, &id)
            .map_err(team_plan_err)?;
        let plan = state
            .svc
            .plan_store
            .export_plan_document(&id)
            .map_err(plan_err)?;
        Ok(Json(plan))
    })
    .await
    .map_err(|_| {
        text_err(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "blocking task failed",
        )
    })?
}

pub async fn state(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path(id): Path<String>,
    Query(window): Query<StateWindowRequest>,
) -> PlanRouteResult {
    crate::plan_routes::run_blocking(move || {
        let snapshot = state
            .svc
            .build_plan_state_with_window(&id, epoch_ms(), window)
            .map_err(plan_err)?;
        Ok(Json(serde_json::to_value(snapshot).unwrap_or_default()))
    })
    .await
}

pub async fn next(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path(id): Path<String>,
    Query(window): Query<StateWindowRequest>,
) -> PlanRouteResult {
    crate::plan_routes::run_blocking(move || {
        let snapshot = state
            .svc
            .build_plan_state_with_window(&id, epoch_ms(), window)
            .map_err(plan_err)?;
        Ok(Json(
            serde_json::to_value(snapshot.into_next_actions()).unwrap_or_default(),
        ))
    })
    .await
}

#[derive(Deserialize, Default)]
pub struct ReconcileQuery {
    #[serde(default, deserialize_with = "de_truthy")]
    fix: Option<bool>,
    #[serde(default)]
    caller_tab_id: Option<String>,
}

pub async fn reconcile(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<ReconcileQuery>,
) -> PlanRouteResult {
    crate::plan_routes::run_blocking(move || {
        let caller = crate::auth::caller_claim(&state, &headers, q.caller_tab_id.as_deref())
            .map_err(|(status, message)| text_err(status, message))?;
        let report = state
            .svc
            .reconcile_plan(&id, q.fix == Some(true), caller.as_deref(), epoch_ms())
            .map_err(plan_err)?;
        Ok(Json(serde_json::to_value(report).unwrap_or_default()))
    })
    .await
}

#[derive(Deserialize, Default)]
pub struct UpdatePlanBody {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    planner: Option<serde_json::Value>,
}

pub async fn update(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<UpdatePlanBody>,
) -> PlanRouteResult {
    tokio::task::spawn_blocking(move || {
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
            .update_plan(
                &id,
                body.title.as_deref(),
                status,
                body.mode.as_deref(),
                now,
            )
            .map_err(plan_err)?;
        if let Some(planner_val) = body.planner {
            let planner_ref: Option<PlannerRef> = serde_json::from_value(planner_val)
                .map_err(|e| text_err(StatusCode::UNPROCESSABLE_ENTITY, e))?;
            state
                .svc
                .transition_plan_planner(&id, planner_ref)
                .map_err(|error| text_err(StatusCode::CONFLICT, error))?;
        }
        emit_plan_changed(&state, &id);
        Ok(Json(serde_json::json!({ "ok": true })))
    })
    .await
    .map_err(|_| {
        text_err(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "blocking task failed",
        )
    })?
}

#[derive(Deserialize)]
pub struct ModeBody {
    mode: String,
}

pub async fn mode_set(
    AxumState(state): AxumState<AppState>,
    headers: crate::auth::ActorHeaders,
    Path(id): Path<String>,
    Json(body): Json<ModeBody>,
) -> PlanRouteResult {
    authorize_plan_mutation_http(&state.svc, &headers, &id).await?;
    let mode = serde_json::from_value::<PlanMode>(serde_json::Value::String(body.mode))
        .map_err(|_| plan_err(PlanError::BadMode))?;
    let now = epoch_ms();
    match mode {
        PlanMode::Approved | PlanMode::Planning => {
            authorize_plan_orchestrator_http(
                &state.svc,
                &id,
                crate::tab_routes::authenticated_plan_caller(&headers),
            )
            .await?;
        }
        PlanMode::Execution => {
            authorize_execution_action_http(
                &state.svc,
                &id,
                crate::tab_routes::authenticated_plan_caller(&headers),
            )
            .await?;
        }
        PlanMode::Paused | PlanMode::Complete => {}
    }
    crate::plan_routes::run_blocking(move || {
        match mode {
            PlanMode::Approved | PlanMode::Planning => {
                let planning_state = if mode == PlanMode::Approved {
                    PlanningState::Approved
                } else {
                    PlanningState::StructureReady
                };
                state
                    .svc
                    .plan_store
                    .set_planning_state(&id, planning_state, PlanningAuthor::User, None, now)
                    .map_err(plan_err)?;
            }
            _ => state
                .svc
                .plan_store
                .set_mode(&id, mode, now)
                .map_err(plan_err)?,
        }
        emit_plan_changed(&state, &id);
        Ok(Json(serde_json::json!({ "ok": true })))
    })
    .await
}

pub async fn delete_plan(
    AxumState(state): AxumState<AppState>,
    headers: crate::auth::ActorHeaders,
    Path(id): Path<String>,
) -> PlanRouteResult {
    authorize_plan_mutation_http(&state.svc, &headers, &id).await?;
    crate::plan_routes::run_blocking(move || {
        state
            .svc
            .plan_store
            .delete(&id)
            .map_err(|e| text_err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
        emit_plan_changed(&state, &id);
        Ok(Json(serde_json::json!({ "ok": true })))
    })
    .await
}

mod lifecycle;
use lifecycle::authorize_plan_mutation_http;
pub(super) use lifecycle::team_plan_err;
#[cfg(test)]
use lifecycle::transfer_caller;
pub use lifecycle::{archive, pause, pause_preview, resume, resume_preview, transfer, unarchive};

pub async fn disown(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path(id): Path<String>,
) -> PlanRouteResult {
    crate::plan_routes::run_blocking(move || {
        state
            .svc
            .plan_store
            .disown(&id, epoch_ms())
            .map_err(plan_err)?;
        emit_plan_changed(&state, &id);
        Ok(Json(serde_json::json!({ "ok": true })))
    })
    .await
}

#[derive(Deserialize)]
pub struct ClaimBody {
    owner: OrchestratorRef,
}

pub async fn claim(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<ClaimBody>,
) -> PlanRouteResult {
    crate::plan_routes::run_blocking(move || {
        state
            .svc
            .plan_store
            .claim(&id, body.owner, epoch_ms())
            .map_err(plan_err)?;
        emit_plan_changed(&state, &id);
        Ok(Json(serde_json::json!({ "ok": true })))
    })
    .await
}

#[derive(Deserialize)]
pub struct KindsSetBody {
    kinds: String,
}

pub async fn kinds_set(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<KindsSetBody>,
) -> PlanRouteResult {
    crate::plan_routes::run_blocking(move || {
        state
            .svc
            .plan_store
            .set_kinds(&id, kinds_from_csv(&body.kinds), epoch_ms())
            .map_err(plan_err)?;
        emit_plan_changed(&state, &id);
        Ok(Json(serde_json::json!({ "ok": true })))
    })
    .await
}

#[derive(Deserialize)]
pub struct TagFacetsSetBody {
    facets: Value,
}

pub async fn tag_facets_set(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<TagFacetsSetBody>,
) -> PlanRouteResult {
    crate::plan_routes::run_blocking(move || {
        let facets = tag_facets_from_body(&body.facets)
            .map_err(|message| text_err(StatusCode::BAD_REQUEST, message))?;
        state
            .svc
            .plan_store
            .set_tag_facets(&id, facets, epoch_ms())
            .map_err(plan_err)?;
        emit_plan_changed(&state, &id);
        Ok(Json(serde_json::json!({ "ok": true })))
    })
    .await
}
