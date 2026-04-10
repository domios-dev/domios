//! REST handlers for `/api/mem/*`. Thin shell over `MemManager`.
//! CLI hits these endpoints via the bundled `codeterm` binary.

use axum::{
    extract::{Path, Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use tauri::Emitter;

use crate::api::routes::{check_auth, AppState};
use crate::mem::activity::{now_ms, MemActivityEvent, MemActivitySource};
use crate::mem::{
    dreaming,
    graph::{Entity, Relation},
    layers::WakeUpOptions,
    learnings::{self, LearnKind},
    rerank::RerankConfig,
    search::SearchQuery,
    store::{MemKind, MemScope, MemUpdate, NewMemItem},
    workers::{self, GraphWorkerInfo, WorkerStatus},
};
use crate::service::SpawnAgentParams;

fn emit_activity(state: &AppState, headers: &HeaderMap, op: &str, mem_ids: Vec<String>) {
    let pane_id = headers
        .get("x-codeterm-pane-id")
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    let worker_id = headers
        .get("x-codeterm-worker-id")
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    let kind = if worker_id.is_some() { "worker" } else { "rest" }.to_string();

    let event = MemActivityEvent {
        op: op.to_string(),
        mem_ids,
        source: MemActivitySource {
            kind,
            pane_id,
            worker_id,
            agent_provider: None,
        },
        ts_ms: now_ms(),
    };

    state.svc.activity.record(event.clone());
    let _ = state.svc.app_handle.emit("mem-activity", &event);
}

#[derive(Deserialize)]
pub struct ActivityRecentParams {
    #[serde(default)]
    pub since_ms: Option<u64>,
}

pub async fn mem_activity_recent(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(p): Query<ActivityRecentParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let since = p.since_ms.unwrap_or_else(|| now_ms().saturating_sub(60_000));
    let events = state.svc.activity.recent(since);
    Ok(Json(serde_json::json!({ "events": events })))
}

#[derive(Deserialize)]
pub struct SearchParams {
    pub q: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub area: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub rerank: Option<bool>,
}

pub async fn mem_search(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(p): Query<SearchParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let q = SearchQuery {
        query: p.q,
        kind: p.kind,
        scope: p.scope,
        area: p.area,
        limit: p.limit,
    };
    // Always use RRF hybrid (FTS5 + vector fusion) for best recall.
    // Falls back gracefully when one backend has no results.
    let hits = if p.rerank.unwrap_or(false) {
        let cfg = RerankConfig {
            enabled: true,
            provider: "auto".into(),
            api_key_env: None,
            model: None,
        };
        state.svc.mem.search_with_rerank(&q, &cfg)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?
    } else {
        state.svc.mem.recall(&q)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?
    };
    let ids: Vec<String> = hits.iter().map(|h| h.item.id.clone()).collect();
    emit_activity(&state, &headers, "search", ids);
    Ok(Json(serde_json::json!({ "hits": hits })))
}

pub async fn mem_get(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let item = state.svc.mem.get(&id).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    match item {
        Some(item) => {
            emit_activity(&state, &headers, "read", vec![id.clone()]);
            Ok(Json(serde_json::to_value(item).unwrap_or_default()))
        }
        None => Err((StatusCode::NOT_FOUND, format!("mem item {} not found", id))),
    }
}

#[derive(Deserialize)]
pub struct SaveBody {
    pub kind: String,
    #[serde(default)]
    pub scope: Option<String>,
    pub title: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub area: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub body: String,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub importance: Option<f64>,
    /// If true and a row with the same slug-derived id exists, overwrite it
    /// in place. If false (default), collision falls through to hash suffix.
    #[serde(default)]
    pub replace: bool,
}

pub async fn mem_save(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<SaveBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let kind = MemKind::from_str(&body.kind)
        .ok_or((StatusCode::BAD_REQUEST, format!("unknown kind: {}", body.kind)))?;
    let scope = body.scope.map(|s| MemScope::from_str(&s)).unwrap_or(MemScope::Global);
    // Check BEFORE moving fields — prevents extraction cascade
    let is_auto_extracted = body.tags.iter().any(|t| t == "auto-extracted")
        || body.source.as_deref().map_or(false, |s| s.starts_with("extracted_from:"));
    let item = NewMemItem {
        kind,
        scope,
        title: body.title,
        summary: body.summary,
        area: body.area,
        tags: body.tags,
        body: body.body,
        source: body.source,
        importance: body.importance.unwrap_or(3.0),
    };
    let title_clone = item.title.clone();
    let body_clone = item.body.clone();
    let kind_for_extract = item.kind;
    let id = if body.replace {
        state.svc.mem.save_replace(item)
    } else {
        state.svc.mem.save(item)
    }.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    emit_activity(&state, &headers, "write", vec![id.clone()]);

    // Auto-extraction: spawn a headless graph-worker to extract entities/relations
    // via claude --model haiku. Fire-and-forget, non-blocking.
    // SKIP for items that are themselves auto-extracted (prevents cascade)
    // or if auto-extraction is disabled in config.
    let auto_extract_enabled = crate::config::memory_config().auto_extract;
    if !is_auto_extracted && auto_extract_enabled {
        let svc = state.svc.clone();
        let id_for_extract = id.clone();
        std::thread::spawn(move || {
            crate::mem::auto_extract::spawn_extraction_worker(
                &svc,
                id_for_extract,
                title_clone,
                body_clone,
                kind_for_extract,
            );
        });
    }

    Ok(Json(serde_json::json!({ "id": id })))
}

pub async fn mem_delete(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let removed = state.svc.mem.delete(&id).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    emit_activity(&state, &headers, "delete", vec![id.clone()]);
    Ok(Json(serde_json::json!({ "deleted": removed })))
}

pub async fn mem_forget_area(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(area): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let count = state.svc.mem.delete_by_area(&area)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    emit_activity(&state, &headers, "delete", vec![format!("area:{}", area)]);
    Ok(Json(serde_json::json!({ "deleted": count, "area": area })))
}

#[derive(Deserialize)]
pub struct UpdateBody {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub area: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub importance: Option<f64>,
}

pub async fn mem_update(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<UpdateBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let patch = MemUpdate {
        title: body.title,
        summary: body.summary,
        area: body.area,
        tags: body.tags,
        body: body.body,
        importance: body.importance,
        bump_recurrence: None,
    };
    let updated = state.svc.mem.update(&id, patch)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    if !updated {
        return Err((StatusCode::NOT_FOUND, format!("mem item {} not found", id)));
    }
    emit_activity(&state, &headers, "update", vec![id.clone()]);
    Ok(Json(serde_json::json!({ "updated": id })))
}

#[derive(Deserialize)]
pub struct WakeUpParams {
    #[serde(default)]
    pub budget: Option<usize>,
    #[serde(default)]
    pub l0_only: Option<bool>,
    #[serde(default)]
    pub no_l0: Option<bool>,
}

pub async fn mem_wake_up(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(p): Query<WakeUpParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let mut opts = WakeUpOptions::default();
    if let Some(b) = p.budget {
        opts.l0_budget_tokens = (b / 8).max(50);
        opts.l1_budget_tokens = b - opts.l0_budget_tokens;
    }
    if p.l0_only.unwrap_or(false) {
        opts.include_l1 = false;
    }
    if p.no_l0.unwrap_or(false) {
        opts.include_l0 = false;
    }
    let text = state.svc.mem.wake_up(&opts).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "text": text })))
}

