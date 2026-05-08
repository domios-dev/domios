use axum::{
    extract::{Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, AppState};
use crate::mem::graph::{Entity, Relation};

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
    #[serde(default)]
    pub include_invalidated: bool,
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
        .graph_related(&p.id, p.rel.as_deref(), p.as_of.as_deref(), p.include_invalidated)
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

pub async fn mem_graph_create(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(b): Json<GraphCreateEntityBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;

    if !b.id.contains('_') {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "Entity id must follow `<type>_<slug>` format (e.g. person_tima, tech_redis). Got `{}`.",
                b.id
            ),
        ));
    }

    let e = Entity {
        id: b.id,
        entity_type: b.entity_type,
        scope: b.scope.unwrap_or_else(|| "global".to_string()),
        properties: b.properties,
    };
    state.svc.mem.graph_create_entity(&e).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct GraphInvalidateBody {
    pub from: String,
    pub rel: String,
    pub to: String,
    #[serde(default)]
    pub ended: Option<String>,
}

pub async fn mem_graph_invalidate(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(b): Json<GraphInvalidateBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let found = state.svc.mem.graph_invalidate(&b.from, &b.rel, &b.to, b.ended.as_deref())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "invalidated": found })))
}

#[derive(Deserialize)]
pub struct GraphTimelineParams {
    pub entity: String,
    #[serde(default)]
    pub include_invalidated: bool,
}

pub async fn mem_graph_timeline(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(p): Query<GraphTimelineParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let relations = state.svc.mem.graph_timeline(&p.entity, p.include_invalidated)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "entity": p.entity, "relations": relations })))
}

#[derive(Deserialize)]
pub struct GraphTraverseParams {
    pub from: String,
    #[serde(default)]
    pub hops: Option<usize>,
    #[serde(default)]
    pub max_nodes: Option<usize>,
    #[serde(default)]
    pub as_of: Option<String>,
    #[serde(default)]
    pub include_invalidated: bool,
}

pub async fn mem_graph_traverse(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(p): Query<GraphTraverseParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let hops = p.hops.unwrap_or(2).min(5);
    let max_nodes = p.max_nodes.unwrap_or(100).min(500);
    let result = state.svc.mem.graph_traverse(&p.from, hops, max_nodes, p.as_of.as_deref(), p.include_invalidated)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!(result)))
}

#[derive(Deserialize)]
pub struct GraphPathParams {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub max_hops: Option<usize>,
    #[serde(default)]
    pub include_invalidated: bool,
}

pub async fn mem_graph_path(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(p): Query<GraphPathParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let max_hops = p.max_hops.unwrap_or(4).min(8);
    let path = state.svc.mem.graph_shortest_path(&p.from, &p.to, max_hops, p.include_invalidated)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "from": p.from, "to": p.to, "path": path })))
}

#[derive(Deserialize)]
pub struct GraphAliasBody {
    pub alias: String,
    pub canonical: String,
}

pub async fn mem_graph_alias(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(b): Json<GraphAliasBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    if !b.canonical.starts_with("person_") && !b.canonical.starts_with("project_") {
        return Err((StatusCode::BAD_REQUEST, "canonical must be person_* or project_*".into()));
    }
    state.svc.mem.graph_alias(&b.alias, &b.canonical)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "ok": true, "alias": b.alias, "canonical": b.canonical })))
}

#[derive(Deserialize)]
pub struct GraphTunnelsParams {
    #[serde(default)]
    pub include_invalidated: bool,
}

pub async fn mem_graph_tunnels(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(p): Query<GraphTunnelsParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let tunnels = state.svc.mem.graph_tunnels(p.include_invalidated)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "tunnels": tunnels })))
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
