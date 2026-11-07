use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::routes::AppState;
use codeterm_service_core::multiplexer::types::AgentRole;
use codeterm_service_core::plan::TaskStatus;
use codeterm_service_core::service::plan::task_queries::{TaskListFilter, TaskListResult};
use codeterm_service_core::service::plan::PlanError;
use codeterm_types::agent::role_kind::builtins::ORCHESTRATOR;

type TaskRouteResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

fn plan_err(e: PlanError) -> (StatusCode, Json<Value>) {
    let status = match e {
        PlanError::NotFound
        | PlanError::ElementNotFound
        | PlanError::TaskNotFound
        | PlanError::GateNotFound
        | PlanError::GroupNotFound
        | PlanError::ReviewContinuationAttemptNotFound { .. } => StatusCode::NOT_FOUND,
        PlanError::Validation(_)
        | PlanError::BadStatus
        | PlanError::BadMode
        | PlanError::BadPriority
        | PlanError::TaskCreationRequiresApproval
        | PlanError::TaskDispatchRequiresApproval
        | PlanError::HeartbeatRequiresExecution
        | PlanError::InProgressRequiresTab
        | PlanError::BlockedRequiresLabel
        | PlanError::ExecutorRoleRequired
        | PlanError::TaskPausedSystemOnly
        | PlanError::PauseRequiresReason
        | PlanError::QaNoteRequired
        | PlanError::QaSlotUnavailable
        | PlanError::QaNotRunning
        | PlanError::GateAlreadyAnswered
        | PlanError::GateRequiresQuestion
        | PlanError::OverrideReasonRequired
        | PlanError::WaitingFocusRequiresOpenGate
        | PlanError::UndeclaredKind { .. }
        | PlanError::OpenSubtasks { .. }
        | PlanError::InvalidSplitParent { .. }
        | PlanError::TransitionDenied { .. }
        | PlanError::DependencyTaskNotFound { .. }
        | PlanError::DependencyTaskReferenced { .. }
        | PlanError::DependencySelfReference
        | PlanError::DependencyCycle
        | PlanError::ReviewWipExceeded { .. }
        | PlanError::ReviewEvidenceRequired
        | PlanError::ReviewRoundExceeded { .. }
        | PlanError::ReviewOracleScopeExpansion
        | PlanError::ReviewOverrideIncomplete
        | PlanError::ReviewNotInReview
        | PlanError::ActivePlanRequiresTeam { .. }
        | PlanError::PlanActivationRequiresUnarchive { .. }
        | PlanError::InvalidPlanSkillSlug
        | PlanError::InvalidPlanText(_)
        | PlanError::SharedReportRejected(_)
        | PlanError::InvalidExecutorName(_)
        | PlanError::InvalidExternalRef(_)
        | PlanError::UnauthorizedGateClaim { .. }
        | PlanError::BudgetExhausted { .. }
        | PlanError::GoalVectorRequired
        | PlanError::WorkFacetViolation(_) => StatusCode::BAD_REQUEST,
        PlanError::OverrideNotAuthorized => StatusCode::FORBIDDEN,
        PlanError::ReadbackMismatch { .. }
        | PlanError::RespawnAttemptNotCurrent { .. }
        | PlanError::TeamAlreadyHasActivePlan { .. }
        | PlanError::PlanAlreadyBoundToTeam { .. }
        | PlanError::PlanSkillAlreadyExists { .. }
        | PlanError::ReviewContinuationDeliveryRefused { .. }
        | PlanError::ReviewHoldRequired
        | PlanError::DispatchInFlight { .. }
        | PlanError::AttemptBindingRestoreRefused { .. } => StatusCode::CONFLICT,
        PlanError::Io(_) | PlanError::Quarantined { .. } => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, Json(json!({ "plan_error": e.to_json() })))
}