pub async fn mem_stats(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let stats = state.svc.mem.stats().map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::to_value(stats).unwrap_or_default()))
}

#[derive(Deserialize)]
pub struct GraphRelateBody {
    pub from: String,
    pub rel: String,
    pub to: String,
    #[serde(default)]
    pub valid_from: Option<String>,
    #[serde(default)]
    pub valid_to: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub confidence: Option<f64>,
}

pub async fn mem_graph_relate(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(b): Json<GraphRelateBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let r = Relation {
        from_id: b.from,
        relation_type: b.rel,
        to_id: b.to,
        scope: b.scope.unwrap_or_else(|| "global".to_string()),
        valid_from: b.valid_from.unwrap_or_else(|| chrono::Utc::now().to_rfc3339()),
        valid_to: b.valid_to,
        confidence: b.confidence.unwrap_or(1.0),
        properties: serde_json::json!({}),
    };
    state.svc.mem.graph_relate(&r).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct GraphRelatedParams {
    pub id: String,
    #[serde(default)]
    pub rel: Option<String>,
    #[serde(default)]
    pub as_of: Option<String>,
}

pub async fn mem_graph_related(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(p): Query<GraphRelatedParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let rels = state
        .svc
        .mem
        .graph_related(&p.id, p.rel.as_deref(), p.as_of.as_deref())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "relations": rels })))
}

#[derive(Deserialize)]
pub struct GraphCreateEntityBody {
    pub id: String,
    #[serde(rename = "type")]
    pub entity_type: String,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub properties: serde_json::Value,
}

#[derive(Deserialize)]
pub struct LearnBody {
    pub text: String,
    /// "insight" (default) | "error" | "feature"
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

pub async fn mem_learn(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(b): Json<LearnBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let lk = match b.kind.as_deref() {
        Some("error") => LearnKind::Error,
        Some("feature") => LearnKind::Feature,
        _ => LearnKind::Insight,
    };
    let id = learnings::learn(&state.svc.mem, &b.text, lk, b.tags)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "id": id })))
}

