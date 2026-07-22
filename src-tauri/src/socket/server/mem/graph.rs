//! Socket handlers for graph actions that are not intentionally local-only.

use crate::api::mem_routes::graph::{
    validate_canonical, GraphAliasBody, GraphCreateEntityBody, GraphInvalidateBody,
    GraphPathParams, GraphRelateBody, GraphRelatedParams, GraphTimelineParams, GraphTraverseParams,
    GraphTunnelsParams, RepairAugmentEntityBody, RepairDeleteEntityBody, RepairDeleteRelationsBody,
    RepairFindEntitiesBody, RepairFindRelationsBody, RepairMergeRelationsBody,
    RepairOrphanBlobsBody, RepairOrphanVectorsBody,
};
use crate::mem::graph::{Entity, Relation};
use crate::service::ServiceState;

use super::super::JsonRpcError;
use super::{internal_error, invalid_params, parse_params};

/// Dispatched from `mem::dispatch` with the `mem.graph.` prefix already
/// stripped, so `rest` here is e.g. `"create"` or `"repair.delete_entity"`.
pub(super) fn dispatch(
    rest: &str,
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    match rest {
        "create" => create(params, svc),
        "relate" => relate(params, svc),
        "related" => related(params, svc),
        "invalidate" => invalidate(params, svc),
        "timeline" => timeline(params, svc),
        "traverse" => traverse(params, svc),
        "path" => path(params, svc),
        "tunnels" => tunnels(params, svc),
        "alias" => alias(params, svc),
        "repair.find_relations" => repair_find_relations(params, svc),
        "repair.delete_relations" => repair_delete_relations(params, svc),
        "repair.merge_relations" => repair_merge_relations(params, svc),
        "repair.find_entities" => repair_find_entities(params, svc),
        "repair.augment_entity" => repair_augment_entity(params, svc),
        "repair.delete_entity" => repair_delete_entity(params, svc),
        "repair.orphan_vectors" => repair_orphan_vectors(params, svc),
        "repair.prune_thin_vectors" => repair_prune_thin_vectors(params, svc),
        "repair.collapse_synonym_edges" => repair_collapse_synonym_edges(params, svc),
        "repair.prune_dream_hub_edges" => repair_prune_dream_hub_edges(params, svc),
        "repair.orphan_blobs" => repair_orphan_blobs(params, svc),
        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".to_string(),
            data: None,
        }),
    }
}

fn create(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let b: GraphCreateEntityBody = parse_params(params)?;
    if !b.id.contains('_') {
        return Err(invalid_params(format!(
            "Entity id must follow `<type>_<slug>` format (e.g. person_tima, tech_redis). Got `{}`.",
            b.id
        )));
    }
    let e = Entity {
        id: b.id,
        entity_type: b.entity_type,
        scope: b.scope.unwrap_or_else(|| "global".to_string()),
        properties: b.properties,
    };
    svc.mem.graph_create_entity(&e).map_err(internal_error)?;
    Ok(serde_json::json!({ "ok": true }))
}

fn relate(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let b: GraphRelateBody = parse_params(params)?;
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
    svc.mem.graph_relate(&r).map_err(internal_error)?;
    Ok(serde_json::json!({ "ok": true }))
}

fn related(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let p: GraphRelatedParams = parse_params(params)?;
    let rels = svc
        .mem
        .graph_related(
            &p.id,
            p.rel.as_deref(),
            p.as_of.as_deref(),
            p.include_invalidated,
        )
        .map_err(internal_error)?;
    Ok(serde_json::json!({ "relations": rels }))
}

fn invalidate(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let b: GraphInvalidateBody = parse_params(params)?;
    let found = svc
        .mem
        .graph_invalidate(&b.from, &b.rel, &b.to, b.ended.as_deref())
        .map_err(internal_error)?;
    Ok(serde_json::json!({ "invalidated": found }))
}

fn timeline(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let p: GraphTimelineParams = parse_params(params)?;
    let relations = svc
        .mem
        .graph_timeline(&p.entity, p.include_invalidated)
        .map_err(internal_error)?;
    Ok(serde_json::json!({ "entity": p.entity, "relations": relations }))
}

fn traverse(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let p: GraphTraverseParams = parse_params(params)?;
    let hops = p.hops.unwrap_or(2).min(5);
    let max_nodes = p.max_nodes.unwrap_or(100).min(500);
    let result = svc
        .mem
        .graph_traverse(
            &p.from,
            hops,
            max_nodes,
            p.as_of.as_deref(),
            p.include_invalidated,
        )
        .map_err(internal_error)?;
    Ok(serde_json::json!(result))
}

