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
