use rusqlite::{params, Connection};
use serde_json::{Map, Value};

use super::types::{DeleteEntityReport, DuplicateEntityGroup, EntitySimilarityStrategy};

pub fn find_duplicate_entities(
    conn: &Connection,
    strategy: EntitySimilarityStrategy,
    limit: usize,
) -> Result<Vec<DuplicateEntityGroup>, String> {
    match strategy {
        EntitySimilarityStrategy::ExactName => {
            let sql = "
                SELECT entity_type,
                       LOWER(TRIM(json_extract(properties_json, '$.name'))) AS norm_name,
                       GROUP_CONCAT(id) AS ids,
                       COUNT(*) AS c
                FROM entities
                WHERE json_extract(properties_json, '$.name') IS NOT NULL
                GROUP BY entity_type, norm_name
                HAVING c > 1
                ORDER BY c DESC
                LIMIT ?1";
            let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map(params![limit as i64], |r| {
                    let entity_type: String = r.get(0)?;
                    let norm_name: String = r.get(1)?;
                    let ids_csv: String = r.get(2)?;
                    Ok(DuplicateEntityGroup {
                        canonical_hint: norm_name,
                        member_ids: ids_csv.split(',').map(String::from).collect(),
                        entity_type,
                        reason: "exact_name".into(),
                    })
                })
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
        }
        EntitySimilarityStrategy::AliasChain => {
            let sql = "
                SELECT canonical_id, GROUP_CONCAT(alias_id) AS aliases, COUNT(*) AS c
                FROM entity_aliases
                GROUP BY canonical_id
                ORDER BY c DESC
                LIMIT ?1";
            let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map(params![limit as i64], |r| {
                    let canonical: String = r.get(0)?;
                    let aliases_csv: String = r.get(1)?;
                    let mut members: Vec<String> =
                        aliases_csv.split(',').map(String::from).collect();
                    members.push(canonical.clone());
                    Ok(DuplicateEntityGroup {
                        canonical_hint: canonical,
                        member_ids: members,
                        entity_type: "Unknown".into(),
                        reason: "alias_chain".into(),
                    })
                })
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
        }
        EntitySimilarityStrategy::Vector {
            threshold,
            entity_type,
        } => find_similar_entities_by_vector(conn, threshold, entity_type.as_deref(), limit),
    }
}