fn path(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let p: GraphPathParams = parse_params(params)?;
    let max_hops = p.max_hops.unwrap_or(4).min(8);
    let path = svc
        .mem
        .graph_shortest_path(&p.from, &p.to, max_hops, p.include_invalidated)
        .map_err(internal_error)?;
    Ok(serde_json::json!({ "from": p.from, "to": p.to, "path": path }))
}

fn tunnels(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let p: GraphTunnelsParams = parse_params(params)?;
    let tunnels = svc
        .mem
        .graph_tunnels(p.include_invalidated)
        .map_err(internal_error)?;
    Ok(serde_json::json!({ "tunnels": tunnels }))
}

fn alias(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let b: GraphAliasBody = parse_params(params)?;
    validate_canonical(&b.canonical).map_err(invalid_params)?;
    svc.mem
        .graph_alias(&b.alias, &b.canonical)
        .map_err(internal_error)?;
    Ok(serde_json::json!({ "ok": true, "alias": b.alias, "canonical": b.canonical }))
}

fn repair_find_relations(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let b: RepairFindRelationsBody = parse_params(params)?;
    let groups = svc
        .mem
        .graph_find_duplicate_relations(&b.group_by, b.limit)
        .map_err(internal_error)?;
    Ok(serde_json::json!({ "groups": groups }))
}

fn repair_delete_relations(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let b: RepairDeleteRelationsBody = parse_params(params)?;
    let deleted = svc
        .mem
        .graph_delete_relations(&b.rowids, &b.actor)
        .map_err(internal_error)?;
    Ok(serde_json::json!({ "deleted": deleted }))
}

fn repair_merge_relations(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let b: RepairMergeRelationsBody = parse_params(params)?;
    let report = svc
        .mem
        .graph_merge_relations(b.keep, &b.merge, &b.actor)
        .map_err(internal_error)?;
    Ok(serde_json::json!({ "report": report }))
}

fn repair_find_entities(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let b: RepairFindEntitiesBody = parse_params(params)?;
    let groups = svc
        .mem
        .graph_find_duplicate_entities(b.strategy, b.limit)
        .map_err(internal_error)?;
    Ok(serde_json::json!({ "groups": groups }))
}

fn repair_augment_entity(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let b: RepairAugmentEntityBody = parse_params(params)?;
    svc.mem
        .graph_augment_entity(&b.id, b.patch, &b.actor)
        .map_err(internal_error)?;
    Ok(serde_json::json!({ "ok": true }))
}

fn repair_delete_entity(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let b: RepairDeleteEntityBody = parse_params(params)?;
    let report = svc
        .mem
        .graph_delete_entity(&b.id, b.force, b.purge_history, &b.actor)
        .map_err(internal_error)?;
    Ok(serde_json::json!({ "report": report }))
}

fn repair_orphan_vectors(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let b: RepairOrphanVectorsBody = parse_params(params)?;
    let report = svc
        .mem
        .graph_repair_orphan_vectors(b.dry_run, &b.actor)
        .map_err(internal_error)?;
    Ok(serde_json::json!({ "report": report }))
}

fn repair_prune_thin_vectors(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let b: RepairOrphanVectorsBody = parse_params(params)?;
    let report = svc
        .mem
        .graph_prune_thin_vectors(b.dry_run, &b.actor)
        .map_err(internal_error)?;
    Ok(serde_json::json!({ "report": report }))
}

fn repair_collapse_synonym_edges(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let b: RepairOrphanVectorsBody = parse_params(params)?;
    let report = svc
        .mem
        .graph_collapse_synonym_edges(b.dry_run, &b.actor)
        .map_err(internal_error)?;
    Ok(serde_json::json!({ "report": report }))
}

fn repair_prune_dream_hub_edges(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let b: RepairOrphanVectorsBody = parse_params(params)?;
    let report = svc
        .mem
        .graph_prune_dream_hub_edges(b.dry_run, &b.actor)
        .map_err(internal_error)?;
    Ok(serde_json::json!({ "report": report }))
}

fn repair_orphan_blobs(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let b: RepairOrphanBlobsBody = parse_params(params)?;
    let report = svc
        .mem
        .graph_repair_orphan_blobs(b.dry_run, &b.actor)
        .map_err(internal_error)?;
    Ok(serde_json::json!({ "report": report }))
}
