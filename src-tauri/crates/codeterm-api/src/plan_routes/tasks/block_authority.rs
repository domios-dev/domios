use axum::{http::HeaderMap, Json};
use serde_json::Value;

use codeterm_service_core::service::ServiceState;

use super::{epoch_ms, plan_authority_err, plan_err, text_err};

/// REST twin of the socket gate's recovery classification: only these verbs may
/// be widened for the live Plan owner. `task_rm` is deliberately excluded.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum TaskMutationKind {
    Recovery,
    Ordinary,
    BlockDecision,
}

fn owner_session_is_gone(
    svc: &ServiceState,
    owner_tab_id: Option<&str>,
    owner_session_id: Option<&str>,
) -> bool {
    let (Some(tab_id), Some(session_id)) = (owner_tab_id, owner_session_id) else {
        return true;
    };
    !svc.mux
        .read()
        .get_tab(tab_id)
        .is_some_and(|tab| tab.agent_session_id.as_deref() == Some(session_id))
}

// A never-dispatched task has no Worker owner to prove, so only the Plan
// Orchestrator, its Planner, or a confirmed human may mutate it.
async fn require_plan_role_for_unowned_task(
    svc: &ServiceState,
    headers: &HeaderMap,
    plan_id: &str,
    caller: &codeterm_service_core::service::AuthenticatedPlanCaller,
) -> Result<(), (axum::http::StatusCode, Json<Value>)> {
    let confirmed_human = headers
        .get("X-CodeTerm-Plan-Confirmation")
        .and_then(|value| value.to_str().ok())
        == Some(plan_id);
    if caller.tab_id.as_deref().is_none_or(str::is_empty) && !confirmed_human {
        return Err(text_err(
            axum::http::StatusCode::FORBIDDEN,
            "task mutation requires a Plan role or explicit human confirmation",
        ));
    }
    if !confirmed_human
        && !matches!(
            svc.classify_planning_caller_async(plan_id, caller).await,
            Ok(codeterm_service_core::plan::PlanningAuthor::User
                | codeterm_service_core::plan::PlanningAuthor::Planner { .. })
        )
    {
        return Err(text_err(
            axum::http::StatusCode::FORBIDDEN,
            "the caller is not the Plan Orchestrator or Planner",
        ));
    }
    Ok(())
}

