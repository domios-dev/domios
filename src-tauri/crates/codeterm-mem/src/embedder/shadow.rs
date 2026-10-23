use std::sync::{mpsc, Arc};
use std::time::Duration;

use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};

use super::{index, pending, ActiveEmbedder, EmbedError, EmbedderIdentity};
use crate::MemManager;

/// Resume passes per run; each pass embeds only units changed since the last.
const PASSES: usize = 8;
/// Consecutive unavailable units that mean the provider itself is down.
const OUTAGE_STREAK: usize = 5;

pub(crate) fn watch_progress(
    mut receive: impl FnMut(Duration) -> Result<(), String>,
    current: impl Fn() -> bool,
    fail: impl Fn(String),
) {
    loop {
        if let Err(error) = receive(Duration::from_secs(180)) {
            fail(format!("re-embed worker stopped advancing: {error}"));
            return;
        }
        if !current() {
            return;
        }
    }
}

struct Unit {
    id: String,
    blueprint: bool,
    texts: Vec<(String, usize, String)>,
}

impl Unit {
    fn key(&self) -> String {
        format!("{}:{}", if self.blueprint { "b" } else { "m" }, self.id)
    }

    fn hash(&self) -> Vec<u8> {
        let mut hasher = Sha256::new();
        for (slot, ordinal, text) in &self.texts {
            hasher.update(slot.as_bytes());
            hasher.update([0]);
            hasher.update(ordinal.to_le_bytes());
            hasher.update(text.as_bytes());
            hasher.update([0]);
        }
        hasher.finalize().to_vec()
    }
}

type Checkpoint = (Vec<u8>, Option<Vec<u8>>);

