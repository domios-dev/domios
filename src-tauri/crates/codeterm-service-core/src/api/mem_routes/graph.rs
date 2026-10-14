use axum::{
    extract::{Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};

use crate::api::routes::{check_auth, AppState};
use crate::mem_ops::graph::*;
use crate::mem::graph::{Entity, Relation};

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
        valid_from: b
            .valid_from
            .unwrap_or_else(|| chrono::Utc::now().to_rfc3339()),
        valid_to: b.valid_to,
        confidence: b.confidence.unwrap_or(1.0),
        properties: serde_json::json!({}),
        reason: b.reason,
    };
    crate::mem::store::with_write_worker(super::worker_id_from_headers(&headers), || {
        state.svc.mem.graph_relate(&r)
    })
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "ok": true })))
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
        .graph_related(
            &p.id,
            p.rel.as_deref(),
            p.as_of.as_deref(),
            p.include_invalidated,
        )
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "relations": rels })))
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
    crate::mem::store::with_write_worker(super::worker_id_from_headers(&headers), || {
        state.svc.mem.graph_create_entity(&e)
    })
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn mem_graph_invalidate(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(b): Json<GraphInvalidateBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let found = state
        .svc
        .mem
        .graph_invalidate(&b.from, &b.rel, &b.to, b.ended.as_deref())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "invalidated": found })))
}

pub async fn mem_graph_timeline(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(p): Query<GraphTimelineParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let relations = state
        .svc
        .mem
        .graph_timeline(&p.entity, p.include_invalidated)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(
        serde_json::json!({ "entity": p.entity, "relations": relations }),
    ))
}

pub async fn mem_graph_traverse(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(p): Query<GraphTraverseParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let hops = p.hops.unwrap_or(2).min(5);
    let max_nodes = p.max_nodes.unwrap_or(100).min(500);
    let result = state
        .svc
        .mem
        .graph_traverse(
            &p.from,
            hops,
            max_nodes,
            p.as_of.as_deref(),
            p.include_invalidated,
        )
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!(result)))
}

pub async fn mem_graph_path(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(p): Query<GraphPathParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let max_hops = p.max_hops.unwrap_or(4).min(8);
    let path = state
        .svc
        .mem
        .graph_shortest_path(&p.from, &p.to, max_hops, p.include_invalidated)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(
        serde_json::json!({ "from": p.from, "to": p.to, "path": path }),
    ))
}

pub async fn mem_graph_alias(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(b): Json<GraphAliasBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    validate_canonical(&b.canonical).map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    state
        .svc
        .mem
        .graph_alias(&b.alias, &b.canonical)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(
        serde_json::json!({ "ok": true, "alias": b.alias, "canonical": b.canonical }),
    ))
}

pub async fn mem_graph_tunnels(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(p): Query<GraphTunnelsParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let tunnels = state
        .svc
        .mem
        .graph_tunnels(p.include_invalidated)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "tunnels": tunnels })))
}

pub async fn mem_graph_full(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let dump = state
        .svc
        .mem
        .graph_full()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let nodes: Vec<serde_json::Value> = dump
        .nodes
        .iter()
        .map(|n| {
            serde_json::json!({
                "id": n.id,
                "kind": n.kind,
                "title": n.title,
                "summary": n.summary,
                "area": n.area,
                "importance": n.importance,
                "tags": n.tags,
                "has_vector": n.has_vector,
                "has_body": n.has_body,
            })
        })
        .collect();
    let edges: Vec<serde_json::Value> = dump
        .edges
        .iter()
        .map(|e| {
            serde_json::json!({
                "from": e.from,
                "to": e.to,
                "rel": e.rel,
                "confidence": e.confidence,
                "valid_from": e.valid_from,
                "valid_to": e.valid_to,
            })
        })
        .collect();
    Ok(Json(serde_json::json!({
        "nodes": nodes,
        "edges": edges,
        "stats": {
            "node_count": dump.stats.node_count,
            "edge_count": dump.stats.edge_count,
        }
    })))
}

