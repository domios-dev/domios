use axum::{
    extract::{Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};
use std::process::Stdio;

use crate::api::routes::{check_auth, AppState};

#[derive(Deserialize)]
pub struct GraphQueryParams {
    pub workspace: String,
    #[serde(default)]
    pub node: Option<i64>,
    #[serde(default = "default_k_hops")]
    pub k_hops: u32,
    #[serde(default = "default_node_cap")]
    pub node_cap: usize,
}

fn default_k_hops() -> u32 { 1 }
fn default_node_cap() -> usize { 5000 }

pub async fn graph(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(params): Query<GraphQueryParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let workspaces = state.svc.list_workspaces();
    let ws = workspaces
        .iter()
        .find(|w| w.id == params.workspace)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("workspace {} not found", params.workspace)))?;
    if ws.name == "General" {
        return Err((StatusCode::BAD_REQUEST, "General workspace has no project map".into()));
    }
    let cwd = ws
        .cwd
        .clone()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "workspace has no cwd".into()))?;

    let cli_path =
        crate::mem::supervisor::Supervisor::resolve_cli_path(&crate::paths::data_dir());

    let svc = state.svc.clone();
    let body = tokio::task::spawn_blocking(move || run_export(&cli_path, &cwd, &params))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("join error: {e}")))?
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    let _ = svc;
    Ok(Json(body))
}

fn run_export(
    cli_path: &std::path::Path,
    workspace_root: &str,
    params: &GraphQueryParams,
) -> Result<serde_json::Value, String> {
    let mut cmd = std::process::Command::new(cli_path);
    cmd.arg("--json")
        .arg("map")
        .arg("export")
        .arg("--workspace-root")
        .arg(workspace_root)
        .arg("--node-cap")
        .arg(params.node_cap.to_string());
    if let Some(node) = params.node {
        cmd.arg("--node").arg(node.to_string());
        cmd.arg("--k-hops").arg(params.k_hops.to_string());
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null());

    let output = cmd.output().map_err(|e| format!("spawn cli: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "cli exit {}: {}",
            output.status.code().unwrap_or(-1),
            stderr.trim()
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(stdout.trim()).map_err(|e| format!("parse cli output: {e}"))
}

#[derive(Deserialize)]
pub struct ShowParams {
    pub workspace_id: String,
}

#[derive(Serialize)]
pub struct ShowResponse {
    pub pane_id: String,
}

pub async fn show(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<ShowParams>,
) -> Result<Json<ShowResponse>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let workspaces = state.svc.list_workspaces();
    let ws = workspaces
        .iter()
        .find(|w| w.id == body.workspace_id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("workspace {} not found", body.workspace_id)))?;
    if ws.name == "General" {
        return Err((StatusCode::BAD_REQUEST, "General workspace has no project map".into()));
    }
    if ws.cwd.is_none() {
        return Err((StatusCode::BAD_REQUEST, "workspace has no cwd".into()));
    }
    let pane_id = state
        .svc
        .open_project_map(&body.workspace_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(ShowResponse { pane_id }))
}

#[derive(Deserialize)]
pub struct WorkspaceQuery {
    pub workspace: String,
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct DispatchState {
    #[serde(default)]
    pub last_dispatch_pane: Option<String>,
    #[serde(default)]
    pub last_message_template: Option<String>,
}

fn workspace_cwd(state: &AppState, workspace_id: &str) -> Result<std::path::PathBuf, (StatusCode, String)> {
    let workspaces = state.svc.list_workspaces();
    let ws = workspaces
        .iter()
        .find(|w| w.id == workspace_id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("workspace {} not found", workspace_id)))?;
    let cwd = ws
        .cwd
        .clone()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "workspace has no cwd".into()))?;
    Ok(std::path::PathBuf::from(cwd))
}

fn state_path(workspace_cwd: &std::path::Path) -> std::path::PathBuf {
    workspace_cwd.join(".codeterm").join("map").join("v1").join("state.json")
}

fn read_state(workspace_cwd: &std::path::Path) -> DispatchState {
    let path = state_path(workspace_cwd);
    let Ok(text) = std::fs::read_to_string(&path) else { return DispatchState::default(); };
    serde_json::from_str(&text).unwrap_or_default()
}

fn write_state(workspace_cwd: &std::path::Path, st: &DispatchState) -> std::io::Result<()> {
    let path = state_path(workspace_cwd);
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    let json = serde_json::to_string_pretty(st).expect("serialize DispatchState");
    std::fs::write(&path, json)
}

pub async fn get_state(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(q): Query<WorkspaceQuery>,
) -> Result<Json<DispatchState>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let cwd = workspace_cwd(&state, &q.workspace)?;
    Ok(Json(read_state(&cwd)))
}

pub async fn patch_state(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(q): Query<WorkspaceQuery>,
    Json(body): Json<DispatchState>,
) -> Result<Json<DispatchState>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let cwd = workspace_cwd(&state, &q.workspace)?;
    let mut st = read_state(&cwd);
    if body.last_dispatch_pane.is_some() {
        st.last_dispatch_pane = body.last_dispatch_pane;
    }
    if body.last_message_template.is_some() {
        st.last_message_template = body.last_message_template;
    }
    write_state(&cwd, &st).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("write state: {e}")))?;
    Ok(Json(st))
}

pub async fn dispatchable(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(q): Query<WorkspaceQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let panes = state.svc.dispatchable_panes_in_workspace(&q.workspace);
    Ok(Json(serde_json::to_value(panes).unwrap_or(serde_json::json!([]))))
}
