use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::api::routes::AppState;
use crate::multiplexer::types::AgentRole;
use crate::plan::{OrchestratorRef, TaskStatus};
use crate::service::plan::task_queries::{TaskListFilter, TaskListResult};
use crate::service::plan::{NewTask, PlanError};
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
        PlanError::BadStatus
        | PlanError::BadMode
        | PlanError::BadPriority
        | PlanError::TaskCreationRequiresApproval
        | PlanError::TaskDispatchRequiresApproval
        | PlanError::HeartbeatRequiresExecution
        | PlanError::InProgressRequiresTab
        | PlanError::BlockedRequiresLabel
        | PlanError::ExecutorRoleRequired
        | PlanError::TaskPauseRequiresReason
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
        | PlanError::InvalidExternalRef(_)
        | PlanError::UnauthorizedGateClaim { .. }
        | PlanError::BudgetExhausted { .. }
        | PlanError::GoalVectorRequired => StatusCode::BAD_REQUEST,
        PlanError::OverrideNotAuthorized => StatusCode::FORBIDDEN,
        PlanError::WritePending { .. } => StatusCode::SERVICE_UNAVAILABLE,
        PlanError::ReadbackMismatch { .. }
        | PlanError::TeamAlreadyHasActivePlan { .. }
        | PlanError::PlanAlreadyBoundToTeam { .. }
        | PlanError::PlanSkillAlreadyExists { .. }
        | PlanError::ReviewHoldRequired => StatusCode::CONFLICT,
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
    project: Option<String>,
    active: Option<bool>,
    all: Option<bool>,
    mine: Option<bool>,
    unassigned: Option<bool>,
    plan: Option<String>,
    status: Option<String>,
}

fn list_response(listed: TaskListResult) -> Json<Value> {
    Json(json!(listed))
}

pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<ListTasksQuery>,
) -> TaskRouteResult {
    let caller = header_tab(&headers);
    let mine = q.mine == Some(true);
    let status = q
        .status
        .map(|status| {
            serde_json::from_value::<TaskStatus>(Value::String(status))
                .map_err(|_| plan_err(PlanError::BadStatus))
        })
        .transpose()?;
    let listed = state.svc.plan_store.list_tasks_with_reason(TaskListFilter {
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
    });
    Ok(list_response(listed))
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
    let tab_id = header_tab(&headers).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "X-CodeTerm-Tab-Id is required" })),
        )
    })?;
    let (plan_id, task_id) = state
        .svc
        .authorize_task_execution_action(
            &id,
            &crate::api::tab_routes::authenticated_plan_caller(&headers),
        )
        .map_err(|message| (StatusCode::FORBIDDEN, Json(json!({ "error": message }))))?;
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
    crate::api::events::emit_everywhere(
        &state.svc.app_handle,
        "plan-changed",
        json!({ "plan_id": plan_id }),
    );
    Ok(Json(
        json!({ "ok": true, "plan_id": plan_id, "task_id": task_id }),
    ))
}

