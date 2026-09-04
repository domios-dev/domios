use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, AppState};
use crate::mem::workers::{self, GraphWorkerInfo, WorkerStatus};
use crate::multiplexer::types::TabCloseReason;

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
    let parent = b
        .parent
        .clone()
        .unwrap_or_else(|| "user:memory-panel".to_string());
    let resolved = codeterm_mem::worker_agent::resolve_worker(
        b.agent.as_deref(),
        &mem_cfg.extraction_agent,
        &mem_cfg.extraction_model,
        &crate::config::default_agent_provider(),
        &mem_cfg.worker_model,
        &crate::service::spawn::CatalogBackgroundModelPort,
    );

    let svc = state.svc.clone();
    let intent = b.intent.clone();
    let task = b.task.clone();
    let worker_id = tokio::task::spawn_blocking(move || {
        workers::spawn_headless(&svc, intent, task, parent, resolved.agent, resolved.model)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("join: {}", e)))?
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    let info = state.svc.workers.get(&worker_id);
    Ok(Json(serde_json::json!({
        "worker_id": worker_id,
        "tab_id": info.as_ref().map(|i| &i.tab_id),
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
        crate::api::events::emit_everywhere(&state.svc.app_handle, "graph-worker-progress", &info);
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
    let worker_tab_id = state.svc.workers.get(&id).map(|w| w.tab_id.clone());
    let (pty_id, session_id, run_cwd) = worker_tab_id
        .as_ref()
        .map(|pid| {
            let mux = state.svc.mux.write();
            let tab = mux.get_tab(pid);
            (
                tab.and_then(|p| p.pty_id.clone()),
                tab.and_then(|p| p.agent_session_id.clone()),
                tab.and_then(|p| p.cwd.clone()),
            )
        })
        .unwrap_or((None, None, None));
    if let Some(ref sid) = session_id {
        state.svc.workers.set_run_session(&id, sid.clone(), run_cwd);
    }

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
        // especially on haiku) does not trip the idle reaper and overwrite the just-set "done" status with a Failed.
        state.svc.run_watcher.mark_completed_externally(pty_id);
    }

    let info = state
        .svc
        .workers
        .complete(&id, b.summary)
        .map_err(|e| (StatusCode::NOT_FOUND, e))?;
    crate::api::events::emit_everywhere(&state.svc.app_handle, "graph-worker-reported", &info);

    let svc = state.svc.clone();
    let tab_id = info.tab_id.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        if let Err(e) = svc.close_tab_with_reason(&tab_id, TabCloseReason::AgentRunEnded) {
            log::warn!("[graph-workers] auto-close Tab {}: {}", tab_id, e);
        }
    });

    Ok(Json(serde_json::json!({ "ok": true, "info": info })))
}

/// Terminate one Worker by exact id: mark it terminal, close its tab, announce
/// it. Shared by the kill route and extraction revocation so both cancel a
/// Worker the same way — and only ever the Worker named.
pub fn cancel_worker(
    svc: &std::sync::Arc<crate::service::ServiceContext>,
    worker_id: &str,
) -> Option<codeterm_mem::workers::GraphWorkerInfo> {
    let info = match svc.workers.kill(worker_id) {
        Ok(info) => info,
        Err(e) => {
            log::warn!("[graph-workers] cancel {}: {}", worker_id, e);
            return None;
        }
    };
    if !info.tab_id.is_empty() {
        if let Err(e) = svc.close_tab_with_reason(&info.tab_id, TabCloseReason::Cancelled) {
            log::warn!("[graph-workers] cancel Tab {}: {}", info.tab_id, e);
        }
    }
    crate::api::events::emit_everywhere(&svc.app_handle, "graph-worker-done", &info);
    Some(info)
}

pub async fn workers_kill(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    cancel_worker(&state.svc, &id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("worker not found: {}", id)))?;
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
            .write()
            .get_tab(&worker.tab_id)
            .and_then(|p| p.pty_id.clone());
        if let Some(pty_id) = pty_id {
            let output = state
                .svc
                .output_buf
                .lock()
                .get_full_text(&pty_id)
                .unwrap_or_default();
            let clean = codeterm_util::terminal::clean_terminal_output_sized(&output, 500, 120);
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