fn epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn header_tab(headers: &HeaderMap) -> Option<String> {
    headers
        .get("X-CodeTerm-Tab-Id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

fn tab_role(state: &AppState, tab_id: &str) -> Option<&'static str> {
    state
        .svc
        .mux
        .read()
        .get_tab(tab_id)
        .and_then(|tab| tab.role.map(|role| (role, tab.role_profile.clone())))
        .map(|(role, profile)| match role {
            AgentRole::Manager if profile.as_deref() == Some(ORCHESTRATOR) => "orchestrator",
            AgentRole::Manager => "manager",
            AgentRole::Worker => "worker",
        })
}

fn tab_and_ancestors(state: &AppState, tab_id: &str) -> std::collections::HashSet<String> {
    let mux = state.svc.mux.read();
    let mut out = std::collections::HashSet::new();
    let mut current = Some(tab_id.to_string());
    while let Some(id) = current {
        if !out.insert(id.clone()) {
            break;
        }
        current = mux.get_tab(&id).and_then(|tab| tab.parent_tab_id.clone());
    }
    out
}

#[derive(Debug, Deserialize)]
pub struct ListTasksQuery {
    pub(super) project: Option<String>,
    pub(super) active: Option<bool>,
    pub(super) all: Option<bool>,
    pub(super) mine: Option<bool>,
    pub(super) unassigned: Option<bool>,
    pub(super) plan: Option<String>,
    pub(super) status: Option<String>,
}

fn list_response(listed: TaskListResult) -> Json<Value> {
    Json(json!(listed))
}

pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<ListTasksQuery>,
) -> TaskRouteResult {
    tokio::task::spawn_blocking(move || {
        let caller = header_tab(&headers);
        let mine = q.mine == Some(true);
        let status = q
            .status
            .map(|status| {
                serde_json::from_value::<TaskStatus>(Value::String(status))
                    .map_err(|_| plan_err(PlanError::BadStatus))
            })
            .transpose()?;
        let listed = state
            .svc
            .plan_store
            .list_tasks_with_reason(TaskListFilter {
                project_path: q.project,
                active_only: status.is_none() && (q.all != Some(true) || q.active == Some(true)),
                mine_tabs: if mine {
                    caller
                        .as_deref()
                        .map(|id| tab_and_ancestors(&state, id))
                        .unwrap_or_default()
                } else {
                    Default::default()
                },
                unassigned: q.unassigned == Some(true),
                plan_id: q.plan,
                status,
            })
            .map_err(plan_err)?;
        Ok(list_response(listed))
    })
    .await
    .map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error.to_string()})),
        )
    })?
}

pub async fn show(State(state): State<AppState>, Path(id): Path<String>) -> TaskRouteResult {
    tokio::task::spawn_blocking(move || {
        let task = state
            .svc
            .plan_store
            .show_task_window(&id)
            .map_err(plan_err)?;
        Ok(Json(serde_json::to_value(task).unwrap_or_default()))
    })
    .await
    .map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error.to_string()})),
        )
    })?
}

pub async fn take(
    State(state): State<AppState>,
    headers: crate::auth::ActorHeaders,
    Path(id): Path<String>,
) -> TaskRouteResult {
    let tab_id = header_tab(&headers).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "X-CodeTerm-Tab-Id is required" })),
        )
    })?;
    let (plan_id, task_id) = crate::plan_routes::authorize_task_execution_action_http(
        &state.svc,
        &id,
        crate::tab_routes::authenticated_plan_caller(&headers),
    )
    .await?;

    tokio::task::spawn_blocking(move || {
        state
            .svc
            .plan_store
            .set_task_assignment(
                &task_id,
                Some(&tab_id),
                tab_role(&state, &tab_id),
                epoch_ms(),
            )
            .map_err(plan_err)?;
        let name = state
            .svc
            .plan_store
            .task_name_projection(&task_id)
            .map_err(plan_err)?;
        let description = codeterm_service_core::service::task_tab_description(&name, None);
        state
            .svc
            .assign_task_tab_description(&tab_id, &description)
            .map_err(|message| (StatusCode::BAD_REQUEST, Json(json!({ "error": message }))))?;
        state.svc.emit_committed_plan_changed(&plan_id);
        Ok(Json(
            json!({ "ok": true, "plan_id": plan_id, "task_id": task_id }),
        ))
    })
    .await
    .map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error.to_string()})),
        )
    })?
}

pub async fn drop_task(
    State(state): State<AppState>,
    headers: crate::auth::ActorHeaders,
    Path(id): Path<String>,
) -> TaskRouteResult {
    let (plan_id, task_id) = crate::plan_routes::authorize_task_execution_action_http(
        &state.svc,
        &id,
        crate::tab_routes::authenticated_plan_caller(&headers),
    )
    .await?;

    tokio::task::spawn_blocking(move || {
        state
            .svc
            .plan_store
            .set_task_assignment(&task_id, None, None, epoch_ms())
            .map_err(plan_err)?;
        state.svc.emit_committed_plan_changed(&plan_id);
        Ok(Json(
            json!({ "ok": true, "plan_id": plan_id, "task_id": task_id }),
        ))
    })
    .await
    .map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error.to_string()})),
        )
    })?
}

#[derive(Debug, Deserialize)]
pub struct CanvasFocusBody {
    task_id: Option<String>,
    tab_id: Option<String>,
    plan_id: Option<String>,
}

pub async fn canvas_focus(
    State(state): State<AppState>,
    Json(body): Json<CanvasFocusBody>,
) -> TaskRouteResult {
    let payload = json!({
        "task_id": body.task_id,
        "tab_id": body.tab_id,
        "plan_id": body.plan_id,
    });
    state.svc.ui.emit(
        "canvas-focus",
        serde_json::to_value(payload.clone()).unwrap_or(serde_json::Value::Null),
    );
    Ok(Json(
        json!({ "ok": true, "event": "canvas-focus", "payload": payload }),
    ))
}

#[cfg(test)]
mod tests;