fn find_similar_entities_by_vector(
    conn: &Connection,
    threshold: f64,
    entity_type_filter: Option<&str>,
    limit: usize,
) -> Result<Vec<DuplicateEntityGroup>, String> {
    if !(0.0..=1.0).contains(&threshold) {
        return Err(format!("threshold must be in [0,1], got {}", threshold));
    }
    // cosine_sim = 1 - distance² / 2  ⇒  distance ≤ sqrt(2 * (1 - threshold))
    let max_distance = (2.0 * (1.0 - threshold)).sqrt();

    let entities: Vec<(String, String)> = {
        let mut stmt = if entity_type_filter.is_some() {
            conn.prepare("SELECT id, entity_type FROM entities WHERE entity_type = ?1")
        } else {
            conn.prepare("SELECT id, entity_type FROM entities")
        }
        .map_err(|e| e.to_string())?;
        let rows: Vec<(String, String)> = if let Some(et) = entity_type_filter {
            stmt.query_map(params![et], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .collect()
        } else {
            stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
                .map_err(|e| e.to_string())?
                .filter_map(Result::ok)
                .collect()
        };
        rows
    };

    let mut seen_pairs: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();
    let mut groups: Vec<DuplicateEntityGroup> = Vec::new();

    for (eid, etype) in &entities {
        let qvec: Option<Vec<u8>> = conn
            .query_row(
                "SELECT embedding FROM mem_vectors WHERE mem_id = ?1",
                params![eid],
                |r| r.get::<_, Vec<u8>>(0),
            )
            .ok();
        let Some(qvec) = qvec else {
            continue;
        };

        let mut neighbor_stmt = match conn.prepare(
            "SELECT mem_id, distance FROM mem_vectors
             WHERE embedding MATCH ?1 AND k = 6 ORDER BY distance",
        ) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let neighbors: Vec<(String, f64)> = neighbor_stmt
            .query_map(params![qvec], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?;
        drop(neighbor_stmt);

        for (nid_raw, dist) in neighbors {
            // Strip _chunk_N suffix.
            let nid = if let Some(pos) = nid_raw.rfind("_chunk_") {
                &nid_raw[..pos]
            } else {
                &nid_raw[..]
            };
            if nid == eid {
                continue;
            }
            if dist > max_distance {
                continue;
            }

            let neighbor_type: Option<String> = conn
                .query_row(
                    "SELECT entity_type FROM entities WHERE id = ?1",
                    params![nid],
                    |r| r.get(0),
                )
                .ok();
            let Some(neighbor_type) = neighbor_type else {
                continue;
            };
            if neighbor_type != *etype {
                continue;
            }

            let (a, b) = if eid.as_str() < nid {
                (eid.clone(), nid.to_string())
            } else {
                (nid.to_string(), eid.clone())
            };
            if !seen_pairs.insert((a.clone(), b.clone())) {
                continue;
            }

            let cosine_sim = 1.0 - (dist * dist) / 2.0;
            groups.push(DuplicateEntityGroup {
                canonical_hint: a.clone(),
                member_ids: vec![a, b],
                entity_type: etype.clone(),
                reason: format!("vector_sim:{:.3}", cosine_sim),
            });
            if groups.len() >= limit {
                return Ok(groups);
            }
        }
    }
    Ok(groups)
}

pub fn augment_entity(
    conn: &Connection,
    entity_id: &str,
    patch: Map<String, Value>,
    actor: &str,
) -> Result<(), String> {
    if patch.is_empty() {
        return Ok(());
    }
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;

    let (entity_type, existing): (String, String) = tx
        .query_row(
            "SELECT entity_type, properties_json FROM entities WHERE id = ?1",
            params![entity_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| format!("entity {} not found: {}", entity_id, e))?;

    let mut props: Map<String, Value> = serde_json::from_str::<Value>(&existing)
        .map_err(|e| format!("parse existing props for {}: {}", entity_id, e))?
        .as_object()
        .cloned()
        .unwrap_or_default();
    for (k, v) in &patch {
        props.insert(k.clone(), v.clone());
    }
    let merged = serde_json::to_string(&Value::Object(props.clone())).map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();
    tx.execute(
        "UPDATE entities SET properties_json = ?1, updated_at = ?2 WHERE id = ?3",
        params![merged, &now, entity_id],
    )
    .map_err(|e| e.to_string())?;

    let text = crate::mem::embeddings::embeddable_text_for_entity(entity_id, &entity_type, &merged);
    if let Err(err) = crate::mem::embeddings::index(&tx, entity_id, &text) {
        log::warn!("[augment_entity] re-embed {} failed: {}", entity_id, err);
    }

    let details = serde_json::json!({
        "op": "augment_entity",
        "entity_id": entity_id,
        "patch": Value::Object(patch),
        "result": Value::Object(props),
    })
    .to_string();
    tx.execute(
        "INSERT INTO graph_purge_log (purged_at, count, scope, before, reason_filter, actor, details_json)
         VALUES (?1, 0, NULL, NULL, NULL, ?2, ?3)",
        params![now, actor, details],
    ).map_err(|e| e.to_string())?;

    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}
pub fn delete_entity_by_id(
    conn: &Connection,
    entity_id: &str,
    force: bool,
    purge_history: bool,
    actor: &str,
) -> Result<DeleteEntityReport, String> {
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;

    let (entity_type, scope, properties_json): (String, String, String) = tx
        .query_row(
            "SELECT entity_type, scope, properties_json FROM entities WHERE id = ?1",
            params![entity_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|e| format!("entity {} not found: {}", entity_id, e))?;

    let active_rels: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM relations
         WHERE valid_to IS NULL AND (from_id = ?1 OR to_id = ?1)",
            params![entity_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;

    if active_rels > 0 && !force {
        return Err(format!(
            "entity {} has {} active relations; pass force=true to cascade-delete them",
            entity_id, active_rels
        ));
    }

    let cascaded_rels: Vec<Value> = if active_rels > 0 {
        let mut stmt = tx
            .prepare(
                "SELECT id, from_id, relation_type, to_id, scope, valid_from, valid_to,
                    confidence, properties_json, created_at, reason
             FROM relations WHERE valid_to IS NULL AND (from_id = ?1 OR to_id = ?1)",
            )
            .map_err(|e| e.to_string())?;
        let rows: Vec<Value> = stmt
            .query_map(params![entity_id], |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, i64>(0)?,
                    "from_id": r.get::<_, String>(1)?,
                    "relation_type": r.get::<_, String>(2)?,
                    "to_id": r.get::<_, String>(3)?,
                    "scope": r.get::<_, String>(4)?,
                    "valid_from": r.get::<_, String>(5)?,
                    "valid_to": r.get::<_, Option<String>>(6)?,
                    "confidence": r.get::<_, f64>(7)?,
                    "properties_json": r.get::<_, String>(8)?,
                    "created_at": r.get::<_, String>(9)?,
                    "reason": r.get::<_, Option<String>>(10)?,
                }))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?;
        drop(stmt);
        tx.execute(
            "DELETE FROM relations WHERE valid_to IS NULL AND (from_id = ?1 OR to_id = ?1)",
            params![entity_id],
        )
        .map_err(|e| e.to_string())?;
        rows
    } else {
        Vec::new()
    };

    let cascaded_history: u64 = if purge_history {
        tx.execute(
            "DELETE FROM relations WHERE valid_to IS NOT NULL AND (from_id = ?1 OR to_id = ?1)",
            params![entity_id],
        )
        .map_err(|e| e.to_string())? as u64
    } else {
        0
    };

    tx.execute("DELETE FROM entities WHERE id = ?1", params![entity_id])
        .map_err(|e| e.to_string())?;

    tx.execute(
        "DELETE FROM entity_aliases WHERE alias_id = ?1 OR canonical_id = ?1",
        params![entity_id],
    )
    .map_err(|e| e.to_string())?;

    let _ = tx.execute(
        "DELETE FROM mem_vectors WHERE mem_id = ?1 OR mem_id LIKE ?2",
        params![entity_id, format!("{}_chunk_%", entity_id)],
    );

    let now = chrono::Utc::now().to_rfc3339();
    let details = serde_json::json!({
        "op": "delete_entity_by_id",
        "entity": {
            "id": entity_id,
            "entity_type": entity_type,
            "scope": scope,
            "properties_json": properties_json,
        },
        "cascaded_relations": cascaded_rels,
        "cascaded_history": cascaded_history,
        "force": force,
        "purge_history": purge_history,
    })
    .to_string();
    tx.execute(
        "INSERT INTO graph_purge_log (purged_at, count, scope, before, reason_filter, actor, details_json)
         VALUES (?1, ?2, NULL, NULL, NULL, ?3, ?4)",
        params![now, (1 + cascaded_rels.len() + cascaded_history as usize) as i64, actor, details],
    ).map_err(|e| e.to_string())?;

    tx.commit().map_err(|e| e.to_string())?;
    Ok(DeleteEntityReport {
        entity_id: entity_id.to_string(),
        cascaded_relations: cascaded_rels.len() as u64,
        cascaded_history,
    })
}
