use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, AppState};
use crate::api_support::optional_json::OptionalJson;

pub async fn list_workspaces(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, message)| (status, message.to_string()))?;
    let result =
        crate::dispatch::dispatch(&crate::dispatch::Command::WorkspaceList, state.svc.as_ref())
            .map_err(|error| {
                if error == crate::startup::STORAGE_UNAVAILABLE_MESSAGE {
                    (
                        StatusCode::SERVICE_UNAVAILABLE,
                        crate::startup::STORAGE_UNAVAILABLE_MESSAGE.to_string(),
                    )
                } else {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Failed to list workspaces".to_string(),
                    )
                }
            })?;
    Ok(Json(result.into_json()))
}

#[derive(Deserialize)]
pub(crate) struct CreateWorkspaceBody {
    name: String,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    activate: Option<bool>,
    #[serde(default)]
    provision: bool,
}

pub(crate) type CreateTabBody = crate::api::tab_routes::CreateShellTabBody;

pub async fn create_workspace(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    OptionalJson(body): OptionalJson<CreateWorkspaceBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, message)| (status, message.to_string()))?;
    let body = body.ok_or((StatusCode::BAD_REQUEST, "name is required".to_string()))?;
    let cmd = crate::dispatch::Command::WorkspaceCreate {
        name: body.name,
        cwd: body.cwd,
        activate: body.activate,
        provision: body.provision,
    };
    let svc = state.svc.clone();
    let result = tokio::task::spawn_blocking(move || crate::dispatch::dispatch(&cmd, svc.as_ref()))
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .map_err(|error| {
            let status = if error == crate::dispatch::WORKSPACE_PROVISION_CWD_CONFLICT
                || error.starts_with("workspace name")
            {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (status, format!("Failed to create workspace: {error}"))
        })?;
    Ok(Json(result.into_json()))
}

pub async fn focus_workspace(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, message)| (status, message.to_string()))?;
    let svc = state.svc.clone();
    tokio::task::spawn_blocking(move || svc.focus_workspace(&id))
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .map_err(|error| (StatusCode::NOT_FOUND, error))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
pub(crate) struct SetWorkspaceCwdBody {
    cwd: String,
}

pub async fn set_workspace_cwd(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<SetWorkspaceCwdBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, message)| (status, message.to_string()))?;
    let svc = state.svc.clone();
    let cmd = crate::dispatch::Command::WorkspaceSetCwd {
        workspace_id: id,
        cwd: body.cwd,
    };
    let result = tokio::task::spawn_blocking(move || crate::dispatch::dispatch(&cmd, svc.as_ref()))
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .map_err(|error| (StatusCode::UNPROCESSABLE_ENTITY, error))?;
    Ok(Json(result.into_json()))
}

#[derive(Deserialize)]
pub(crate) struct RenameWorkspaceBody {
    name: String,
}

fn map_rename_error(e: String) -> (StatusCode, String) {
    if e.contains("not found") {
        (StatusCode::NOT_FOUND, e)
    } else if e.starts_with("workspace name") {
        (StatusCode::BAD_REQUEST, e)
    } else {
        (StatusCode::INTERNAL_SERVER_ERROR, e)
    }
}

#[derive(Deserialize, Default)]
pub(crate) struct CloseWorkspaceBody {
    #[serde(default)]
    force: bool,
    #[serde(default)]
    confirm: Option<String>,
}

fn has_human_workspace_confirmation(body: &CloseWorkspaceBody, workspace_id: &str) -> bool {
    body.force && body.confirm.as_deref() == Some(workspace_id)
}

#[derive(Debug, PartialEq, Eq)]
enum WorkspaceCloseCaller {
    AgentTab(String),
    HumanConfirmedHeaderless,
    Unattributed,
}

fn workspace_close_caller(
    headers: &HeaderMap,
    body: &CloseWorkspaceBody,
    workspace_id: &str,
) -> WorkspaceCloseCaller {
    if let Some(tab_id) = crate::api::tab_routes::caller_tab_header(headers)
        .filter(|tab_id| !tab_id.trim().is_empty())
    {
        WorkspaceCloseCaller::AgentTab(tab_id)
    } else if has_human_workspace_confirmation(body, workspace_id) {
        WorkspaceCloseCaller::HumanConfirmedHeaderless
    } else {
        WorkspaceCloseCaller::Unattributed
    }
}

pub async fn rename_workspace(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<RenameWorkspaceBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let cmd = crate::dispatch::Command::WorkspaceRename {
        workspace_id: id,
        name: body.name,
    };
    let result = crate::dispatch::dispatch(&cmd, state.svc.as_ref()).map_err(map_rename_error)?;
    Ok(Json(result.into_json()))
}

pub async fn close_workspace(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    OptionalJson(body): OptionalJson<CloseWorkspaceBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let body = body.unwrap_or_default();
    let result = match workspace_close_caller(&headers, &body, &id) {
        WorkspaceCloseCaller::AgentTab(caller_tab_id) => {
            state
                .svc
                .close_workspace_guarded_async(Some(&caller_tab_id), &id, body.force)
                .await
        }
        WorkspaceCloseCaller::HumanConfirmedHeaderless => {
            state
                .svc
                .close_workspace_as_confirmed_headerless_human(&id, body.force)
                .await
        }
        WorkspaceCloseCaller::Unattributed => {
            state
                .svc
                .close_workspace_guarded_async(None, &id, body.force)
                .await
        }
    };
    result.map_err(crate::api::tab_routes::map_close_error)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn list_recently_closed(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let cmd = crate::dispatch::Command::WorkspaceRecentlyClosed { workspace_id: id };
    let result = crate::dispatch::dispatch(&cmd, state.svc.as_ref()).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to list recently closed",
        )
    })?;
    Ok(Json(result.into_json()))
}

