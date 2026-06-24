use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::api::routes::AppState;
use crate::multiplexer::types::AgentRole;
use crate::plan::OrchestratorRef;
use crate::service::plan::task_queries::TaskListFilter;
use crate::service::plan::{NewTask, PlanError};

type TaskRouteResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

fn plan_err(e: PlanError) -> (StatusCode, Json<Value>) {
    let status = match e {
        PlanError::NotFound | PlanError::TaskNotFound => StatusCode::NOT_FOUND,
        PlanError::BadStatus
        | PlanError::BadPriority
        | PlanError::UndeclaredKind { .. }
        | PlanError::OpenSubtasks { .. }
        | PlanError::TransitionDenied { .. }
        | PlanError::BudgetExhausted { .. } => StatusCode::BAD_REQUEST,
        PlanError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, Json(json!({ "plan_error": e.to_json() })))
}

fn epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn header_pane(headers: &HeaderMap) -> Option<String> {
    headers
        .get("X-CodeTerm-Pane-Id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

fn pane_role(state: &AppState, pane_id: &str) -> Option<&'static str> {
    state
        .svc
        .mux
        .read()
        .get_pane(pane_id)
        .and_then(|pane| pane.role)
        .map(|role| match role {
            AgentRole::Orchestrator => "orchestrator",
            AgentRole::Worker => "worker",
        })
}

fn pane_and_ancestors(state: &AppState, pane_id: &str) -> std::collections::HashSet<String> {
    let mux = state.svc.mux.read();
    let mut out = std::collections::HashSet::new();
    let mut current = Some(pane_id.to_string());
    while let Some(id) = current {
        if !out.insert(id.clone()) {
            break;
        }
        current = mux
            .get_pane(&id)
            .and_then(|pane| pane.parent_pane_id.clone());
    }
    out
}

#[derive(Debug, Deserialize)]
pub struct ListTasksQuery {
    project: Option<String>,
    active: Option<bool>,
    all: Option<bool>,
    mine: Option<bool>,
    unassigned: Option<bool>,
    plan: Option<String>,
}

pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<ListTasksQuery>,
) -> TaskRouteResult {
    let caller = header_pane(&headers);
    let mine = q.mine == Some(true);
    let tasks = state.svc.plan_store.list_tasks(TaskListFilter {
        project_path: q.project,
        active_only: q.all != Some(true) || q.active == Some(true),
        mine_panes: if mine {
            caller
                .as_deref()
                .map(|id| pane_and_ancestors(&state, id))
                .unwrap_or_default()
        } else {
            Default::default()
        },
        unassigned: q.unassigned == Some(true),
        plan_id: q.plan,
    });
    Ok(Json(json!({ "tasks": tasks })))
}

pub async fn show(State(state): State<AppState>, Path(id): Path<String>) -> TaskRouteResult {
    let task = state
        .svc
        .plan_store
        .show_task(&id)
        .ok_or_else(|| plan_err(PlanError::TaskNotFound))?;
    Ok(Json(serde_json::to_value(task).unwrap_or_default()))
}

pub async fn take(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> TaskRouteResult {
    let pane_id = header_pane(&headers).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "X-CodeTerm-Pane-Id is required" })),
        )
    })?;
    let plan_id = state
        .svc
        .plan_store
        .set_task_assignment(&id, Some(&pane_id), pane_role(&state, &pane_id), epoch_ms())
        .map_err(plan_err)?;
    crate::api::events::emit_everywhere(
        &state.svc.app_handle,
        "plan-changed",
        json!({ "plan_id": plan_id }),
    );
    Ok(Json(
        json!({ "ok": true, "plan_id": plan_id, "task_id": id }),
    ))
}

pub async fn drop_task(State(state): State<AppState>, Path(id): Path<String>) -> TaskRouteResult {
    let plan_id = state
        .svc
        .plan_store
        .set_task_assignment(&id, None, None, epoch_ms())
        .map_err(plan_err)?;
    crate::api::events::emit_everywhere(
        &state.svc.app_handle,
        "plan-changed",
        json!({ "plan_id": plan_id }),
    );
    Ok(Json(
        json!({ "ok": true, "plan_id": plan_id, "task_id": id }),
    ))
}

#[derive(Debug, Deserialize)]
pub struct AddTaskBody {
    name: String,
    #[serde(rename = "type")]
    kind: Option<String>,
    project: String,
    model: Option<String>,
    why: Option<String>,
    body: Option<String>,
    intent: Option<String>,
    assign_pane: Option<String>,
    owner: Option<OrchestratorRef>,
}

pub async fn add(State(state): State<AppState>, Json(body): Json<AddTaskBody>) -> TaskRouteResult {
    let assign_role = body
        .assign_pane
        .as_deref()
        .and_then(|pane| pane_role(&state, pane));
    let has_model = body.model.as_deref().is_some_and(|s| !s.trim().is_empty());
    let has_why = body.why.as_deref().is_some_and(|s| !s.trim().is_empty());
    let assign_pane = body.assign_pane.clone();
    let (plan_id, task_id) = state
        .svc
        .plan_store
        .add_inbox_task(
            &body.project,
            body.owner.unwrap_or_default(),
            NewTask {
                name: body.name,
                kind: body.kind.unwrap_or_else(|| "task".into()),
                model: body.model,
                model_rationale: body.why,
                body: body.body.unwrap_or_default(),
                intent: body.intent,
                assigned_pane_id: body.assign_pane,
                ..Default::default()
            },
            epoch_ms(),
        )
        .map_err(plan_err)?;
    if let Some(pane) = assign_pane.as_deref() {
        state
            .svc
            .plan_store
            .set_task_assignment(&task_id, Some(pane), assign_role, epoch_ms())
            .map_err(plan_err)?;
    }
    crate::api::events::emit_everywhere(
        &state.svc.app_handle,
        "plan-changed",
        json!({ "plan_id": plan_id }),
    );
    Ok(Json(json!({
        "plan_id": plan_id.clone(),
        "task_id": task_id.clone(),
        "_hints": crate::service::plan::hints::task_create_hints(
            &plan_id,
            &task_id,
            false,
            has_model,
            has_why
        )
    })))
}

#[derive(Debug, Deserialize)]
pub struct CanvasFocusBody {
    task_id: Option<String>,
    pane_id: Option<String>,
    plan_id: Option<String>,
}

pub async fn canvas_focus(
    State(state): State<AppState>,
    Json(body): Json<CanvasFocusBody>,
) -> TaskRouteResult {
    let payload = json!({
        "task_id": body.task_id,
        "pane_id": body.pane_id,
        "plan_id": body.plan_id,
    });
    crate::api::events::emit_everywhere(&state.svc.app_handle, "canvas-focus", payload.clone());
    Ok(Json(
        json!({ "ok": true, "event": "canvas-focus", "payload": payload }),
    ))
}
