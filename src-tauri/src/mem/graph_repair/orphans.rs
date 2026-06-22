use rusqlite::{params, Connection};

use super::types::{RepairOrphanBlobsReport, RepairOrphanVectorsReport};

pub fn repair_orphan_blobs(
    conn: &Connection,
    blobs_dir: &std::path::Path,
    dry_run: bool,
    actor: &str,
) -> Result<RepairOrphanBlobsReport, String> {
    let mut referenced: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut stmt = conn
        .prepare("SELECT DISTINCT body_blob_path FROM mem_items WHERE body_blob_path IS NOT NULL")
        .map_err(|e| e.to_string())?;
    for row in stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?
    {
        if let Ok(p) = row {
            referenced.insert(p);
        }
    }
    drop(stmt);

    let mut orphan_files = Vec::new();
    if blobs_dir.is_dir() {
        for entry in std::fs::read_dir(blobs_dir).map_err(|e| format!("read blobs_dir: {}", e))? {
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with("sha256_") {
                continue;
            }
            if !referenced.contains(&name) {
                orphan_files.push(name);
            }
        }
    }

    if dry_run || orphan_files.is_empty() {
        return Ok(RepairOrphanBlobsReport {
            deleted: 0,
            orphan_files,
            dry_run,
        });
    }

    let mut deleted = 0u64;
    for name in &orphan_files {
        if std::fs::remove_file(blobs_dir.join(name)).is_ok() {
            deleted += 1;
        }
    }

    let now = chrono::Utc::now().to_rfc3339();
    let details = serde_json::json!({
        "op": "repair_orphan_blobs",
        "orphan_files": orphan_files,
    })
    .to_string();
    conn.execute(
        "INSERT INTO graph_purge_log (purged_at, count, scope, before, reason_filter, actor, details_json)
         VALUES (?1, ?2, NULL, NULL, NULL, ?3, ?4)",
        params![now, deleted as i64, actor, details],
    ).map_err(|e| e.to_string())?;

    Ok(RepairOrphanBlobsReport {
        orphan_files,
        deleted,
        dry_run: false,
    })
}

pub fn repair_orphan_vectors(
    conn: &Connection,
    dry_run: bool,
    actor: &str,
) -> Result<RepairOrphanVectorsReport, String> {
    let sql = "SELECT mem_id FROM mem_vectors v
               WHERE NOT EXISTS (
                 SELECT 1 FROM mem_items
                 WHERE id = v.mem_id
                    OR (instr(v.mem_id, '_chunk_') > 0 AND id = substr(v.mem_id, 1, instr(v.mem_id, '_chunk_') - 1))
               )
               AND NOT EXISTS (
                 SELECT 1 FROM entities
                 WHERE id = v.mem_id
                    OR (instr(v.mem_id, '_chunk_') > 0 AND id = substr(v.mem_id, 1, instr(v.mem_id, '_chunk_') - 1))
               )";
    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| format!("orphan-vectors prepare: {}", e))?;
    let orphan_ids: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    drop(stmt);

    if dry_run || orphan_ids.is_empty() {
        return Ok(RepairOrphanVectorsReport {
            deleted: 0,
            orphan_ids,
            dry_run,
        });
    }

    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let placeholders = std::iter::repeat("?")
        .take(orphan_ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let bind: Vec<&dyn rusqlite::ToSql> = orphan_ids
        .iter()
        .map(|s| s as &dyn rusqlite::ToSql)
        .collect();
    let deleted = tx
        .execute(
            &format!("DELETE FROM mem_vectors WHERE mem_id IN ({})", placeholders),
            rusqlite::params_from_iter(bind.iter()),
        )
        .map_err(|e| e.to_string())? as u64;

    let now = chrono::Utc::now().to_rfc3339();
    let details = serde_json::json!({
        "op": "repair_orphan_vectors",
        "orphan_ids": orphan_ids,
    })
    .to_string();
    tx.execute(
        "INSERT INTO graph_purge_log (purged_at, count, scope, before, reason_filter, actor, details_json)
         VALUES (?1, ?2, NULL, NULL, NULL, ?3, ?4)",
        params![now, deleted as i64, actor, details],
    ).map_err(|e| e.to_string())?;

    tx.commit().map_err(|e| e.to_string())?;
    Ok(RepairOrphanVectorsReport {
        orphan_ids,
        deleted,
        dry_run: false,
    })
}

/// Delete vectors whose owning mem_item has no real body (empty / title-only /
/// below the embed threshold). These are the "bodyless embeddings" — pure noise
/// in vector search. Blob-stored bodies (large) are never thin, so only inline
/// bodies are considered. Reversible via graph_purge_log.
pub fn prune_thin_vectors(
    conn: &Connection,
    dry_run: bool,
    actor: &str,
) -> Result<RepairOrphanVectorsReport, String> {
    let min = crate::mem::MIN_EMBED_BODY_CHARS as i64;
    let sql = format!(
        "SELECT v.mem_id FROM mem_vectors v
         JOIN mem_items m ON (
            m.id = v.mem_id
            OR (instr(v.mem_id, '_chunk_') > 0 AND m.id = substr(v.mem_id, 1, instr(v.mem_id, '_chunk_') - 1))
         )
         WHERE m.body_blob_path IS NULL
           AND LENGTH(TRIM(COALESCE(m.body_inline, ''))) < {}",
        min
    );
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| format!("thin-vectors prepare: {}", e))?;
    let thin_ids: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    drop(stmt);

    if dry_run || thin_ids.is_empty() {
        return Ok(RepairOrphanVectorsReport {
            deleted: 0,
            orphan_ids: thin_ids,
            dry_run,
        });
    }

    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let placeholders = std::iter::repeat("?")
        .take(thin_ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let bind: Vec<&dyn rusqlite::ToSql> =
        thin_ids.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
    let deleted = tx
        .execute(
            &format!("DELETE FROM mem_vectors WHERE mem_id IN ({})", placeholders),
            rusqlite::params_from_iter(bind.iter()),
        )
        .map_err(|e| e.to_string())? as u64;

    let now = chrono::Utc::now().to_rfc3339();
    let details = serde_json::json!({
        "op": "prune_thin_vectors",
        "thin_ids": thin_ids,
    })
    .to_string();
    tx.execute(
        "INSERT INTO graph_purge_log (purged_at, count, scope, before, reason_filter, actor, details_json)
         VALUES (?1, ?2, NULL, NULL, NULL, ?3, ?4)",
        params![now, deleted as i64, actor, details],
    )
    .map_err(|e| e.to_string())?;

    tx.commit().map_err(|e| e.to_string())?;
    Ok(RepairOrphanVectorsReport {
        orphan_ids: thin_ids,
        deleted,
        dry_run: false,
    })
}
