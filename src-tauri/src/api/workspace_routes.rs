use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, AppState};

pub async fn list_workspaces(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let result =
        crate::dispatch::dispatch(&crate::dispatch::Command::WorkspaceList, state.svc.as_ref())
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to list workspaces",
                )
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
}

#[derive(Deserialize, Default)]
pub(crate) struct CreateTabBody {
    #[serde(default)]
    eager: bool,
}

pub async fn create_workspace(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateWorkspaceBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let cmd = crate::dispatch::Command::WorkspaceCreate {
        name: body.name,
        cwd: body.cwd,
        activate: body.activate,
    };
    let result = crate::dispatch::dispatch(&cmd, state.svc.as_ref()).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to create workspace",
        )
    })?;
    Ok(Json(result.into_json()))
}

pub async fn focus_workspace(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    state
        .svc
        .focus_workspace(&id)
        .map_err(|_| (StatusCode::NOT_FOUND, "Workspace not found"))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
pub(crate) struct RenameWorkspaceBody {
    name: String,
}

fn map_rename_error(e: String) -> (StatusCode, String) {
    if e.contains("not found") {
        (StatusCode::NOT_FOUND, e)
    } else {
        (StatusCode::INTERNAL_SERVER_ERROR, e)
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
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let caller_pane_id = crate::api::pane_routes::caller_pane_header(&headers);
    let cmd = crate::dispatch::Command::WorkspaceClose {
        workspace_id: id,
        caller_pane_id,
    };
    let result = crate::dispatch::dispatch(&cmd, state.svc.as_ref())
        .map_err(crate::api::pane_routes::map_close_error)?;
    Ok(Json(result.into_json()))
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

pub async fn reopen_closed_pane(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Option<Json<ReopenClosedBody>>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let index = body.and_then(|Json(b)| b.index);
    let cmd = crate::dispatch::Command::WorkspaceReopenClosedPane {
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
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let cmd = crate::dispatch::Command::WorkspaceReopenClosedGroup { group_id };
    let result = crate::dispatch::dispatch(&cmd, state.svc.as_ref())
        .map_err(|_| (StatusCode::NOT_FOUND, "No closed group"))?;
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
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    state
        .svc
        .move_tab(&tab_id, &workspace_id, &body.to_workspace_id)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Failed to move tab"))?;
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
    Path(parent_pane_id): Path<String>,
    Json(body): Json<ReorderTabsBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    state
        .svc
        .reorder_child(&parent_pane_id, &body.ordered_tab_ids)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Failed to reorder children"))?;
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
    Path((workspace_id, tab_id)): Path<(String, String)>,
    Json(body): Json<RenameTabBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    state
        .svc
        .rename_tab(&workspace_id, &tab_id, &body.title)
        .map_err(map_rename_error)?;
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn create_tab(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(workspace_id): Path<String>,
    body: Option<Json<CreateTabBody>>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let (tab_id, pane_id) = state
        .svc
        .create_tab_with_activate(&workspace_id, false)
        .map_err(|_| (StatusCode::NOT_FOUND, "Workspace not found"))?;
    let eager = body.map(|Json(b)| b.eager).unwrap_or(false);
    let pty_id = if eager {
        state
            .svc
            .ensure_pane_pty_for_send(&pane_id)
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to spawn PTY"))?;
        state
            .svc
            .mux
            .write()
            .get_pane(&pane_id)
            .and_then(|p| p.pty_id.clone())
    } else {
        None
    };
    let title = state.svc.mux.read().display_label_for_pane(&pane_id);
    Ok(Json(serde_json::json!({
        "id": tab_id.clone(),
        "title": title,
        "pane_id": pane_id,
        "tab_id": tab_id,
        "pty_id": pty_id
    })))
}