pub async fn mem_promote(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    state.svc.mem.promote_learning(&id)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({ "promoted": id })))
}

#[derive(Deserialize)]
pub struct FlushBody {
    pub pane: String,
    #[serde(default)]
    pub title: Option<String>,
}

pub async fn mem_flush(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(b): Json<FlushBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    // Resolve pane id (supports aliases like "general" / "current")
    let pty_id = {
        let mux = state.svc.mux.lock();
        mux.resolve_pane_id(&b.pane)
            .ok_or((StatusCode::NOT_FOUND, format!("pane not found: {}", b.pane)))?
    };
    let pty_id_for_buffer = state.svc.resolve_pty_id(&pty_id)
        .map_err(|e| (StatusCode::NOT_FOUND, e))?;
    let body = {
        let buf = state.svc.output_buf.lock();
        buf.get_full_text(&pty_id_for_buffer).unwrap_or_default()
    };
    let id = learnings::flush_pane(
        &state.svc.mem,
        &b.pane,
        b.title.as_deref().unwrap_or(""),
        body,
    )
    .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({ "id": id })))
}

#[derive(Deserialize)]
pub struct RecallParams {
    pub q: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

pub async fn mem_recall(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(p): Query<RecallParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let q = SearchQuery {
        query: p.q,
        kind: p.kind,
        scope: None,
        area: None,
        limit: p.limit,
    };
    let hits = state.svc.mem.recall(&q).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let ids: Vec<String> = hits.iter().map(|h| h.item.id.clone()).collect();
    emit_activity(&state, &headers, "search", ids);
    Ok(Json(serde_json::json!({ "hits": hits })))
}

pub async fn mem_dream(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let reports = dreaming::run_all(&state.svc.mem)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "phases": reports })))
}

pub async fn mem_dream_report(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let report = dreaming::prepare_rem_data(&state.svc.mem)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "report": report })))
}

pub async fn mem_graph_full(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let dump = state.svc.mem.graph_full()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let nodes: Vec<serde_json::Value> = dump.nodes.iter().map(|n| serde_json::json!({
        "id": n.id,
        "kind": n.kind,
        "title": n.title,
        "summary": n.summary,
        "area": n.area,
        "importance": n.importance,
        "tags": n.tags,
        "has_vector": n.has_vector,
        "has_body": n.has_body,
        "graph_x": n.graph_x,
        "graph_y": n.graph_y,
    })).collect();
    let edges: Vec<serde_json::Value> = dump.edges.iter().map(|e| serde_json::json!({
        "from": e.from,
        "to": e.to,
        "rel": e.rel,
        "confidence": e.confidence,
        "valid_from": e.valid_from,
        "valid_to": e.valid_to,
    })).collect();
    Ok(Json(serde_json::json!({
        "nodes": nodes,
        "edges": edges,
        "stats": {
            "node_count": dump.stats.node_count,
            "edge_count": dump.stats.edge_count,
        }
    })))
}

#[derive(Deserialize)]
pub struct PositionEntry {
    pub id: String,
    pub x: f64,
    pub y: f64,
}

#[derive(Deserialize)]
pub struct PositionsBody {
    pub positions: Vec<PositionEntry>,
}

pub async fn mem_positions(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(b): Json<PositionsBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let entries: Vec<(String, f64, f64)> = b.positions
        .into_iter()
        .map(|p| (p.id, p.x, p.y))
        .collect();
    let ids: Vec<String> = entries.iter().map(|(id, _, _)| id.clone()).collect();
    let updated = state.svc.mem.set_positions(&entries)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    emit_activity(&state, &headers, "update", ids);
    Ok(Json(serde_json::json!({ "updated": updated })))
}