fn checkpoint(conn: &Connection, stamp: &str, key: &str) -> Result<Option<Checkpoint>, String> {
    conn.query_row(
        "SELECT text_hash, vectors FROM reembed.shadow WHERE target=?1 AND unit_key=?2",
        params![stamp, key],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()
    .map_err(|error| error.to_string())
}

fn embed_unit(
    target: &ActiveEmbedder,
    identity: &EmbedderIdentity,
    unit: &Unit,
) -> Result<Vec<u8>, EmbedError> {
    let texts = unit
        .texts
        .iter()
        .map(|(_, _, text)| text.clone())
        .collect::<Vec<_>>();
    let mut blob = Vec::with_capacity(texts.len() * identity.dimension * 4);
    for result in target.passages(&texts) {
        let vector = result?;
        if vector.identity != *identity {
            return Err(EmbedError::Refused("mixed-model shadow vector".into()));
        }
        super::provider::validate_vector(&vector.vector, identity.dimension)?;
        blob.extend(index::encode(&vector.vector));
    }
    Ok(blob)
}

impl MemManager {
    fn shadow_units(&self, conn: &Connection) -> Result<Vec<Unit>, String> {
        let mut units = Vec::new();
        for id in crate::blueprint_library::index::indexed_items(conn)? {
            units.push(Unit {
                texts: crate::blueprint_library::enrichment::blueprint_vector_texts(conn, &id)?,
                id,
                blueprint: true,
            });
        }
        let mut targets = self.reembed_item_targets(conn)?;
        targets.extend(self.reembed_entity_targets(conn)?);
        units.extend(targets.into_iter().map(|(id, text)| {
            Unit {
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

    /// Checkpoints live in a sibling database file so publication can delete
    /// them without leaving knowledge.db at a high-water mark.
    pub(super) fn prepare_shadow(&self, identity: &EmbedderIdentity) -> Result<(), String> {
        let stamp = serde_json::to_string(identity).map_err(|error| error.to_string())?;
        let conn = self.conn.lock();
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

    pub(super) fn clear_shadow(&self, conn: &Connection) {
        let path = shadow_path(conn);
        let _ = conn.execute_batch("DETACH DATABASE reembed");
        if let Some(path) = path.ok().filter(|path| !path.is_empty()) {
            for suffix in ["", "-journal", "-wal", "-shm"] {
                let _ = std::fs::remove_file(format!("{path}{suffix}"));
            }
        }
    }

    pub(super) fn run_shadow(
        self: &Arc<Self>,
        generation: u64,
        target: Arc<ActiveEmbedder>,
        changed: &mpsc::Sender<()>,
    ) -> Result<(), String> {
        let identity = target.identity();
        let stamp = serde_json::to_string(&identity).map_err(|error| error.to_string())?;
        let mut docs = None;
        for pass in 0..PASSES {
            let _ = changed.send(());
            let stale = {
                let conn = self.conn.lock();
                let units = self.shadow_units(&conn)?;
                let total = units.len() as u64;
                let mut stale = Vec::new();
                for unit in units {
                    if checkpoint(&conn, &stamp, &unit.key())?
                        .is_none_or(|(hash, _)| hash != unit.hash())
                    {
                        stale.push(unit);
                    }
                }
                let stale_count = stale.len() as u64;
                if pass == 0 {
                    self.reembed
                        .resume(generation, total - stale_count, 0, &self.embedder);
                } else {
                    self.reembed
                        .resume(generation, 0, stale_count, &self.embedder);
                }
                stale
            };
            let _ = changed.send(());
            let mut streak = 0;
            let mut unavailable = Vec::new();
            for unit in &stale {
                if !self.reembed.is_current(generation) {
                    return Err("re-embed cancelled".into());
                }
                let (vectors, error) = match embed_unit(&target, &identity, unit) {
                    Ok(blob) => {
                        streak = 0;
                        (Some(blob), None)
                    }
                    Err(EmbedError::NotBuilt(reason)) => return Err(reason),
                    Err(EmbedError::Unavailable(reason)) => {
                        streak += 1;
                        if streak >= OUTAGE_STREAK {
                            return Err(format!(
                                "provider unavailable for {streak} consecutive items: {reason}"
                            ));
                        }
                        unavailable.push(unit);
                        (None, Some(format!("{}: {reason}", unit.id)))
                    }
                    Err(EmbedError::Refused(reason)) => {
                        (None, Some(format!("{}: {reason}", unit.id)))
                    }
                };
                {
                    let conn = self.conn.lock();
                    if !self.reembed.is_current(generation) {
                        return Err("re-embed cancelled".into());
                    }
                    conn.execute(
                        "INSERT OR REPLACE INTO reembed.shadow(target,unit_key,text_hash,vectors) VALUES (?1,?2,?3,?4)",
                        params![stamp, unit.key(), unit.hash(), vectors],
                    )
                    .map_err(|error| error.to_string())?;
                }
                if !self
                    .reembed
                    .advance(generation, error.map_or(Ok(()), Err), &self.embedder)
                {
                    return Err("re-embed cancelled".into());
                }
                let _ = changed.send(());
            }
            for unit in unavailable {
                if !self.reembed.is_current(generation) {
                    return Err("re-embed cancelled".into());
                }
                if let Ok(blob) = embed_unit(&target, &identity, unit) {
                    let conn = self.conn.lock();
                    if !self.reembed.is_current(generation) {
                        return Err("re-embed cancelled".into());
                    }
                    conn.execute(
                        "UPDATE reembed.shadow SET vectors=?3 WHERE target=?1 AND unit_key=?2",
                        params![stamp, unit.key(), blob],
                    )
                    .map_err(|error| error.to_string())?;
                    self.reembed.recovered(generation);
                }
                let _ = changed.send(());
            }
            if docs.is_none() {
                if let Some(data_dir) = &self.docs_dir {
                    docs = Some(crate::docs_source::prepare_reembed(
                        data_dir,
                        &target,
                        || {
                            self.reembed.advance(generation, Ok(()), &self.embedder);
                            let _ = changed.send(());
                        },
                    )?);
                }
            }
            let _ = changed.send(());
            let published = self.publish_shadow(generation, &target, &stamp)?;
            let _ = changed.send(());
            if published {
                if let Some(docs) = docs {
                    crate::docs_source::publish_reembed(docs, &self.embedder);
                }
                self.embedder
                    .emit(&super::EmbedderEvent::Switched(identity));
                self.reembed.finish(generation, &self.embedder);
                return Ok(());
            }
        }
        Err(format!(
            "memory kept changing across {PASSES} re-embed passes; switch again to resume"
        ))
    }

    /// Diffs the live units against the checkpoints inside one IMMEDIATE
    /// transaction; `false` means a unit changed and another pass is needed.
    fn publish_shadow(
        &self,
        generation: u64,
        target: &ActiveEmbedder,
        stamp: &str,
    ) -> Result<bool, String> {
        let identity = target.identity();
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
        index::recreate_in(&tx, &identity)?;
        tx.execute("DELETE FROM blueprint_vectors", [])
            .map_err(|error| error.to_string())?;
        let width = identity.dimension * 4;
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
        if !self.reembed.claim(generation) {
            return Err("re-embed cancelled before publication".into());
        }
        tx.commit().map_err(|error| error.to_string())?;
        // Swapped under the connection lock: every stamp-checked reader and writer sees model and index change together.
        self.embedder.replace_silent(target.provider());
        self.clear_shadow(&conn);
        Ok(true)
    }

    pub(super) fn launch_shadow(
        self: &Arc<Self>,
        generation: u64,
        target: Arc<ActiveEmbedder>,
    ) -> Result<(), String> {
        let (changed, progress) = mpsc::channel();
        let manager = self.clone();
        std::thread::Builder::new()
            .name("mem-reembed".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    manager.run_shadow(generation, target, &changed)
                }));
                match result {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => manager.reembed.fail(generation, error, &manager.embedder),
                    Err(_) => manager.reembed.fail(
                        generation,
                        "re-embed worker panicked".into(),
                        &manager.embedder,
                    ),
                }
            })
            .map_err(|error| format!("start shadow worker: {error}"))?;
        let manager = self.clone();
        std::thread::Builder::new()
            .name("mem-reembed-watchdog".into())
            .spawn(move || {
                watch_progress(
                    |timeout| {
                        progress
                            .recv_timeout(timeout)
                            .map_err(|error| error.to_string())
                    },
                    || manager.reembed.is_current(generation),
                    |error| manager.reembed.fail(generation, error, &manager.embedder),
                );
            })
            .map_err(|error| format!("start re-embed watchdog: {error}"))?;
        Ok(())
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
