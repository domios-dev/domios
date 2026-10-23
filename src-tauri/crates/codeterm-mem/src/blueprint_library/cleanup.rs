use super::*;
use std::path::Path;

pub(crate) fn remove_v1(conn: &Connection, blobs: &Path) -> Result<usize, String> {
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS blueprint_v1_cleanup_receipt (id INTEGER PRIMARY KEY CHECK(id=1), removed_count INTEGER NOT NULL, completed_at TEXT NOT NULL)").map_err(|e| e.to_string())?;
    let completed: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM blueprint_v1_cleanup_receipt)",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if completed {
        return Ok(0);
    }
    let ids = {
        let mut stmt = tx
            .prepare("SELECT id FROM mem_items WHERE source=?1 AND kind IN ('skill','archived')")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([SOURCE], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    };
    let mut obsolete = Vec::new();
    for id in ids {
        let item =
            store::get(&tx, &id, blobs)?.ok_or("Blueprint solution disappeared during cleanup")?;
        let version = serde_yaml::from_str::<serde_json::Value>(&item.body)
            .ok()
            .and_then(|v| v["schema_version"].as_u64());
        if version == Some(1) {
            obsolete.push((id, item.title, item.body));
        }
    }
    let vectors: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='mem_vectors')",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    let mut blob_paths = Vec::new();
    for (id, title, body) in &obsolete {
        let blob: Option<String> = tx
            .query_row(
                "SELECT body_blob_path FROM mem_items WHERE id=?1",
                [id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        blob_paths.extend(blob);
        tx.execute("DELETE FROM mem_fts WHERE mem_id=?1", [id])
            .map_err(|e| e.to_string())?;
        if vectors {
            tx.execute("DELETE FROM mem_vectors WHERE mem_id=?1 OR (substr(mem_id,1,length(?1)+7)=?1||'_chunk_' AND NOT EXISTS(SELECT 1 FROM mem_items WHERE id=mem_vectors.mem_id))",[id]).map_err(|e|e.to_string())?;
        }
        tx.execute("DELETE FROM relations WHERE from_id=?1 OR to_id=?1", [id])
            .map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO mem_item_history(item_id,changed_at,changed_by,change_type,old_title,old_body,reason) VALUES (?1,datetime('now'),'system:blueprint-v2','delete',?2,?3,'Obsolete v1 Blueprint solution removed; native v2 required')",params![id,title,body]).map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM mem_items WHERE id=?1", [id])
            .map_err(|e| e.to_string())?;
    }
    tx.execute(
        "INSERT INTO blueprint_v1_cleanup_receipt VALUES (1,?1,datetime('now'))",
        [obsolete.len() as i64],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    if !obsolete.is_empty() {
        log::warn!("[blueprint-memory] removed {} obsolete v1 solutions; cleanup receipt and history retained", obsolete.len());
    }
    for path in blob_paths {
        let referenced: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM mem_items WHERE body_blob_path=?1)",
                [&path],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if !referenced
            && std::path::Path::new(&path).components().count() == 1
            && path.starts_with("sha256_")
            && path.ends_with(".txt")
        {
            match std::fs::remove_file(blobs.join(path)) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.to_string()),
            }
        }
    }
    Ok(obsolete.len())
}
