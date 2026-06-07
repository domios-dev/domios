use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use tauri::Emitter;

use crate::api::routes::{check_auth, AppState};
use crate::mem::workers::{self, GraphWorkerInfo, WorkerStatus};

pub async fn workers_list(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<GraphWorkerInfo>>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    Ok(Json(state.svc.workers.list()))
}

pub async fn workers_get(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<GraphWorkerInfo>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    state
        .svc
        .workers
        .get(&id)
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, format!("worker {} not found", id)))
}

#[derive(Deserialize)]
pub struct WorkerSpawnBody {
    pub intent: String,
    pub task: String,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub parent: Option<String>,
}

pub async fn workers_spawn(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(b): Json<WorkerSpawnBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;

    let mem_cfg = crate::config::memory_config();
    let agent = b.agent.clone().unwrap_or_else(|| {
        if mem_cfg.extraction_agent.is_empty() {
            crate::config::default_agent_provider()
        } else {
            mem_cfg.extraction_agent.clone()
        }
    });
    let parent = b
        .parent
        .clone()
        .unwrap_or_else(|| "user:memory-panel".to_string());

    let raw_model = if mem_cfg.extraction_model.is_empty() {
        mem_cfg.worker_model.clone()
    } else {
        mem_cfg.extraction_model.clone()
    };
    let model = crate::agent::models::canonicalize_short(&raw_model).to_string();

    let svc = state.svc.clone();
    let intent = b.intent.clone();
    let task = b.task.clone();
    let worker_id = tokio::task::spawn_blocking(move || {
        workers::spawn_headless(&svc, intent, task, parent, agent, model)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("join: {}", e)))?
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    let info = state.svc.workers.get(&worker_id);
    Ok(Json(serde_json::json!({
        "worker_id": worker_id,
        "pane_id": info.as_ref().map(|i| &i.pane_id),
        "info": info,
    })))
}

#[derive(Deserialize)]
pub struct WorkerProgressBody {
    pub text: String,
    #[serde(default)]
    pub status: Option<String>,
}

pub async fn workers_progress(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<WorkerProgressBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    state
        .svc
        .workers
        .update_progress(&id, b.text.clone())
        .map_err(|e| (StatusCode::NOT_FOUND, e))?;
    if let Some(s) = b.status.as_deref().and_then(WorkerStatus::from_str) {
        state
            .svc
            .workers
            .update_status(&id, s)
            .map_err(|e| (StatusCode::NOT_FOUND, e))?;
    }
    if let Some(info) = state.svc.workers.get(&id) {
        let _ = state.svc.app_handle.emit("graph-worker-progress", &info);
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct WorkerReportBody {
    pub summary: String,
}

pub async fn workers_report(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<WorkerReportBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let worker_pane_id = state.svc.workers.get(&id).map(|w| w.pane_id.clone());
    let pty_id = worker_pane_id.as_ref().and_then(|pid| {
        state
            .svc
            .mux
            .lock()
            .get_pane(pid)
            .and_then(|p| p.pty_id.clone())
    });

    if let Some(ref pty_id) = pty_id {
        let output = state
            .svc
            .output_buf
            .lock()
            .get_full_text(pty_id)
            .unwrap_or_default();
        if !output.is_empty() {
            match workers::save_worker_log(&id, &output) {
                Ok(path) => {
                    state
                        .svc
                        .workers
                        .set_log_path(&id, path.display().to_string());
                }
                Err(e) => log::warn!("[graph-workers] save log: {}", e),
            }
        }
        // Disarm the run-watcher so the post-report 30s of PTY silence (claude can take that long to clean up after the report tool returned,
        // especially on haiku) does not trip the idle reaper and overwrite the just-set "done" status with a Failed.
        state.svc.run_watcher.mark_completed_externally(pty_id);
    }

    let info = state
        .svc
        .workers
        .complete(&id, b.summary)
        .map_err(|e| (StatusCode::NOT_FOUND, e))?;
    let _ = state.svc.app_handle.emit("graph-worker-reported", &info);

    let svc = state.svc.clone();
    let pane_id = info.pane_id.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        if let Err(e) = svc.close_pane(&pane_id) {
            log::warn!("[graph-workers] auto-close pane {}: {}", pane_id, e);
        }
    });

    Ok(Json(serde_json::json!({ "ok": true, "info": info })))
}

pub async fn workers_kill(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let info = state
        .svc
        .workers
        .kill(&id)
        .map_err(|e| (StatusCode::NOT_FOUND, e))?;
    if let Err(e) = state.svc.close_pane(&info.pane_id) {
        log::warn!("[graph-workers] kill pane {}: {}", info.pane_id, e);
    }
    let _ = state.svc.app_handle.emit("graph-worker-done", &info);
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn workers_log(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<
    (
        StatusCode,
        [(axum::http::header::HeaderName, &'static str); 1],
        String,
    ),
    (StatusCode, String),
> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;

    let worker = state
        .svc
        .workers
        .get(&id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("worker not found: {}", id)))?;

    if !worker.status.is_terminal() {
        let pty_id = state
            .svc
            .mux
            .lock()
            .get_pane(&worker.pane_id)
            .and_then(|p| p.pty_id.clone());
        if let Some(pty_id) = pty_id {
            let output = state
                .svc
                .output_buf
                .lock()
                .get_full_text(&pty_id)
                .unwrap_or_default();
            let clean = crate::pty::ansi::clean_terminal_output_sized(&output, 500, 120);
            return Ok((
                StatusCode::OK,
                [(
                    axum::http::header::CONTENT_TYPE,
                    "text/plain; charset=utf-8",
                )],
                clean,
            ));
        }
    }

    if let Some(ref path) = worker.log_path {
        let content = std::fs::read_to_string(path).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("read log: {}", e),
            )
        })?;
        return Ok((
            StatusCode::OK,
            [(
                axum::http::header::CONTENT_TYPE,
                "text/plain; charset=utf-8",
            )],
            content,
        ));
    }

    Err((StatusCode::NOT_FOUND, "no log available".into()))
}
