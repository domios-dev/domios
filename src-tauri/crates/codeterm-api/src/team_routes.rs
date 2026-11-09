use axum::{
    extract::{Path, State as AxumState},
    http::StatusCode,
    Json,
};
use codeterm_orchestration::TeamExt;
use serde::Deserialize;

use crate::agent_routes::SpawnRejection;
use crate::routes::AppState;
use codeterm_service_core::service::spawn_params::SpawnTargetRefusal;

#[derive(Deserialize)]
pub struct RenameTeamBody {
    name: String,
}

#[derive(Deserialize)]
pub struct AddTeamMemberBody {
    provider: String,
    role: String,
    title: Option<String>,
    #[serde(alias = "brief")]
    task: Option<String>,
    model: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct CloseTeamBody {
    #[serde(default)]
    force: bool,
    #[serde(default)]
    confirm: Option<String>,
    /// Close even while Team work is in progress (distinct from `force`, which
    /// only confirms a headerless human caller).
    #[serde(default)]
    force_busy: bool,
}

fn validate_name(name: &str) -> Result<(), (StatusCode, String)> {
    if name.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Team name must not be empty".into(),
        ));
    }
    Ok(())
}

fn map_rename_error(error: String) -> (StatusCode, String) {
    if error.contains("not found") {
        (StatusCode::NOT_FOUND, error)
    } else if error.contains("not authorized")
        || error.contains("not a member")
        || error.contains("Unattributed caller")
        || error.contains("cannot close Team")
    {
        (StatusCode::FORBIDDEN, error)
    } else {
        (StatusCode::BAD_REQUEST, error)
    }
}

fn map_close_team_error(error: String) -> (StatusCode, String) {
    if error.contains("remote Team preflight failed")
        || error.contains("unresolved remote Worker transfer")
    {
        return (StatusCode::CONFLICT, error);
    }
    let inventory = codeterm_service_core::service::close_inventory_error_data(&error);
    let status = if inventory
        .as_ref()
        .is_some_and(|data| data["type"] == "close_inventory_timeout")
    {
        StatusCode::GATEWAY_TIMEOUT
    } else if inventory.as_ref().is_some_and(|data| {
        data["type"] == "close_inventory_busy" || data["type"] == "close_scope_busy"
    }) || error
        .starts_with(codeterm_service_core::service::TEAM_CLOSE_BUSY_REFUSAL_PREFIX)
    {
        StatusCode::CONFLICT
    } else if error.starts_with(codeterm_service_core::service::TEAM_CLOSE_REFUSAL_PREFIX) {
        StatusCode::FORBIDDEN
    } else if error.starts_with("Team ") && error.contains(" not found") {
        StatusCode::NOT_FOUND
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    (status, error)
}

fn require_caller_tab_id(
    headers: &crate::auth::ActorHeaders,
) -> Result<String, (StatusCode, String)> {
    crate::tab_routes::caller_tab_header(headers).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "X-CodeTerm-Tab-Id is required".into(),
        )
    })
}

fn rename_team_adapter(
    headers: &crate::auth::ActorHeaders,
    team_id: String,
    body: RenameTeamBody,
    rename: impl FnOnce(String, String, String) -> Result<codeterm_types::team::Team, String>,
) -> Result<codeterm_types::team::Team, (StatusCode, String)> {
    validate_name(&body.name)?;
    let caller_tab_id = require_caller_tab_id(headers)?;
    rename(team_id, caller_tab_id, body.name).map_err(map_rename_error)
}

pub async fn rename_team(
    AxumState(state): AxumState<AppState>,
    headers: crate::auth::ActorHeaders,
    Path(team_id): Path<String>,
    Json(body): Json<RenameTeamBody>,
) -> Result<Json<codeterm_types::team::Team>, (StatusCode, String)> {
    let team = rename_team_adapter(&headers, team_id, body, |team_id, caller_tab_id, name| {
        codeterm_commands::commands::mux_commands::mux_rename_team_service_core(
            &state.svc,
            team_id,
            name,
            Some(caller_tab_id),
        )
    })?;
    Ok(Json(team))
}

