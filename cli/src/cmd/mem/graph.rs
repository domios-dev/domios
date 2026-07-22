use crate::client::SocketClient;
use crate::MemGraphAction;

pub(crate) fn handle_mem_graph(
    client: &mut SocketClient,
    action: MemGraphAction,
) -> Result<serde_json::Value, String> {
    match action {
        MemGraphAction::Create {
            id,
            entity_type,
            props,
            scope,
        } => {
            let props_val: serde_json::Value =
                serde_json::from_str(&props).unwrap_or(serde_json::json!({}));
            let payload = serde_json::json!({
                "id": id, "type": entity_type, "scope": scope, "properties": props_val,
            });
            client.call("mem.graph.create", Some(payload))
        }
        MemGraphAction::Relate {
            from,
            rel,
            to,
            valid_from,
            valid_to,
            confidence,
            scope,
            reason,
        } => {
            let payload = serde_json::json!({
                "from": from, "rel": rel, "to": to,
                "valid_from": valid_from, "valid_to": valid_to,
                "confidence": confidence, "scope": scope,
                "reason": reason,
            });
            client.call("mem.graph.relate", Some(payload))
        }
        MemGraphAction::Related {
            id,
            rel,
            as_of,
            include_invalidated,
        } => {
            let payload = serde_json::json!({
                "id": id, "rel": rel, "as_of": as_of, "include_invalidated": include_invalidated,
            });
            client.call("mem.graph.related", Some(payload))
        }
        MemGraphAction::Invalidate {
            from,
            rel,
            to,
            ended,
        } => {
            let payload = serde_json::json!({
                "from": from, "rel": rel, "to": to, "ended": ended,
            });
            client.call("mem.graph.invalidate", Some(payload))
        }
        MemGraphAction::Timeline {
            entity,
            include_invalidated,
        } => {
            let payload = serde_json::json!({
                "entity": entity, "include_invalidated": include_invalidated,
            });
            client.call("mem.graph.timeline", Some(payload))
        }
        MemGraphAction::Traverse {
            from,
            hops,
            max_nodes,
            as_of,
            include_invalidated,
        } => {
            let payload = serde_json::json!({
                "from": from, "hops": hops, "max_nodes": max_nodes,
                "as_of": as_of, "include_invalidated": include_invalidated,
            });
            client.call("mem.graph.traverse", Some(payload))
        }
        MemGraphAction::Path {
            from,
            to,
            max_hops,
            include_invalidated,
        } => {
            let payload = serde_json::json!({
                "from": from, "to": to, "max_hops": max_hops,
                "include_invalidated": include_invalidated,
            });
            client.call("mem.graph.path", Some(payload))
        }
        MemGraphAction::Tunnels {
            include_invalidated,
        } => {
            let payload = serde_json::json!({ "include_invalidated": include_invalidated });
            client.call("mem.graph.tunnels", Some(payload))
        }
        MemGraphAction::Alias { alias, canonical } => {
            let payload = serde_json::json!({ "alias": alias, "canonical": canonical });
            client.call("mem.graph.alias", Some(payload))
        }
        MemGraphAction::FindDuplicateRelations { group_by, limit } => {
            let group_by_vec: Vec<&str> = group_by.split(',').map(|s| s.trim()).collect();
            let payload = serde_json::json!({
                "group_by": group_by_vec,
                "limit": limit,
            });
            client.call("mem.graph.repair.find_relations", Some(payload))
        }
        MemGraphAction::DeleteRelations { rowids, actor } => {
            let rowids_vec: Vec<i64> = rowids
                .split(',')
                .filter_map(|s| s.trim().parse::<i64>().ok())
                .collect();
            let payload = serde_json::json!({
                "rowids": rowids_vec,
                "actor": actor,
            });
            client.call("mem.graph.repair.delete_relations", Some(payload))
        }
        MemGraphAction::MergeRelations { keep, merge, actor } => {
            let merge_vec: Vec<i64> = merge
                .split(',')
                .filter_map(|s| s.trim().parse::<i64>().ok())
                .collect();
            let payload = serde_json::json!({
                "keep": keep,
                "merge": merge_vec,
                "actor": actor,
            });
            client.call("mem.graph.repair.merge_relations", Some(payload))
        }
        MemGraphAction::FindDuplicateEntities {
            strategy,
            threshold,
            entity_type,
            limit,
        } => {
            let strategy_value = match strategy.as_str() {
                "exact-name" | "exact_name" => serde_json::json!({"kind": "exact_name"}),
                "alias-chain" | "alias_chain" => serde_json::json!({"kind": "alias_chain"}),
                "vector" => {
                    let mut v = serde_json::json!({"kind": "vector", "threshold": threshold});
                    if let Some(et) = entity_type.clone() {
                        v["entity_type"] = serde_json::Value::String(et);
                    }
                    v
                }
                other => {
                    eprintln!(
                        "unknown strategy: {} (use exact-name, alias-chain, or vector)",
                        other
                    );
                    std::process::exit(1);
                }
            };
            let payload = serde_json::json!({
                "strategy": strategy_value,
                "limit": limit,
            });
            client.call("mem.graph.repair.find_entities", Some(payload))
        }
        MemGraphAction::AugmentEntity { id, patch, actor } => {
            let patch_val: serde_json::Value =
                serde_json::from_str(&patch).unwrap_or(serde_json::json!({}));
            if !patch_val.is_object() {
                eprintln!("--patch must be a JSON object, got: {}", patch);
                std::process::exit(1);
            }
            let payload = serde_json::json!({
                "id": id,
                "patch": patch_val,
                "actor": actor,
            });
            client.call("mem.graph.repair.augment_entity", Some(payload))
        }
        MemGraphAction::RepairOrphanVectors { dry_run, actor } => {
            let payload = serde_json::json!({"dry_run": dry_run, "actor": actor});
            client.call("mem.graph.repair.orphan_vectors", Some(payload))
        }
        MemGraphAction::PruneThinVectors { dry_run, actor } => {
            let payload = serde_json::json!({"dry_run": dry_run, "actor": actor});
            client.call("mem.graph.repair.prune_thin_vectors", Some(payload))
        }
        MemGraphAction::CollapseSynonymEdges { dry_run, actor } => {
            let payload = serde_json::json!({"dry_run": dry_run, "actor": actor});
            client.call("mem.graph.repair.collapse_synonym_edges", Some(payload))
        }
        MemGraphAction::PruneDreamHubEdges { dry_run, actor } => {
            let payload = serde_json::json!({"dry_run": dry_run, "actor": actor});
            client.call("mem.graph.repair.prune_dream_hub_edges", Some(payload))
        }
        MemGraphAction::RepairOrphanBlobs { dry_run, actor } => {
            let payload = serde_json::json!({"dry_run": dry_run, "actor": actor});
            client.call("mem.graph.repair.orphan_blobs", Some(payload))
        }
        MemGraphAction::DeleteEntity {
            id,
            force,
            purge_history,
            actor,
        } => {
            let payload = serde_json::json!({
                "id": id,
                "force": force,
                "purge_history": purge_history,
                "actor": actor,
            });
            client.call("mem.graph.repair.delete_entity", Some(payload))
        }
        // Local-only mutations + reads that benefit from direct DB access: route through the same dispatcher as main()
        // cannot turn this into a panic in production. The run_* helpers exit; the line after never runs.
        local @ (MemGraphAction::Route { .. }
        | MemGraphAction::PurgeInvalidated { .. }
        | MemGraphAction::ReviveBroken { .. }
        | MemGraphAction::IntroduceTopic { .. }
        | MemGraphAction::Hubs { .. }
        | MemGraphAction::Islands { .. }
        | MemGraphAction::Orphans { .. }
        | MemGraphAction::Get { .. }) => {
            crate::cmd::graph_mutate::try_dispatch_local(&local);
            unreachable!("local-only graph variant did not exit")
        }
    }
}
