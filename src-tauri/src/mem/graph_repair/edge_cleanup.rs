use rusqlite::{params, Connection};

use super::types::RepairOrphanVectorsReport;

const MEMBERSHIP_PREDS: &str = "'about','applies_to','belongs_to','part_of','documented_in'";

/// Predicates a dream diary uses to backlink into hubs it merely discussed —
/// pure hub noise (the analysis lives in the diary body).
const DREAM_NOISE_PREDS: &str =
    "'about','applies_to','works_on','postmortem_for','analyzes','mentions','discusses'";

fn invalidate_relation_ids(
    conn: &Connection,
    ids: Vec<i64>,
    op: &str,
    dry_run: bool,
    actor: &str,
) -> Result<RepairOrphanVectorsReport, String> {
    let id_strs: Vec<String> = ids.iter().map(|i| i.to_string()).collect();
    if dry_run || ids.is_empty() {
        return Ok(RepairOrphanVectorsReport {
            deleted: 0,
            orphan_ids: id_strs,
            dry_run,
        });
    }
    let now = chrono::Utc::now().to_rfc3339();
    let id_list = id_strs.join(",");
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    // Soft-delete: set valid_to, preserving history (reversible, audited).
    let invalidated = tx
        .execute(
            &format!(
                "UPDATE relations SET valid_to = ?1 WHERE id IN ({}) AND valid_to IS NULL",
                id_list
            ),
            params![now],
        )
        .map_err(|e| e.to_string())? as u64;
    let details = serde_json::json!({ "op": op, "rowids": ids }).to_string();
    tx.execute(
        "INSERT INTO graph_purge_log (purged_at, count, scope, before, reason_filter, actor, details_json)
         VALUES (?1, ?2, NULL, NULL, NULL, ?3, ?4)",
        params![now, invalidated as i64, actor, details],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(RepairOrphanVectorsReport {
        deleted: invalidated,
        orphan_ids: id_strs,
        dry_run: false,
    })
}

/// Collapse parallel membership-synonym edges graph-wide in ONE pass: for every
/// node-pair carrying more than one membership predicate (about / applies_to /
/// belongs_to / part_of / documented_in), keep the highest-priority one (about
/// first) and invalidate the rest. Distinct predicates (uses, runs_on, …) are
/// never in the set, so they're untouched. Replaces hundreds of per-edge agent
/// round-trips with a single server-side query.
pub fn collapse_synonym_edges(
    conn: &Connection,
    dry_run: bool,
    actor: &str,
) -> Result<RepairOrphanVectorsReport, String> {
    let sql = format!(
        "SELECT id FROM (
           SELECT id, ROW_NUMBER() OVER (
             PARTITION BY from_id, to_id
             ORDER BY CASE relation_type
               WHEN 'about' THEN 0 WHEN 'applies_to' THEN 1 WHEN 'belongs_to' THEN 2
               WHEN 'part_of' THEN 3 WHEN 'documented_in' THEN 4 ELSE 9 END, id
           ) AS rn
           FROM relations
           WHERE valid_to IS NULL AND relation_type IN ({preds})
         ) WHERE rn > 1",
        preds = MEMBERSHIP_PREDS
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let ids: Vec<i64> = stmt
        .query_map([], |r| r.get::<_, i64>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    drop(stmt);
    invalidate_relation_ids(conn, ids, "collapse_synonym_edges", dry_run, actor)
}

/// Invalidate, in ONE pass, every edge from a dream diary (kind=daily) to a graph
/// entity via a hub-noise predicate — dreams record their analysis in the body,
/// not as backlinks that inflate project/topic/person hubs. The diary's link to
/// the dreams anchor (project_dreams / scope_global) is preserved.
pub fn prune_dream_hub_edges(
    conn: &Connection,
    dry_run: bool,
    actor: &str,
) -> Result<RepairOrphanVectorsReport, String> {
    let sql = format!(
        "SELECT r.id FROM relations r
           JOIN mem_items m ON m.id = r.from_id AND m.kind = 'daily'
           JOIN entities e ON e.id = r.to_id
         WHERE r.valid_to IS NULL
           AND r.relation_type IN ({preds})
           AND r.to_id NOT IN ('project_dreams','scope_global')",
        preds = DREAM_NOISE_PREDS
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let ids: Vec<i64> = stmt
        .query_map([], |r| r.get::<_, i64>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    drop(stmt);
    invalidate_relation_ids(conn, ids, "prune_dream_hub_edges", dry_run, actor)
}
