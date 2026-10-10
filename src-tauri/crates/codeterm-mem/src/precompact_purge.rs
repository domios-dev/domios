use std::collections::BTreeMap;
use std::io::Read;

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::MemManager;

const MAX_BODY_BYTES: u64 = 64 * 1024 * 1024;
const MAX_HISTORY_BYTES: u64 = 256 * 1024 * 1024;
const MAX_HISTORY_ROWS: usize = 4096;

#[derive(Debug, Serialize)]
pub struct PurgeReport {
    pub id: String,
    pub dry_run: bool,
    pub provenance: &'static str,
    pub history_rows: usize,
    pub history_rows_removed: usize,
    pub history_body_bytes_removed: u64,
    pub history_body_bytes: u64,
    pub vectors_removed: usize,
    pub fts_rows_removed: usize,
    pub blob_candidates: usize,
    pub blobs_removed: usize,
    pub blob_bytes_unlinked: u64,
    pub blobs_already_absent: usize,
    pub shared_blobs_retained: usize,
    pub logical_purge_complete: bool,
    pub blob_cleanup_complete: bool,
    pub vacuum: String,
    pub fts_compaction: String,
    pub wal_checkpoint: String,
    pub secure_erasure: bool,
    pub limitations: &'static str,
    pub errors: Vec<String>,
}

#[derive(Default)]
struct Inventory {
    rows: usize,
    bytes: u64,
    blobs: BTreeMap<String, u64>,
    previously_purged: bool,
}

fn tab_from_id(id: &str) -> Result<&str, String> {
    let tab = id
        .strip_prefix("conversation_flush_")
        .and_then(|s| s.strip_suffix("_pre_compact"))
        .filter(|s| {
            s.len() == 8
                && s.bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        })
        .ok_or("not an exact retired PreCompact record id")?;
    Ok(tab)
}

fn blob_name(body: &[u8]) -> String {
    format!("sha256_{:x}.txt", Sha256::digest(body))
}