pub(super) async fn verify_task_mutation_owner(
    svc: &ServiceState,
    headers: &HeaderMap,
    plan_id: &str,
    task_id: &str,
    recovery: TaskMutationKind,
    requested_status_label: Option<&str>,
) -> Result<(), (axum::http::StatusCode, Json<Value>)> {
    let detail = svc
        .plan_store
        .show_task(task_id)
        .filter(|detail| detail.plan_id == plan_id)
        .ok_or_else(|| plan_err(codeterm_service_core::service::plan::PlanError::TaskNotFound))?;
    let caller = crate::tab_routes::authenticated_plan_caller(headers);
    if recovery == TaskMutationKind::BlockDecision {
        let owner = match svc
            .authorize_plan_orchestrator_async(plan_id, &caller)
            .await
        {
            Ok(()) => true,
            Err(codeterm_service_core::service::PlanAuthorityError::SessionBindingPending) => {
                return Err(plan_authority_err(
                    codeterm_service_core::service::PlanAuthorityError::SessionBindingPending,
                ));
            }
            Err(_) => false,
        };
        let planner = matches!(
            svc.classify_planning_caller_async(plan_id, &caller).await,
            Ok(codeterm_service_core::plan::PlanningAuthor::Planner { .. })
        );
        if !owner && !planner {
            return Err(text_err(
                axum::http::StatusCode::FORBIDDEN,
                "only the Plan Orchestrator or Planner may explicitly block a task",
            ));
        }
        let owner_gone = owner_session_is_gone(
            svc,
            detail.task.interruption.owner_tab_id.as_deref(),
            detail.task.interruption.owner_session_id.as_deref(),
        );
        if planner {
            if owner_gone && detail.task.interruption.owner_epoch > 0 {
                svc.plan_store
                    .retire_stale_task_owner_with_label(
                        plan_id,
                        task_id,
                        Some(codeterm_service_core::plan::TaskStatus::Blocked),
                        requested_status_label,
                        |_| false,
                        epoch_ms(),
                    )
                    .map_err(|error| {
                        text_err(axum::http::StatusCode::CONFLICT, error.to_string())
                    })?;
            }
            return Ok(());
        }
        if owner && !owner_gone {
            return Ok(());
        }
    }
    if detail.task.interruption.owner_epoch == 0 {
        return require_plan_role_for_unowned_task(svc, headers, plan_id, &caller).await;
    }
    if matches!(
        recovery,
        TaskMutationKind::Recovery | TaskMutationKind::BlockDecision
    ) && caller
        .tab_id
        .as_deref()
        .is_some_and(|tab_id| !tab_id.is_empty())
        && owner_session_is_gone(
            svc,
            detail.task.interruption.owner_tab_id.as_deref(),
            detail.task.interruption.owner_session_id.as_deref(),
        )
    {
        match svc
            .authorize_plan_orchestrator_async(plan_id, &caller)
            .await
        {
            Ok(()) => {
                return svc
                    .plan_store
                    .retire_stale_task_owner_with_label(
                        plan_id,
                        task_id,
                        (recovery == TaskMutationKind::BlockDecision)
                            .then_some(codeterm_service_core::plan::TaskStatus::Blocked),
                        (recovery == TaskMutationKind::BlockDecision)
                            .then_some(requested_status_label)
                            .flatten(),
                        |_| false,
                        epoch_ms(),
                    )
                    .map(|_| ())
                    .map_err(|error| {
                        text_err(axum::http::StatusCode::CONFLICT, error.to_string())
                    });
            }
            Err(codeterm_service_core::service::PlanAuthorityError::SessionBindingPending) => {
                return Err(plan_authority_err(
                    codeterm_service_core::service::PlanAuthorityError::SessionBindingPending,
                ));
            }
            Err(_) => {}
        }
    }
    let tab_id = headers
        .get("X-CodeTerm-Tab-Id")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            text_err(
                axum::http::StatusCode::UNAUTHORIZED,
                "missing Worker tab id",
            )
        })?;
    let session_id = headers
        .get("X-CodeTerm-Session-Id")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            text_err(
                axum::http::StatusCode::UNAUTHORIZED,
                "missing Worker session id",
            )
        })?;
    let nonce = headers
        .get("X-CodeTerm-Session-Binding-Nonce")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            text_err(
                axum::http::StatusCode::UNAUTHORIZED,
                "missing session binding nonce",
            )
        })?;
    let caller_epoch = headers
        .get("X-CodeTerm-Owner-Epoch")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| text_err(axum::http::StatusCode::CONFLICT, "missing task owner epoch"))?;
    if detail.task.interruption.owner_epoch != caller_epoch {
        return Err(text_err(
            axum::http::StatusCode::CONFLICT,
            "task mutation owner epoch is stale",
        ));
    }
    let owner = svc.clone();
    let target = tab_id.to_owned();
    let session = session_id.to_owned();
    let binding = nonce.to_owned();
    let binding_matches = tokio::task::spawn_blocking(move || {
        let svc = owner;
        let tab_id = target.as_str();
        let session_id = session.as_str();
        let nonce = binding.as_str();
        svc.mux.read().get_tab(tab_id).is_some_and(|tab| {
            tab.agent_session_id.as_deref() == Some(session_id)
                && tab.session_binding_nonce.as_deref() == Some(nonce)
                && tab.reported_session_binding_nonce.as_deref() == Some(nonce)
        })
    })
    .await
    .map_err(|_| {
        text_err(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "blocking task failed",
        )
    })?;
    if !binding_matches {
        return Err(text_err(
            axum::http::StatusCode::CONFLICT,
            "task mutation caller binding is stale or rebound",
        ));
    }
    match svc
        .authorize_plan_orchestrator_async(plan_id, &caller)
        .await
    {
        Ok(()) => return Ok(()),
        Err(codeterm_service_core::service::PlanAuthorityError::SessionBindingPending) => {
            return Err(plan_authority_err(
                codeterm_service_core::service::PlanAuthorityError::SessionBindingPending,
            ));
        }
        Err(_) => {}
    }
    let owner = svc.clone();
    let task = task_id.to_owned();
    let target = tab_id.to_owned();
    let session = session_id.to_owned();
    tokio::task::spawn_blocking(move || {
        let svc = owner;
        let task_id = task.as_str();
        let tab_id = target.as_str();
        let session_id = session.as_str();
        svc.verify_task_owner_session(task_id, tab_id, session_id, caller_epoch)
            .map_err(|error| text_err(axum::http::StatusCode::CONFLICT, error))
    })
    .await
    .map_err(|_| {
        text_err(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "blocking task failed",
        )
    })?
}
