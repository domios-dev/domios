//! Graph layout snapshot/delta/labels/edges + rebuild trigger routes.

use axum::{
    extract::{Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::routes::AppState;
use codeterm_service_core::mem::graph_layout::snapshot::MAX_IDS;

#[derive(Deserialize)]
pub struct SnapshotParams {
    #[serde(default)]
    pub since_version: Option<u64>,
    #[serde(default)]
    pub since_seq: Option<u64>,
}

pub async fn mem_graph_snapshot(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Query(p): Query<SnapshotParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || {
        let mem = &state.svc.mem;

        // Delta path: client knows the version and its last-seen seq.
        if let (Some(since_version), Some(since_seq)) = (p.since_version, p.since_seq) {
            let current = mem.layout_cache().layout_version();
            if since_version == current {
                let delta = mem
                    .layout_delta(since_seq)
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
                return Ok(Json(serde_json::json!(delta)));
            }
            // Version changed → fall through to a full snapshot.
        }

        let snap = mem
            .layout_snapshot()
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
        Ok(Json(serde_json::json!(snap)))
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
}

#[derive(Deserialize)]
pub struct IdsParams {
    #[serde(default)]
    pub ids: String,
}

fn parse_ids(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .take(MAX_IDS)
        .map(String::from)
        .collect()
}

pub async fn mem_graph_labels(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Query(p): Query<IdsParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || {
        let ids = parse_ids(&p.ids);
        let labels = state
            .svc
            .mem
            .layout_labels(&ids)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
        let out: Vec<codeterm_service_core::mem::graph_view::LabelRow> = labels
            .into_iter()
            .map(|(id, title)| codeterm_service_core::mem::graph_view::LabelRow { id, title })
            .collect();
        Ok(Json(serde_json::json!(out)))
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
}

pub async fn mem_graph_edges(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Query(p): Query<IdsParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || {
        let ids = parse_ids(&p.ids);
        let edges = state
            .svc
            .mem
            .layout_edges(&ids)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
        Ok(Json(serde_json::json!(edges)))
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
}

pub async fn mem_graph_layout_rebuild(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    if state.svc.mem.layout_is_rebuilding() {
        return Err((
            StatusCode::CONFLICT,
            "layout rebuild already running".into(),
        ));
    }
    let mem = state.svc.mem.clone();
    codeterm_service_core::async_runtime::spawn_blocking(move || match mem.run_full_layout() {
        Ok(v) => log::info!("[graph_layout] rebuild done v{v}"),
        Err(e) => log::warn!("[graph_layout] rebuild failed: {e}"),
    });
    Ok(Json(serde_json::json!({ "started": true })))
}

#[cfg(test)]
mod tests {
    use super::parse_ids;

    #[test]
    fn parse_ids_caps_at_max() {
        let raw = (0..600)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let ids = parse_ids(&raw);
        assert_eq!(ids.len(), super::MAX_IDS);
    }
}