fn inventory(conn: &Connection, id: &str) -> Result<Inventory, String> {
    let tab = tab_from_id(id)?;
    let present: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM mem_items WHERE id=?1)",
            [id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if present {
        return Err(
            "record is still live; verify its complete flush provenance and use mem delete first"
                .into(),
        );
    }
    let mut result = Inventory::default();
    let previous = conn
        .query_row(
            "SELECT history_rows, history_body_bytes FROM mem_precompact_purges WHERE item_id=?1",
            [id],
            |r| Ok((r.get::<_, usize>(0)?, r.get::<_, u64>(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let title = format!("flush:{tab} pre-compact");
    let mut stmt = conn
        .prepare(
            "SELECT change_type,old_title,new_title,old_kind,new_kind,
         coalesce(length(CAST(old_body AS BLOB)),0),coalesce(length(CAST(new_body AS BLOB)),0)
         FROM mem_item_history WHERE item_id=?1 ORDER BY id",
        )
        .map_err(|e| e.to_string())?;
    let mut rows = stmt.query([id]).map_err(|e| e.to_string())?;
    let mut created = false;
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        result.rows += 1;
        if result.rows > MAX_HISTORY_ROWS {
            return Err("flush history exceeds the bounded row limit".into());
        }
        let change: String = row.get(0).map_err(|e| e.to_string())?;
        if !matches!(change.as_str(), "create" | "update" | "delete") {
            return Err("history contains a non-flush lifecycle operation; refusing purge".into());
        }
        for index in [1, 2] {
            let value: Option<String> = row.get(index).map_err(|e| e.to_string())?;
            if value.as_deref().is_some_and(|v| v != title) {
                return Err("history title changed; refusing purge".into());
            }
        }
        for index in [3, 4] {
            let value: Option<String> = row.get(index).map_err(|e| e.to_string())?;
            if value.as_deref().is_some_and(|v| v != "conversation") {
                return Err("history kind changed; refusing purge".into());
            }
        }
        if change == "create" {
            let new_title: Option<String> = row.get(2).map_err(|e| e.to_string())?;
            let new_kind: Option<String> = row.get(4).map_err(|e| e.to_string())?;
            created |=
                new_title.as_deref() == Some(&title) && new_kind.as_deref() == Some("conversation");
        }
        for index in [5, 6] {
            let size: u64 = row.get(index).map_err(|e| e.to_string())?;
            result.bytes = result
                .bytes
                .checked_add(size)
                .ok_or("history size overflow")?;
            if size > MAX_BODY_BYTES || result.bytes > MAX_HISTORY_BYTES {
                return Err("flush history exceeds the bounded body limit".into());
            }
        }
    }
    drop(rows);
    drop(stmt);
    if result.rows == 0 {
        if let Some((count, bytes)) = previous {
            result.rows = count;
            result.bytes = bytes;
            result.previously_purged = true;
        } else {
            return Err("no qualifying retained history or previous purge receipt".into());
        }
    } else if !created || previous.is_some() {
        return Err("missing create provenance or record recreated after purge".into());
    }
    let mut stmt = conn
        .prepare(
            "SELECT old_body FROM mem_item_history WHERE item_id=?1 AND old_body IS NOT NULL
         UNION ALL SELECT new_body FROM mem_item_history WHERE item_id=?1 AND new_body IS NOT NULL",
        )
        .map_err(|e| e.to_string())?;
    let bodies = stmt
        .query_map([id], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    for body in bodies {
        let body = body.map_err(|e| e.to_string())?;
        result
            .blobs
            .insert(blob_name(body.as_bytes()), body.len() as u64);
    }
    let mut stmt = conn
        .prepare("SELECT path,body_bytes FROM mem_precompact_purge_blobs WHERE item_id=?1")
        .map_err(|e| e.to_string())?;
    let pending = stmt
        .query_map([id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u64>(1)?)))
        .map_err(|e| e.to_string())?;
    for row in pending {
        let (name, bytes) = row.map_err(|e| e.to_string())?;
        result.blobs.insert(name, bytes);
    }
    Ok(result)
}

impl MemManager {
    pub fn purge_precompact(
        &self,
        id: &str,
        apply: bool,
        verified_deleted: bool,
        vacuum: bool,
    ) -> Result<PurgeReport, String> {
        if apply && !verified_deleted {
            return Err("apply requires verified_deleted: confirm the original source/summary/tags from the prior deletion receipt".into());
        }
        if vacuum && !apply {
            return Err("vacuum requires apply".into());
        }
        let mut conn = self.conn.lock();
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let items = inventory(&tx, id)?;
        let mut report = PurgeReport {
            id: id.into(), dry_run: !apply,
            provenance: "history signature plus operator-verified prior deletion receipt; source/summary/tags absent from history",
            history_rows: items.rows,
            history_rows_removed: 0,
            history_body_bytes_removed: 0, history_body_bytes: items.bytes,
            vectors_removed: 0, fts_rows_removed: 0, blob_candidates: items.blobs.len(),
            blobs_removed: 0, blob_bytes_unlinked: 0, blobs_already_absent: 0, shared_blobs_retained: 0,
            logical_purge_complete: items.previously_purged, blob_cleanup_complete: false,
            vacuum: "not_requested".into(),
            fts_compaction: "not_requested".into(), wal_checkpoint: "not_requested".into(), secure_erasure: false,
            limitations: "No backup, snapshot, prior WAL copy, filesystem free-space or provider transcript erasure. Shared bodies and graph knowledge are preserved. No global orphan sweep.",
            errors: Vec::new(),
        };
        if !apply {
            return Ok(report);
        }
        if !items.previously_purged {
            report.history_rows_removed = tx
                .execute("DELETE FROM mem_item_history WHERE item_id=?1", [id])
                .map_err(|e| e.to_string())?;
            report.history_body_bytes_removed = items.bytes;
            report.fts_rows_removed = tx
                .execute("DELETE FROM mem_fts WHERE mem_id=?1", [id])
                .map_err(|e| e.to_string())?;
            let vectors: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='mem_vectors')",
                    [],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            if vectors {
                let prefix = format!("{id}_chunk_");
                report.vectors_removed = tx
                    .execute(
                        "DELETE FROM mem_vectors WHERE mem_id=?1 OR
                     (substr(mem_id,1,length(?2))=?2 AND length(mem_id)>length(?2)
                     AND substr(mem_id,length(?2)+1) NOT GLOB '*[^0-9]*')",
                        params![id, prefix],
                    )
                    .map_err(|e| e.to_string())?;
            }
            tx.execute("INSERT INTO mem_precompact_purges(item_id,history_rows,history_body_bytes,purged_at) VALUES(?1,?2,?3,?4)",
                params![id,items.rows,items.bytes,chrono::Utc::now().to_rfc3339()]).map_err(|e| e.to_string())?;
            for (name, bytes) in &items.blobs {
                tx.execute("INSERT INTO mem_precompact_purge_blobs(item_id,path,body_bytes) VALUES(?1,?2,?3)", params![id,name,bytes]).map_err(|e| e.to_string())?;
            }
        }
        tx.commit().map_err(|e| e.to_string())?;
        report.logical_purge_complete = true;
        for (name, bytes) in items.blobs {
            match reclaim_blob(&mut conn, &self.blobs_dir, id, &name, bytes) {
                Ok(BlobOutcome::Removed) => {
                    report.blobs_removed += 1;
                    report.blob_bytes_unlinked += bytes;
                }
                Ok(BlobOutcome::Absent) => report.blobs_already_absent += 1,
                Ok(BlobOutcome::Shared) => report.shared_blobs_retained += 1,
                Err(error) => report.errors.push(format!("{name}: {error}")),
            }
        }
        report.blob_cleanup_complete = report.errors.is_empty();
        if vacuum {
            match conn.execute("INSERT INTO mem_fts(mem_fts) VALUES('optimize')", []) {
                Ok(_) => report.fts_compaction = "completed".into(),
                Err(error) => {
                    report.fts_compaction = "failed".into();
                    report.errors.push(format!("FTS compaction: {error}"));
                }
            }
            match conn.execute_batch("VACUUM") {
                Ok(()) => report.vacuum = "completed".into(),
                Err(error) => {
                    report.vacuum = "failed".into();
                    report.errors.push(format!("vacuum: {error}"));
                }
            }
            let checkpoint = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            });
            match checkpoint {
                Ok((0, log, done)) => {
                    report.wal_checkpoint = format!("completed: log={log}, checkpointed={done}")
                }
                Ok((busy, log, done)) => {
                    report.wal_checkpoint = "incomplete".into();
                    report
                        .errors
                        .push(format!("WAL busy={busy}, log={log}, checkpointed={done}"));
                }
                Err(error) => {
                    report.wal_checkpoint = "failed".into();
                    report.errors.push(format!("WAL checkpoint: {error}"));
                }
            }
        }
        Ok(report)
    }
}