#[derive(Deserialize, Default)]
pub(crate) struct ReopenClosedBody {
    #[serde(default)]
    index: Option<usize>,
}

pub async fn reopen_closed_tab(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    OptionalJson(body): OptionalJson<ReopenClosedBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let index = body.and_then(|b| b.index);
    let cmd = crate::dispatch::Command::WorkspaceReopenClosedTab {
        workspace_id: id,
        index,
    };
    let result = crate::dispatch::dispatch(&cmd, state.svc.as_ref())
        .map_err(|_| (StatusCode::NOT_FOUND, "No recently closed Tab"))?;
    Ok(Json(result.into_json()))
}

pub async fn reopen_closed_group(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, error)| (status, error.to_string()))?;
    let cmd = crate::dispatch::Command::WorkspaceReopenClosedGroup { group_id };
    let service = state.svc.clone();
    let result =
        tokio::task::spawn_blocking(move || crate::dispatch::dispatch(&cmd, service.as_ref()))
            .await
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
            .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    Ok(Json(result.into_json()))
}

#[derive(Deserialize)]
pub(crate) struct MoveTabBody {
    to_workspace_id: String,
}

pub async fn move_tab(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((workspace_id, tab_id)): Path<(String, String)>,
    Json(body): Json<MoveTabBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, message)| (status, message.to_string()))?;
    state
        .svc
        .move_tab(&tab_id, &workspace_id, &body.to_workspace_id)
        .map_err(|message| {
            let status = if message.starts_with("Workspace not found:") {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::BAD_REQUEST
            };
            (status, message)
        })?;
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
pub(crate) struct ReorderTabsBody {
    ordered_tab_ids: Vec<String>,
}

pub async fn reorder_tabs(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(workspace_id): Path<String>,
    Json(body): Json<ReorderTabsBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    state
        .svc
        .reorder_tab(&workspace_id, &body.ordered_tab_ids)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Failed to reorder tabs"))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn reorder_children(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(parent_tab_id): Path<String>,
    Json(body): Json<ReorderTabsBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    state
        .svc
        .reorder_child(&parent_tab_id, &body.ordered_tab_ids)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Failed to reorder children"))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn reorder_display(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(workspace_id): Path<String>,
    Json(body): Json<ReorderTabsBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    state
        .svc
        .reorder_display(&workspace_id, &body.ordered_tab_ids)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Failed to reorder display"))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
pub(crate) struct ReorderWorkspacesBody {
    ordered_workspace_ids: Vec<String>,
}

pub async fn reorder_workspaces(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<ReorderWorkspacesBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    state
        .svc
        .reorder_workspace(&body.ordered_workspace_ids)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Failed to reorder workspaces"))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
pub(crate) struct RenameTabBody {
    title: String,
}

pub async fn rename_tab(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((_workspace_id, tab_id)): Path<(String, String)>,
    Json(body): Json<RenameTabBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    crate::service::tab::validate_tab_title(&body.title)
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    state
        .svc
        .rename_tab(&tab_id, &body.title)
        .map_err(map_rename_error)?;
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn create_tab(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(workspace_id): Path<String>,
    OptionalJson(body): OptionalJson<CreateTabBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let mut body = body.unwrap_or_default();
    body.workspace_id = Some(workspace_id);
    let response =
        crate::api::tab_routes::create_shell_tab(AxumState(state.clone()), headers, Json(body))
            .await?;
    let mut value = response.0;
    let tab_id = value["tab_id"].as_str().map(str::to_owned);
    if let Some(tab_id) = tab_id {
        value["title"] =
            serde_json::Value::String(state.svc.mux.read().display_label_for_tab(&tab_id));
    }
    Ok(Json(value))
}

#[cfg(test)]
mod close_tests {
    use super::{workspace_close_caller, CloseWorkspaceBody, WorkspaceCloseCaller};
    use axum::http::HeaderMap;

    #[test]
    fn workspace_close_route_reads_force_from_body() {
        let no_headers = HeaderMap::new();
        assert_eq!(
            workspace_close_caller(&no_headers, &CloseWorkspaceBody::default(), "workspace-1"),
            WorkspaceCloseCaller::Unattributed,
        );
        assert_eq!(
            workspace_close_caller(
                &no_headers,
                &CloseWorkspaceBody {
                    force: false,
                    confirm: Some("workspace-1".into()),
                },
                "workspace-1",
            ),
            WorkspaceCloseCaller::Unattributed,
        );
        assert_eq!(
            workspace_close_caller(
                &no_headers,
                &CloseWorkspaceBody {
                    force: true,
                    confirm: Some("workspace-1".into()),
                },
                "workspace-1",
            ),
            WorkspaceCloseCaller::HumanConfirmedHeaderless,
        );
        assert_eq!(
            workspace_close_caller(
                &no_headers,
                &CloseWorkspaceBody {
                    force: true,
                    confirm: Some("workspace-other".into()),
                },
                "workspace-1",
            ),
            WorkspaceCloseCaller::Unattributed,
        );
        let mut agent_headers = HeaderMap::new();
        agent_headers.insert("X-CodeTerm-Tab-Id", "agent-1".parse().unwrap());
        assert_eq!(
            workspace_close_caller(
                &agent_headers,
                &CloseWorkspaceBody {
                    force: true,
                    confirm: Some("workspace-1".into()),
                },
                "workspace-1",
            ),
            WorkspaceCloseCaller::AgentTab("agent-1".into()),
        );
    }
}