async fn close_team_adapter<T>(
    headers: &crate::auth::ActorHeaders,
    team_id: String,
    body: CloseTeamBody,
    close: impl AsyncFnOnce(
        String,
        codeterm_service_core::service::TeamMutationCaller,
    ) -> Result<T, String>,
) -> Result<T, (StatusCode, String)> {
    let caller = close_team_caller(headers, &team_id, &body);
    close(team_id, caller).await.map_err(map_close_team_error)
}

fn close_team_caller(
    headers: &crate::auth::ActorHeaders,
    team_id: &str,
    body: &CloseTeamBody,
) -> codeterm_service_core::service::TeamMutationCaller {
    crate::tab_routes::caller_tab_header(headers)
        .filter(|tab_id| !tab_id.trim().is_empty())
        .map(codeterm_service_core::service::TeamMutationCaller::AgentTab)
        .unwrap_or_else(|| {
            if body.force && body.confirm.as_deref() == Some(team_id) {
                codeterm_service_core::service::TeamMutationCaller::HumanConfirmedHeaderless
            } else {
                codeterm_service_core::service::TeamMutationCaller::Unattributed
            }
        })
}

pub async fn close_team(
    AxumState(state): AxumState<AppState>,
    headers: crate::auth::ActorHeaders,
    Path(team_id): Path<String>,
    codeterm_service_core::api_support::OptionalJson(body): codeterm_service_core::api_support::OptionalJson<
        CloseTeamBody,
    >,
) -> Result<Json<codeterm_orchestration::TeamCloseResult>, (StatusCode, String)> {
    let service = state.svc.clone();
    let body = body.unwrap_or_default();
    let force_busy = body.force_busy;
    let team =
        close_team_adapter(
            &headers,
            team_id,
            body,
            |team_id: String, caller| async move {
                service.close_team(&team_id, caller, force_busy).await
            },
        )
        .await?;
    Ok(Json(team))
}

/// The caller tab is optional here alone: a Team whose Orchestrator is gone has
/// no membership left to spend, and staffing authorizes the caller-less add.
async fn add_team_member_adapter<T>(
    headers: &crate::auth::ActorHeaders,
    team_id: String,
    body: AddTeamMemberBody,
    add_member: impl AsyncFnOnce(
        String,
        Option<String>,
        AddTeamMemberBody,
    ) -> Result<T, SpawnTargetRefusal>,
) -> Result<T, SpawnRejection> {
    let caller_tab_id =
        crate::tab_routes::caller_tab_header(headers).filter(|tab_id| !tab_id.trim().is_empty());
    add_member(team_id, caller_tab_id, body)
        .await
        .map_err(|error| {
            if error.data().is_some() {
                return SpawnRejection::from(error);
            }
            let (status, message) = map_rename_error(error.to_string());
            SpawnRejection::new(status, "team_member_spawn_failed", message)
        })
}

pub async fn add_team_member(
    AxumState(state): AxumState<AppState>,
    headers: crate::auth::ActorHeaders,
    Path(team_id): Path<String>,
    Json(body): Json<AddTeamMemberBody>,
) -> Result<Json<codeterm_orchestration::AddTeamMemberResult>, SpawnRejection> {
    let result = add_team_member_adapter(
        &headers,
        team_id,
        body,
        |team_id: String, caller_tab_id: Option<String>, body: AddTeamMemberBody| async move {
            codeterm_orchestration::team::add_team_member_checked(
                &state.svc,
                &team_id,
                caller_tab_id.as_deref(),
                codeterm_orchestration::AddTeamMemberParams {
                    provider: body.provider,
                    role: body.role,
                    title: body.title,
                    task: body.task,
                    model: body.model,
                },
            )
            .await
        },
    )
    .await?;
    Ok(Json(result))
}

#[cfg(test)]
mod tests;
