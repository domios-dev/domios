use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use codeterm_orchestration::TeamExt;
use serde::Deserialize;

use crate::routes::{check_auth, AppState};

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

fn require_caller_tab_id(headers: &HeaderMap) -> Result<String, (StatusCode, String)> {
    crate::tab_routes::caller_tab_header(headers).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "X-CodeTerm-Tab-Id is required".into(),
        )
    })
}

fn rename_team_adapter(
    headers: &HeaderMap,
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
    headers: HeaderMap,
    Path(team_id): Path<String>,
    Json(body): Json<RenameTeamBody>,
) -> Result<Json<codeterm_types::team::Team>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, message)| (status, message.to_string()))?;
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
    headers: &HeaderMap,
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
    headers: &HeaderMap,
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
    headers: HeaderMap,
    Path(team_id): Path<String>,
    codeterm_service_core::api_support::OptionalJson(body): codeterm_service_core::api_support::OptionalJson<
        CloseTeamBody,
    >,
) -> Result<Json<codeterm_orchestration::TeamCloseResult>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, message)| (status, message.to_string()))?;
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
    headers: &HeaderMap,
    team_id: String,
    body: AddTeamMemberBody,
    add_member: impl AsyncFnOnce(String, Option<String>, AddTeamMemberBody) -> Result<T, String>,
) -> Result<T, (StatusCode, String)> {
    let caller_tab_id =
        crate::tab_routes::caller_tab_header(headers).filter(|tab_id| !tab_id.trim().is_empty());
    add_member(team_id, caller_tab_id, body)
        .await
        .map_err(map_rename_error)
}

pub async fn add_team_member(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(team_id): Path<String>,
    Json(body): Json<AddTeamMemberBody>,
) -> Result<Json<codeterm_orchestration::AddTeamMemberResult>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, message)| (status, message.to_string()))?;
    let result = add_team_member_adapter(
        &headers,
        team_id,
        body,
        |team_id, caller_tab_id, body: AddTeamMemberBody| async {
            codeterm_commands::commands::mux_commands::mux_add_team_member_service_core(
                &state.svc,
                team_id,
                body.provider,
                body.role,
                caller_tab_id,
                body.title,
                body.task,
                body.model,
            )
            .await
        },
    )
    .await?;
    Ok(Json(result))
}

#[cfg(test)]
mod tests;
