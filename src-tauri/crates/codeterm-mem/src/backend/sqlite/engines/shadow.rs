use super::*;
use rusqlite::{params, Connection, OptionalExtension};

type Checkpoint = (Vec<u8>, Option<Vec<u8>>);

pub(super) fn checkpoint(
    conn: &Connection,
    stamp: &str,
    key: &str,
) -> Result<Option<Checkpoint>, String> {
    conn.query_row(
        "SELECT text_hash, vectors FROM reembed.shadow WHERE target=?1 AND unit_key=?2",
        params![stamp, key],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()
    .map_err(|error| error.to_string())
}

impl SqliteMemStore {
    pub(super) fn shadow_units(&self, conn: &Connection) -> Result<Vec<ShadowUnit>, String> {
        let mut units = Vec::new();
        for id in crate::backend::sqlite::blueprint::index::indexed_items(conn)? {
            units.push(ShadowUnit {
                texts: crate::backend::sqlite::blueprint::vector_projection(conn, &id)?
                    .map(|(row, intents)| {
                        crate::blueprint_library::enrichment::blueprint_vector_texts(&row, &intents)
                    })
                    .unwrap_or_default(),
                id,
                blueprint: true,
            });
        }
        let mut targets = self.item_targets_in(conn)?;
        targets.extend(self.entity_targets_in(conn)?);
        units.extend(targets.into_iter().map(|(id, text)| {
            ShadowUnit {
                id,
                blueprint: false,
                texts: crate::embeddings::chunk_text(&text)
                    .into_iter()
                    .enumerate()
                    .map(|(ordinal, text)| (String::new(), ordinal, text))
                    .collect(),
            }
        }));
        Ok(units)
    }

    pub(super) fn prepare_shadow_in(
        &self,
        conn: &Connection,
        identity: &EmbedderIdentity,
    ) -> Result<(), String> {
        let stamp = serde_json::to_string(identity).map_err(|error| error.to_string())?;
        let attached: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_database_list WHERE name='reembed')",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !attached {
            let path = shadow_path(&conn)?;
            conn.execute("ATTACH DATABASE ?1 AS reembed", [path])
                .map_err(|error| error.to_string())?;
        }
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS reembed.shadow (target TEXT NOT NULL, unit_key TEXT NOT NULL, text_hash BLOB NOT NULL, vectors BLOB, PRIMARY KEY(target, unit_key))",
        )
        .map_err(|error| error.to_string())?;
        conn.execute(
            "DELETE FROM reembed.shadow WHERE target != ?1 OR vectors IS NULL",
            [stamp],
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub(super) fn clear_shadow_in(&self, conn: &Connection) {
        let path = shadow_path(conn);
        let _ = conn.execute_batch("DETACH DATABASE reembed");
        if let Some(path) = path.ok().filter(|path| !path.is_empty()) {
            for suffix in ["", "-journal", "-wal", "-shm"] {
                let _ = std::fs::remove_file(format!("{path}{suffix}"));
            }
        }
    }

    pub(super) fn publish_shadow_in(
        &self,
        target: &ActiveEmbedder,
        active: &ActiveEmbedder,
        stamp: &str,
        claim: &dyn Fn() -> bool,
    ) -> Result<bool, String> {
        let identity = target.identity();
        if stamp != serde_json::to_string(&identity).map_err(|e| e.to_string())? {
            return Err("shadow target stamp mismatch".into());
        }
        let width = identity
            .dimension
            .checked_mul(4)
            .filter(|width| *width > 0)
            .ok_or("invalid shadow vector dimension")?;
        let conn = self.conn.lock();
        let tx =
            rusqlite::Transaction::new_unchecked(&conn, rusqlite::TransactionBehavior::Immediate)
                .map_err(|error| error.to_string())?;
        let mut ready = Vec::new();
        for unit in self.shadow_units(&tx)? {
            match checkpoint(&tx, stamp, &unit.key())? {
                Some((hash, vectors)) if hash == unit.hash() => ready.push((unit, vectors)),
                _ => return Ok(false),
            }
        }
        if !ready.is_empty() && ready.iter().all(|(_, vectors)| vectors.is_none()) {
            return Err("the target embedder produced no vectors".into());
        }
        for (unit, vectors) in &ready {
            if let Some(vectors) = vectors {
                if Some(vectors.len()) != unit.texts.len().checked_mul(width) {
                    return Err("shadow vector count or dimension mismatch".into());
                }
            }
        }
        index::recreate_in(&tx, &identity)?;
        tx.execute("DELETE FROM blueprint_vectors", [])
            .map_err(|error| error.to_string())?;
        for (unit, vectors) in &ready {
            let Some(vectors) = vectors else {
                if !unit.blueprint {
                    pending::mark(&tx, &unit.id, "re-embed failed for this item")?;
                }
                continue;
            };
            for ((slot, ordinal, text), bytes) in unit.texts.iter().zip(vectors.chunks(width)) {
                if unit.blueprint {
                    tx.execute("INSERT INTO blueprint_vectors(item_id,slot,ordinal,text,embedding,provider,model,dimension) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)", params![unit.id,slot,*ordinal as i64,text,bytes,identity.provider,identity.model,identity.dimension as i64]).map_err(|error| error.to_string())?;
                } else {
                    tx.execute(
                        "INSERT INTO mem_vectors(mem_id,embedding) VALUES (?1,?2)",
                        params![index::vector_id(&unit.id, *ordinal), bytes],
                    )
                    .map_err(|error| error.to_string())?;
                }
            }
        }
        if !claim() {
            return Err("re-embed cancelled before publication".into());
        }
        tx.commit().map_err(|error| error.to_string())?;
        // Swapped under the connection lock: every stamp-checked reader and writer sees model and index change together.
        active.replace_silent(target.provider());
        self.clear_shadow_in(&conn);
        Ok(true)
    }
}
fn shadow_path(conn: &Connection) -> Result<String, String> {
    let main: String = conn
        .query_row(
            "SELECT file FROM pragma_database_list WHERE name='main'",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    Ok(if main.is_empty() {
        main
    } else {
        format!("{main}.reembed")
    })
}
