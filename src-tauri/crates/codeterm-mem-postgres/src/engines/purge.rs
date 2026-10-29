use super::*;
use codeterm_mem::precompact_purge::PurgeReport;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
const MAX_BODY: u64 = 64 * 1024 * 1024;
const MAX_HISTORY: u64 = 256 * 1024 * 1024;
#[derive(Default)]
struct Inventory {
    rows: usize,
    bytes: u64,
    blobs: BTreeMap<String, u64>,
    previous: bool,
}
fn blob_name(body: &[u8]) -> String {
    format!("sha256_{:x}.txt", Sha256::digest(body))
}
fn inventory(tx: &mut Transaction<'_>, id: &str) -> Result<Inventory, String> {
    let tab = id
        .strip_prefix("conversation_flush_")
        .and_then(|s| s.strip_suffix("_pre_compact"))
        .filter(|s| {
            s.len() == 8
                && s.bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        })
        .ok_or("not an exact retired PreCompact record id")?;
    if tx
        .query_opt("SELECT id FROM mem_items WHERE id=$1", &[&id])
        .map_err(db_error)?
        .is_some()
    {
        return Err(
            "record is still live; verify its complete flush provenance and use mem delete first"
                .into(),
        );
    }
    let previous = tx
        .query_opt(
            "SELECT history_rows,history_body_bytes FROM mem_precompact_purges WHERE item_id=$1",
            &[&id],
        )
        .map_err(db_error)?;
    let rows=tx.query("SELECT change_type,old_title,new_title,old_kind,new_kind,coalesce(octet_length(old_body),0)::bigint,coalesce(octet_length(new_body),0)::bigint FROM mem_item_history WHERE item_id=$1 ORDER BY id LIMIT 4097", &[&id]).map_err(db_error)?;
    if rows.len() > 4096 {
        return Err("flush history exceeds the bounded row limit".into());
    }
    let mut result = Inventory::default();
    let title = format!("flush:{tab} pre-compact");
    let mut created = false;
    for r in rows {
        result.rows += 1;
        let change: String = r.get(0);
        if !matches!(change.as_str(), "create" | "update" | "delete") {
            return Err("history contains a non-flush lifecycle operation; refusing purge".into());
        }
        for i in [1, 2] {
            if r.get::<_, Option<String>>(i).is_some_and(|v| v != title) {
                return Err("history title changed; refusing purge".into());
            }
        }
        for i in [3, 4] {
            if r.get::<_, Option<String>>(i)
                .is_some_and(|v| v != "conversation")
            {
                return Err("history kind changed; refusing purge".into());
            }
        }
        created |= change == "create"
            && r.get::<_, Option<String>>(2).as_deref() == Some(&title)
            && r.get::<_, Option<String>>(4).as_deref() == Some("conversation");
        for i in [5, 6] {
            let size = r.get::<_, i64>(i) as u64;
            result.bytes = result
                .bytes
                .checked_add(size)
                .ok_or("history size overflow")?;
            if size > MAX_BODY || result.bytes > MAX_HISTORY {
                return Err("flush history exceeds the bounded body limit".into());
            }
        }
    }
    if result.rows == 0 {
        if let Some(r) = &previous {
            result.rows = r.get::<_, i64>(0) as usize;
            result.bytes = r.get::<_, i64>(1) as u64;
            result.previous = true;
        } else {
            return Err("no qualifying retained history or previous purge receipt".into());
        }
    } else if !created || previous.is_some() {
        return Err("missing create provenance or record recreated after purge".into());
    }
    for r in tx.query("SELECT old_body FROM mem_item_history WHERE item_id=$1 AND old_body IS NOT NULL UNION ALL SELECT new_body FROM mem_item_history WHERE item_id=$1 AND new_body IS NOT NULL", &[&id]).map_err(db_error)? {
        let body:String=r.get(0);result.blobs.insert(blob_name(body.as_bytes()),body.len() as u64);
    }
    for r in tx
        .query(
            "SELECT path,body_bytes FROM mem_precompact_purge_blobs WHERE item_id=$1",
            &[&id],
        )
        .map_err(db_error)?
    {
        result.blobs.insert(r.get(0), r.get::<_, i64>(1) as u64);
    }
    Ok(result)
}
impl PostgresMemStore {
    pub(super) fn purge_precompact_in(
        &self,
        id: &str,
        apply: bool,
        verified: bool,
        vacuum: bool,
    ) -> Result<PurgeReport, String> {
        if apply && !verified {
            return Err("apply requires verified_deleted: confirm the original source/summary/tags from the prior deletion receipt".into());
        }
        if vacuum && !apply {
            return Err("vacuum requires apply".into());
        }
        let (items,mut report)=self.transaction(|tx| {
            let items=inventory(tx,id)?;
            let mut report=PurgeReport {
                id:id.into(),dry_run:!apply,provenance:"history signature plus operator-verified prior deletion receipt; source/summary/tags absent from history",
                history_rows:items.rows,history_rows_removed:0,history_body_bytes_removed:0,history_body_bytes:items.bytes,vectors_removed:0,fts_rows_removed:0,blob_candidates:items.blobs.len(),blobs_removed:0,blob_bytes_unlinked:0,blobs_already_absent:0,shared_blobs_retained:0,logical_purge_complete:items.previous,blob_cleanup_complete:false,vacuum:"not_requested".into(),fts_compaction:"not_requested".into(),wal_checkpoint:"not_requested".into(),secure_erasure:false,limitations:"No backup, snapshot, prior WAL copy, filesystem free-space or provider transcript erasure. Shared bodies and graph knowledge are preserved. No global orphan sweep.",errors:Vec::new()
            };
            if apply&&!items.previous {
                report.history_rows_removed=tx.execute("DELETE FROM mem_item_history WHERE item_id=$1", &[&id]).map_err(db_error)? as usize;
                report.history_body_bytes_removed=items.bytes;
                report.fts_rows_removed=tx.execute("DELETE FROM mem_fts WHERE mem_id=$1", &[&id]).map_err(db_error)? as usize;
                if vectors::stamp_in(tx)?.is_some() {
                    let prefix=format!("{id}_chunk_");
                    report.vectors_removed=tx.execute("DELETE FROM mem_vectors WHERE mem_id=$1 OR (left(mem_id,length($2))=$2 AND substring(mem_id FROM length($2)+1) ~ '^[0-9]+$')", &[&id,&prefix]).map_err(db_error)? as usize;
                }
                tx.execute("INSERT INTO mem_precompact_purges(item_id,history_rows,history_body_bytes,purged_at) VALUES ($1,$2,$3,$4)", &[&id,&(items.rows as i64),&(items.bytes as i64),&crate::items::now()]).map_err(db_error)?;
                for (name,size) in &items.blobs { tx.execute("INSERT INTO mem_precompact_purge_blobs(item_id,path,body_bytes) VALUES ($1,$2,$3)", &[&id,&name,&(*size as i64)]).map_err(db_error)?; }
            }
            Ok((items,report))
        })?;
        if !apply {
            return Ok(report);
        }
        report.logical_purge_complete = true;
        for (name, size) in items.blobs {
            let outcome = self.transaction(|tx| reclaim(tx, id, &name, size));
            match outcome {
                Ok(Outcome::Removed) => {
                    report.blobs_removed += 1;
                    report.blob_bytes_unlinked += size;
                }
                Ok(Outcome::Absent) => report.blobs_already_absent += 1,
                Ok(Outcome::Shared) => report.shared_blobs_retained += 1,
                Err(e) => report.errors.push(format!("{name}: {e}")),
            }
        }
        report.blob_cleanup_complete = report.errors.is_empty();
        if vacuum {
            report.wal_checkpoint = "managed_by_postgres".into();
            let result = self.vacuum_precompact();
            match result {
                Ok(()) => {
                    report.vacuum = "completed".into();
                    report.fts_compaction = "completed".into();
                }
                Err(e) => {
                    report.vacuum = "failed".into();
                    report.fts_compaction = "failed".into();
                    report.errors.push(e);
                }
            }
        }
        Ok(report)
    }