enum BlobOutcome {
    Removed,
    Absent,
    Shared,
}

fn reclaim_blob(
    conn: &mut Connection,
    dir: &std::path::Path,
    id: &str,
    name: &str,
    bytes: u64,
) -> Result<BlobOutcome, String> {
    let hash = name
        .strip_prefix("sha256_")
        .and_then(|s| s.strip_suffix(".txt"))
        .filter(|s| {
            s.len() == 64
                && s.bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        })
        .ok_or("invalid retained blob name")?;
    if bytes > MAX_BODY_BYTES {
        return Err("blob exceeds bounded size".into());
    }
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let path = dir.join(format!("sha256_{hash}.txt"));
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(m) => Some(m),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.to_string()),
    };
    let outcome =
        if let Some(metadata) = metadata {
            if !metadata.is_file() || metadata.len() != bytes {
                return Err("blob type or size changed".into());
            }
            let file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
            let mut body = Vec::new();
            file.take(MAX_BODY_BYTES + 1)
                .read_to_end(&mut body)
                .map_err(|e| e.to_string())?;
            if body.len() as u64 != bytes || blob_name(&body) != name {
                return Err("blob content changed".into());
            }
            let body = std::str::from_utf8(&body).map_err(|e| e.to_string())?;
            let shared: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM mem_items WHERE body_blob_path=?1 OR body_inline=?2)
             OR EXISTS(SELECT 1 FROM mem_item_history WHERE old_body=?2 OR new_body=?2)",
            params![name,body], |r| r.get(0),
        ).map_err(|e| e.to_string())?;
            if shared {
                return Ok(BlobOutcome::Shared);
            }
            std::fs::remove_file(&path).map_err(|e| e.to_string())?;
            BlobOutcome::Removed
        } else {
            BlobOutcome::Absent
        };
    tx.execute(
        "DELETE FROM mem_precompact_purge_blobs WHERE item_id=?1 AND path=?2",
        params![id, name],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(outcome)
}

#[cfg(test)]
mod tests;