pub async fn drop_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> TaskRouteResult {
    let (plan_id, task_id) = state
        .svc
        .authorize_task_execution_action(
            &id,
            &crate::api::tab_routes::authenticated_plan_caller(&headers),
        )
        .map_err(|message| (StatusCode::FORBIDDEN, Json(json!({ "error": message }))))?;
    state
        .svc
        .plan_store
        .set_task_assignment(&task_id, None, None, epoch_ms())
        .map_err(plan_err)?;
    crate::api::events::emit_everywhere(
        &state.svc.app_handle,
        "plan-changed",
        json!({ "plan_id": plan_id }),
    );
    Ok(Json(
        json!({ "ok": true, "plan_id": plan_id, "task_id": task_id }),
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
    #[serde(default)]
    acceptance_criteria: Vec<String>,
    test_plan: Option<String>,
    qa_plan: Option<String>,
    correctness: Option<String>,
    body: Option<String>,
    intent: Option<String>,
    #[serde(default)]
    execution_intent: crate::plan::TaskExecutionIntent,
    assign_tab: Option<String>,
    owner: Option<OrchestratorRef>,
}

pub async fn add(State(state): State<AppState>, Json(body): Json<AddTaskBody>) -> TaskRouteResult {
    let assign_role = body
        .assign_tab
        .as_deref()
        .and_then(|tab| tab_role(&state, tab));
    let has_model = body.model.as_deref().is_some_and(|s| !s.trim().is_empty());
    let has_why = body.why.as_deref().is_some_and(|s| !s.trim().is_empty());
    let has_criteria = body
        .acceptance_criteria
        .iter()
        .any(|s| !s.trim().is_empty());
    let has_test_plan = body
        .test_plan
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty());
    let has_correctness = body
        .correctness
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty());
    let assign_tab = body.assign_tab.clone();
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
                acceptance_criteria: body.acceptance_criteria,
                test_plan: body.test_plan,
                qa_plan: body.qa_plan,
                correctness: body.correctness,
                body: body.body.unwrap_or_default(),
                competition: None,
                intent: body.intent,
                execution_intent: body.execution_intent,
                assigned_tab_id: body.assign_tab,
                ..Default::default()
            },
            epoch_ms(),
        )
        .map_err(plan_err)?;
    if let Some(tab) = assign_tab.as_deref() {
        state
            .svc
            .plan_store
            .set_task_assignment(&task_id, Some(tab), assign_role, epoch_ms())
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
        "key": state.svc.plan_store.task_display_key(&plan_id, &task_id),
        "_hints": crate::service::plan::hints::task_create_hints(
            &plan_id,
            &task_id,
            false,
            has_model,
            has_why,
            has_criteria,
            has_test_plan,
            has_correctness
        )
    })))
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
    crate::api::events::emit_everywhere(&state.svc.app_handle, "canvas-focus", payload.clone());
    Ok(Json(
        json!({ "ok": true, "event": "canvas-focus", "payload": payload }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::plan::task_queries::{TaskListEmptyReason, TaskSummary};

    fn summary() -> TaskSummary {
        TaskSummary {
            plan_id: "pl_one".into(),
            plan_title: "One".into(),
            plan_owner: OrchestratorRef::default(),
            display_key: "ONE".into(),
            task: serde_json::from_value(json!({
                "id": "task_one",
                "name": "One",
                "type": "task",
                "model": null,
                "model_rationale": null,
                "test_plan": null,
                "correctness": null,
                "status": "pending",
                "status_label": null,
                "intent": null,
                "assigned_tab_id": null,
                "peer": null
            }))
            .expect("task fixture must deserialize"),
            section_titles: vec![],
        }
    }

    #[test]
    fn list_response_carries_the_empty_reason_verbatim() {
        let reason = TaskListEmptyReason::AllTasksInactive { hidden: 3 };
        let Json(body) = list_response(TaskListResult {
            tasks: vec![],
            empty_reason: Some(reason.clone()),
        });

        assert_eq!(body["tasks"], json!([]));
        assert_eq!(
            body["empty_reason"],
            serde_json::to_value(&reason).expect("reason must serialize")
        );
    }

    #[test]
    fn list_response_omits_the_empty_reason_key_for_a_non_empty_listing() {
        let Json(body) = list_response(TaskListResult {
            tasks: vec![summary()],
            empty_reason: None,
        });

        assert_eq!(body["tasks"].as_array().map(Vec::len), Some(1));
        assert!(body.get("empty_reason").is_none());
    }

    #[test]
    fn team_active_plan_conflict_is_409_with_plan_error_json() {
        let (status, Json(body)) = plan_err(PlanError::TeamAlreadyHasActivePlan {
            team_id: "team_alpha".into(),
            existing_plan_id: "pl_existing".into(),
            existing_title: "Existing".into(),
        });
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["plan_error"]["type"], "team_already_has_active_plan");
    }

    #[test]
    fn review_hold_required_is_409_with_plan_error_json() {
        let (status, Json(body)) = plan_err(PlanError::ReviewHoldRequired);
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["plan_error"]["type"], "review_hold_required");
    }
}