    pub(crate) fn vacuum_precompact(&self) -> Result<(), String> {
        self.nontransactional_write(|conn| {
            conn.batch_execute("VACUUM (ANALYZE) mem_fts, mem_item_history, mem_blobs")
                .map_err(db_error)
        })
    }
}
enum Outcome {
    Removed,
    Absent,
    Shared,
}
fn reclaim(tx: &mut Transaction<'_>, id: &str, name: &str, size: u64) -> Result<Outcome, String> {
    name.strip_prefix("sha256_")
        .and_then(|s| s.strip_suffix(".txt"))
        .filter(|s| {
            s.len() == 64
                && s.bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        })
        .ok_or("invalid retained blob name")?;
    if size > MAX_BODY {
        return Err("blob exceeds bounded size".into());
    }
    let row = tx
        .query_opt(
            "SELECT body FROM mem_blobs WHERE name=$1 FOR UPDATE",
            &[&name],
        )
        .map_err(db_error)?;
    let outcome = if let Some(r) = row {
        let bytes: Vec<u8> = r.get(0);
        if bytes.len() as u64 != size || blob_name(&bytes) != name {
            return Err("blob content changed".into());
        }
        let body = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
        let shared:bool=tx.query_one("SELECT EXISTS(SELECT 1 FROM mem_items WHERE body_blob_path=$1 OR body_inline=$2) OR EXISTS(SELECT 1 FROM mem_item_history WHERE old_body=$2 OR new_body=$2)", &[&name,&body]).map_err(db_error)?.get(0);
        if shared {
            return Ok(Outcome::Shared);
        }
        tx.execute("DELETE FROM mem_blobs WHERE name=$1", &[&name])
            .map_err(db_error)?;
        Outcome::Removed
    } else {
        Outcome::Absent
    };
    tx.execute(
        "DELETE FROM mem_precompact_purge_blobs WHERE item_id=$1 AND path=$2",
        &[&id, &name],
    )
    .map_err(db_error)?;
    Ok(outcome)
}
