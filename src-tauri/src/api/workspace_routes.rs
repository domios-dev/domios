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
    let result = crate::dispatch::dispatch(&crate::dispatch::Command::WorkspaceList, &state.svc)
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
    let result = crate::dispatch::dispatch(&cmd, &state.svc).map_err(|_| {
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

pub async fn rename_workspace(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<RenameWorkspaceBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let cmd = crate::dispatch::Command::WorkspaceRename {
        workspace_id: id,
        name: body.name,
    };
    let result = crate::dispatch::dispatch(&cmd, &state.svc)
        .map_err(|_| (StatusCode::NOT_FOUND, "Workspace not found"))?;
    Ok(Json(result.into_json()))
}

pub async fn close_workspace(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let cmd = crate::dispatch::Command::WorkspaceClose { workspace_id: id };
    let result = crate::dispatch::dispatch(&cmd, &state.svc)
        .map_err(|_| (StatusCode::NOT_FOUND, "Workspace not found"))?;
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
pub(crate) struct RenameTabBody {
    title: String,
}

pub async fn rename_tab(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((workspace_id, tab_id)): Path<(String, String)>,
    Json(body): Json<RenameTabBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    state
        .svc
        .rename_tab(&workspace_id, &tab_id, &body.title)
        .map_err(|_| (StatusCode::NOT_FOUND, "Tab not found"))?;
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
    let _ = state
        .svc
        .set_pane_role(&pane_id, "Orchestrator", None, Some(true));
    let eager = body.map(|Json(b)| b.eager).unwrap_or(false);
    let pty_id = if eager {
        state
            .svc
            .ensure_pane_pty_for_send(&pane_id)
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to spawn PTY"))?;
        state
            .svc
            .mux
            .lock()
            .get_pane(&pane_id)
            .and_then(|p| p.pty_id.clone())
    } else {
        None
    };
    Ok(Json(serde_json::json!({
        "tab_id": tab_id,
        "pane_id": pane_id,
        "pty_id": pty_id
    })))
}