pub async fn mem_graph_create(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(b): Json<GraphCreateEntityBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let e = Entity {
        id: b.id,
        entity_type: b.entity_type,
        scope: b.scope.unwrap_or_else(|| "global".to_string()),
        properties: b.properties,
    };
    state.svc.mem.graph_create_entity(&e).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ── Graph workers ───────────────────────────────────────────────────

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

    let worker_id = workers::new_worker_id();
    let mem_cfg = crate::config::memory_config();
    let agent = b.agent.clone().unwrap_or_else(|| {
        if mem_cfg.extraction_agent.is_empty() {
            crate::config::default_agent_provider()
        } else {
            mem_cfg.extraction_agent.clone()
        }
    });
    let parent = b.parent.clone().unwrap_or_else(|| "user:memory-panel".to_string());
    let task = workers::build_worker_task(&b.intent, &worker_id, &b.task);

    // Ask the provider how to pass the model — no provider-specific branches here
    let model_args: Option<Vec<String>> = {
        let args = state.svc.agent_registry
            .with_provider(&agent, |p| p.model_launch_args(&mem_cfg.extraction_model))
            .unwrap_or_default();
        if args.is_empty() { None } else { Some(args) }
    };

    let params = SpawnAgentParams {
        agent: Some(agent.clone()),
        agent_name: Some(format!("worker:{}", b.intent)),
        args: model_args,
        cwd: None,
        title: Some(format!("worker:{}", b.intent)),
        role: None,
        parent_pane_id: None,
        can_spawn: None,
        task: Some(task),
        workspace_id: None,
        force_tab: Some(true),
        report_target_pane_id: None,
        skip_permissions: Some(true),
    };

    let result = state
        .svc
        .spawn_agent(params)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    // Set worker_id immediately but defer hidden until PTY is ready.
    // Frontend must see the pane to create xterm and spawn PTY.
    {
        let mut mux = state.svc.mux.lock();
        if let Some(pane) = mux.get_pane_mut(&result.pane_id) {
            pane.worker_id = Some(worker_id.clone());
        }
    }

    let color = state.svc.workers.pick_color();
    let mut info = workers::make_info(
        worker_id.clone(),
        result.pane_id.clone(),
        b.intent.clone(),
        parent,
        color,
    );
    info.agent_provider = Some(agent);
    if !mem_cfg.extraction_model.is_empty() {
        info.model = Some(mem_cfg.extraction_model.clone());
    }
    state.svc.workers.register(info.clone());

    let _ = state.svc.app_handle.emit("graph-worker-spawned", &info);

    // Hide pane after PTY is ready (background task)
    let svc = state.svc.clone();
    let pane_id = result.pane_id.clone();
    tokio::spawn(async move {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            if std::time::Instant::now() > deadline { break; }
            let has_pty = svc.mux.lock()
                .get_pane(&pane_id)
                .and_then(|p| p.pty_id.clone())
                .is_some();
            if has_pty {
                let mut mux = svc.mux.lock();
                if let Some(pane) = mux.get_pane_mut(&pane_id) {
                    pane.hidden = true;
                    pane.show_in_tabbar = false;
                }
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    });

    Ok(Json(serde_json::json!({
        "worker_id": worker_id,
        "pane_id": result.pane_id,
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
    // Capture full PTY output before completing (pane will be closed soon)
    let worker_pane_id = state.svc.workers.get(&id).map(|w| w.pane_id.clone());
    let pty_id = worker_pane_id.as_ref().and_then(|pid| {
        state.svc.mux.lock().get_pane(pid).and_then(|p| p.pty_id.clone())
    });

    if let Some(ref pty_id) = pty_id {
        let output = state.svc.output_buf.lock().get_full_text(pty_id).unwrap_or_default();
        if !output.is_empty() {
            match workers::save_worker_log(&id, &output) {
                Ok(path) => {
                    state.svc.workers.set_log_path(&id, path.display().to_string());
                }
                Err(e) => log::warn!("[graph-workers] save log: {}", e),
            }
        }
    }

    let info = state
        .svc
        .workers
        .complete(&id, b.summary)
        .map_err(|e| (StatusCode::NOT_FOUND, e))?;
    let _ = state.svc.app_handle.emit("graph-worker-reported", &info);

    // Auto-close the pane after a short delay to let output flush.
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
) -> Result<(StatusCode, [(axum::http::header::HeaderName, &'static str); 1], String), (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;

    let worker = state.svc.workers.get(&id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("worker not found: {}", id)))?;

    // If still running, return live PTY output via VT100 parser
    if !worker.status.is_terminal() {
        let pty_id = state.svc.mux.lock()
            .get_pane(&worker.pane_id)
            .and_then(|p| p.pty_id.clone());
        if let Some(pty_id) = pty_id {
            let output = state.svc.output_buf.lock()
                .get_full_text(&pty_id)
                .unwrap_or_default();
            let clean = crate::pty::ansi::clean_terminal_output_sized(&output, 500, 120);
            return Ok((StatusCode::OK, [(axum::http::header::CONTENT_TYPE, "text/plain; charset=utf-8")], clean));
        }
    }

    // Done — read from saved log file
    if let Some(ref path) = worker.log_path {
        let content = std::fs::read_to_string(path)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("read log: {}", e)))?;
        return Ok((StatusCode::OK, [(axum::http::header::CONTENT_TYPE, "text/plain; charset=utf-8")], content));
    }

    Err((StatusCode::NOT_FOUND, "no log available".into()))
}
