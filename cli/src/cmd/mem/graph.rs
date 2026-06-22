use super::{mem_get_request, mem_post_request};
use crate::MemGraphAction;

pub(crate) fn handle_mem_graph(
    http: &reqwest::blocking::Client,
    base: &str,
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
            mem_post_request(http, &format!("{}/api/mem/graph/entities", base), &payload)
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
            mem_post_request(http, &format!("{}/api/mem/graph/relate", base), &payload)
        }
        MemGraphAction::Related {
            id,
            rel,
            as_of,
            include_invalidated,
        } => {
            let mut url = format!(
                "{}/api/mem/graph/related?id={}",
                base,
                urlencoding::encode(&id)
            );
            if let Some(r) = &rel {
                url.push_str(&format!("&rel={}", r));
            }
            if let Some(d) = &as_of {
                url.push_str(&format!("&as_of={}", urlencoding::encode(d)));
            }
            if include_invalidated {
                url.push_str("&include_invalidated=true");
            }
            mem_get_request(http, &url)
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
            mem_post_request(
                http,
                &format!("{}/api/mem/graph/invalidate", base),
                &payload,
            )
        }
        MemGraphAction::Timeline {
            entity,
            include_invalidated,
        } => {
            let mut url = format!(
                "{}/api/mem/graph/timeline?entity={}",
                base,
                urlencoding::encode(&entity)
            );
            if include_invalidated {
                url.push_str("&include_invalidated=true");
            }
            mem_get_request(http, &url)
        }
        MemGraphAction::Traverse {
            from,
            hops,
            max_nodes,
            as_of,
            include_invalidated,
        } => {
            let mut url = format!(
                "{}/api/mem/graph/traverse?from={}&hops={}&max_nodes={}",
                base,
                urlencoding::encode(&from),
                hops,
                max_nodes
            );
            if let Some(d) = &as_of {
                url.push_str(&format!("&as_of={}", urlencoding::encode(d)));
            }
            if include_invalidated {
                url.push_str("&include_invalidated=true");
            }
            mem_get_request(http, &url)
        }
        MemGraphAction::Path {
            from,
            to,
            max_hops,
            include_invalidated,
        } => {
            let mut url = format!(
                "{}/api/mem/graph/path?from={}&to={}&max_hops={}",
                base,
                urlencoding::encode(&from),
                urlencoding::encode(&to),
                max_hops
            );
            if include_invalidated {
                url.push_str("&include_invalidated=true");
            }
            mem_get_request(http, &url)
        }
        MemGraphAction::Tunnels {
            include_invalidated,
        } => {
            let mut url = format!("{}/api/mem/graph/tunnels", base);
            if include_invalidated {
                url.push_str("?include_invalidated=true");
            }
            mem_get_request(http, &url)
        }
        MemGraphAction::Alias { alias, canonical } => {
            let payload = serde_json::json!({ "alias": alias, "canonical": canonical });
            mem_post_request(http, &format!("{}/api/mem/graph/alias", base), &payload)
        }
        MemGraphAction::FindDuplicateRelations { group_by, limit } => {
            let group_by_vec: Vec<&str> = group_by.split(',').map(|s| s.trim()).collect();
            let payload = serde_json::json!({
                "group_by": group_by_vec,
                "limit": limit,
            });
            mem_post_request(
                http,
                &format!("{}/api/mem/graph/repair/find-relations", base),
                &payload,
            )
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
            mem_post_request(
                http,
                &format!("{}/api/mem/graph/repair/delete-relations", base),
                &payload,
            )
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
            mem_post_request(
                http,
                &format!("{}/api/mem/graph/repair/merge-relations", base),
                &payload,
            )
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
            mem_post_request(
                http,
                &format!("{}/api/mem/graph/repair/find-entities", base),
                &payload,
            )
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
            mem_post_request(
                http,
                &format!("{}/api/mem/graph/repair/augment-entity", base),
                &payload,
            )
        }
        MemGraphAction::RepairOrphanVectors { dry_run, actor } => {
            let payload = serde_json::json!({"dry_run": dry_run, "actor": actor});
            mem_post_request(
                http,
                &format!("{}/api/mem/graph/repair/orphan-vectors", base),
                &payload,
            )
        }
        MemGraphAction::PruneThinVectors { dry_run, actor } => {
            let payload = serde_json::json!({"dry_run": dry_run, "actor": actor});
            mem_post_request(
                http,
                &format!("{}/api/mem/graph/repair/prune-thin-vectors", base),
                &payload,
            )
        }
        MemGraphAction::CollapseSynonymEdges { dry_run, actor } => {
            let payload = serde_json::json!({"dry_run": dry_run, "actor": actor});
            mem_post_request(
                http,
                &format!("{}/api/mem/graph/repair/collapse-synonym-edges", base),
                &payload,
            )
        }
        MemGraphAction::PruneDreamHubEdges { dry_run, actor } => {
            let payload = serde_json::json!({"dry_run": dry_run, "actor": actor});
            mem_post_request(
                http,
                &format!("{}/api/mem/graph/repair/prune-dream-hub-edges", base),
                &payload,
            )
        }
        MemGraphAction::RepairOrphanBlobs { dry_run, actor } => {
            let payload = serde_json::json!({"dry_run": dry_run, "actor": actor});
            mem_post_request(
                http,
                &format!("{}/api/mem/graph/repair/orphan-blobs", base),
                &payload,
            )
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
            mem_post_request(
                http,
                &format!("{}/api/mem/graph/repair/delete-entity", base),
                &payload,
            )
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