// ─── Repair endpoints ────────────────────────────────────────────────


pub async fn mem_graph_repair_find_relations(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(b): Json<RepairFindRelationsBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let groups = state
        .svc
        .mem
        .graph_find_duplicate_relations(&b.group_by, b.limit)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "groups": groups })))
}

pub async fn mem_graph_repair_delete_relations(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(b): Json<RepairDeleteRelationsBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let deleted = state
        .svc
        .mem
        .graph_delete_relations(&b.rowids, &b.actor)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "deleted": deleted })))
}

pub async fn mem_graph_repair_merge_relations(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(b): Json<RepairMergeRelationsBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let report = state
        .svc
        .mem
        .graph_merge_relations(b.keep, &b.merge, &b.actor)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "report": report })))
}

pub async fn mem_graph_repair_find_entities(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(b): Json<RepairFindEntitiesBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let groups = state
        .svc
        .mem
        .graph_find_duplicate_entities(b.strategy, b.limit)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "groups": groups })))
}

pub async fn mem_graph_repair_augment_entity(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(b): Json<RepairAugmentEntityBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    state
        .svc
        .mem
        .graph_augment_entity(&b.id, b.patch, &b.actor)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn mem_graph_repair_delete_entity(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(b): Json<RepairDeleteEntityBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let report = state
        .svc
        .mem
        .graph_delete_entity(&b.id, b.force, b.purge_history, &b.actor)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "report": report })))
}

pub async fn mem_graph_repair_orphan_vectors(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(b): Json<RepairOrphanVectorsBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let report = state
        .svc
        .mem
        .graph_repair_orphan_vectors(b.dry_run, &b.actor)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "report": report })))
}

pub async fn mem_graph_prune_thin_vectors(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(b): Json<RepairOrphanVectorsBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let report = state
        .svc
        .mem
        .graph_prune_thin_vectors(b.dry_run, &b.actor)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "report": report })))
}

pub async fn mem_graph_collapse_synonym_edges(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(b): Json<RepairOrphanVectorsBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let report = state
        .svc
        .mem
        .graph_collapse_synonym_edges(b.dry_run, &b.actor)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "report": report })))
}

pub async fn mem_graph_prune_dream_hub_edges(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(b): Json<RepairOrphanVectorsBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let report = state
        .svc
        .mem
        .graph_prune_dream_hub_edges(b.dry_run, &b.actor)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "report": report })))
}

pub async fn mem_graph_repair_orphan_blobs(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(b): Json<RepairOrphanBlobsBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let report = state
        .svc
        .mem
        .graph_repair_orphan_blobs(b.dry_run, &b.actor)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "report": report })))
}

#[cfg(test)]
mod tests {
    use super::validate_canonical;

    #[test]
    fn accepts_known_entity_prefixes() {
        for id in [
            "person_alpha",
            "project_alpha",
            "machine_alpha",
            "tech_alpha",
            "topic_alpha",
            "role_alpha",
        ] {
            assert!(validate_canonical(id).is_ok(), "should accept: {}", id);
        }
    }

    #[test]
    fn accepts_future_entity_prefixes() {
        // Anything shaped like type_slug works — validation must not lock the API to today's ontology.
        for id in ["org_alpha", "team_alpha", "flow_step_one"] {
            assert!(validate_canonical(id).is_ok(), "should accept: {}", id);
        }
    }

    #[test]
    fn rejects_empty_string() {
        assert!(validate_canonical("").is_err());
    }

    #[test]
    fn rejects_whitespace_only() {
        assert!(validate_canonical("   ").is_err());
    }

    #[test]
    fn rejects_internal_whitespace() {
        let err = validate_canonical("topic alpha").unwrap_err();
        assert!(err.contains("whitespace"));
    }

    #[test]
    fn rejects_no_separator() {
        let err = validate_canonical("nakedid").unwrap_err();
        assert!(err.contains("'_'"));
    }
}
