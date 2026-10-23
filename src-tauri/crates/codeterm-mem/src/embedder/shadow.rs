use std::sync::{mpsc, Arc};
use std::time::Duration;

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::{index, ActiveEmbedder, EmbedderIdentity};
use crate::MemManager;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Unit {
    id: String,
    blueprint: bool,
    texts: Vec<(String, usize, String)>,
}

#[derive(Serialize, Deserialize)]
struct Checkpoint {
    unit: Unit,
    vectors: Vec<Vec<f32>>,
}

impl Unit {
    fn key(&self) -> String {
        format!("{}:{}", if self.blueprint { "b" } else { "m" }, self.id)
    }
}

impl MemManager {
    fn shadow_revision(&self) -> Result<(i64, i64), String> {
        self.conn
            .lock()
            .query_row(
                "SELECT revision, data_version FROM mem_reembed_revision, pragma_data_version",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|error| error.to_string())
    }

    fn shadow_units(&self) -> Result<Vec<Unit>, String> {
        let blueprints = crate::blueprint_library::index::indexed_items(&self.conn.lock())?;
        let mut units = Vec::new();
        for id in blueprints {
            units.push(Unit {
                texts: self.blueprint_vector_texts(&id)?,
                id,
                blueprint: true,
            });
        }
        let mut targets = self.reembed_item_targets()?;
        targets.extend(self.reembed_entity_targets()?);
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

    pub(super) fn prepare_shadow(&self, identity: &EmbedderIdentity) -> Result<(), String> {
        let identity = serde_json::to_string(identity).map_err(|error| error.to_string())?;
        let conn = self.conn.lock();
        conn.execute_batch("CREATE TABLE IF NOT EXISTS mem_reembed_shadow (target TEXT NOT NULL, unit_key TEXT NOT NULL, checkpoint TEXT NOT NULL, PRIMARY KEY(target,unit_key))").map_err(|error| error.to_string())?;
        conn.execute_batch("CREATE TABLE IF NOT EXISTS mem_reembed_revision (singleton INTEGER PRIMARY KEY CHECK(singleton=1), revision INTEGER NOT NULL); INSERT OR IGNORE INTO mem_reembed_revision VALUES (1,0);").map_err(|error| error.to_string())?;
        for table in [
            "mem_items",
            "entities",
            "blueprint_index",
            "solution_metadata",
        ] {
            for operation in ["INSERT", "UPDATE", "DELETE"] {
                let columns = if table == "mem_items" && operation == "UPDATE" {
                    " OF kind,title,summary,area,tags_json,body_inline,body_blob_path,source,source_path"
                } else {
                    ""
                };
                let id = if matches!(table, "blueprint_index" | "solution_metadata") {
                    "item_id"
                } else {
                    "id"
                };
                let invalidate = if operation == "INSERT" {
                    String::new()
                } else {
                    format!("DELETE FROM mem_reembed_shadow WHERE unit_key IN ('m:' || OLD.{id}, 'b:' || OLD.{id});")
                };
                let condition = if table == "solution_metadata" && operation == "UPDATE" {
                    " WHEN json_extract(NEW.metadata_json,'$.blueprint.intents') IS NOT json_extract(OLD.metadata_json,'$.blueprint.intents')"
                } else {
                    ""
                };
                conn.execute_batch(&format!("CREATE TRIGGER IF NOT EXISTS reembed_{table}_{operation} AFTER {operation}{columns} ON {table}{condition} BEGIN UPDATE mem_reembed_revision SET revision=revision+1; {invalidate} END;")).map_err(|error| error.to_string())?;
            }
        }
        conn.execute(
            "DELETE FROM mem_reembed_shadow WHERE target != ?1",
            [&identity],
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub(super) fn run_shadow(
        self: &Arc<Self>,
        generation: u64,
        target: Arc<ActiveEmbedder>,
        changed: &mpsc::Sender<()>,
    ) -> Result<(), String> {
        let identity = target.identity();
        let stamp = serde_json::to_string(&identity).map_err(|error| error.to_string())?;
        let units = self.shadow_units()?;
        for unit in &units {
            if !self.reembed.is_current(generation) {
                return Err("re-embed cancelled before next item".into());
            }
            let cached: Option<String> = self
                .conn
                .lock()
                .query_row(
                    "SELECT checkpoint FROM mem_reembed_shadow WHERE target=?1 AND unit_key=?2",
                    params![stamp, unit.key()],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| error.to_string())?;
            let ready = cached
                .as_deref()
                .and_then(|json| serde_json::from_str::<Checkpoint>(json).ok())
                .is_some_and(|checkpoint| {
                    checkpoint.unit == *unit
                        && checkpoint.vectors.len() == unit.texts.len()
                        && checkpoint.vectors.iter().all(|vector| {
                            super::provider::validate_vector(vector, identity.dimension).is_ok()
                        })
                });
            if !ready {
                let texts = unit
                    .texts
                    .iter()
                    .map(|(_, _, text)| text.clone())
                    .collect::<Vec<_>>();
                let vectors = target
                    .passages(&texts)
                    .into_iter()
                    .map(|result| {
                        let vector = result.map_err(|error| error.to_string())?;
                        if vector.identity != identity {
                            return Err("mixed-model shadow vector refused".into());
                        }
                        super::provider::validate_vector(&vector.vector, identity.dimension)
                            .map_err(|error| error.to_string())?;
                        Ok(vector.vector)
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                let checkpoint = serde_json::to_string(&Checkpoint {
                    unit: unit.clone(),
                    vectors,
                })
                .map_err(|error| error.to_string())?;
                let written = self.conn.lock().execute("INSERT INTO mem_reembed_shadow(target,unit_key,checkpoint) SELECT ?1,?2,?3 WHERE EXISTS(SELECT 1 FROM mem_items WHERE id=?4 UNION ALL SELECT 1 FROM entities WHERE id=?4) ON CONFLICT(target,unit_key) DO UPDATE SET checkpoint=excluded.checkpoint", params![stamp,unit.key(),checkpoint,unit.id]).map_err(|error| error.to_string())?;
                if written == 0 {
                    return Err(format!("{} disappeared during re-embed", unit.id));
                }
            }
            if !self.reembed.advance(generation, Ok(()), &self.embedder) {
                return Err("re-embed cancelled after item".into());
            }
            let _ = changed.send(());
        }
        let docs = if let Some(data_dir) = &self.docs_dir {
            Some(crate::docs_source::prepare_reembed(
                data_dir,
                &target,
                || {
                    self.reembed.advance(generation, Ok(()), &self.embedder);
                    let _ = changed.send(());
                },
            )?)
        } else {
            None
        };
        if !self.reembed.is_current(generation) {
            return Err("re-embed cancelled before publication".into());
        }
        // Mutations require a new stamped pass rather than publishing stale semantic data.
        let before = self.shadow_revision()?;
        if self.shadow_units()? != units {
            return Err("memory changed during re-embed; retry to resume checkpointed work".into());
        }
        let _publication = self.vector_transition.write();
        if !self.reembed.is_current(generation) {
            return Err("re-embed cancelled before atomic swap".into());
        }
        {
            let conn = self.conn.lock();
            let tx = rusqlite::Transaction::new_unchecked(
                &conn,
                rusqlite::TransactionBehavior::Immediate,
            )
            .map_err(|error| error.to_string())?;
            let after: (i64, i64) = tx
                .query_row(
                    "SELECT revision, data_version FROM mem_reembed_revision, pragma_data_version",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(|error| error.to_string())?;
            if before != after {
                return Err(
                    "memory changed before publication; retry to resume checkpointed work".into(),
                );
            }
            index::recreate_in(&tx, &identity)?;
            tx.execute("DELETE FROM blueprint_vectors", [])
                .map_err(|error| error.to_string())?;
            for unit in &units {
                let json: String = tx
                    .query_row(
                        "SELECT checkpoint FROM mem_reembed_shadow WHERE target=?1 AND unit_key=?2",
                        params![stamp, unit.key()],
                        |row| row.get(0),
                    )
                    .map_err(|error| error.to_string())?;
                let checkpoint: Checkpoint =
                    serde_json::from_str(&json).map_err(|error| error.to_string())?;
                for ((slot, ordinal, text), vector) in unit.texts.iter().zip(checkpoint.vectors) {
                    let bytes = index::encode(&vector);
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
            tx.execute("DELETE FROM mem_reembed_shadow", [])
                .map_err(|error| error.to_string())?;
            tx.commit().map_err(|error| error.to_string())?;
        }
        self.embedder.replace_silent(target.provider());
        if let Some(docs) = docs {
            crate::docs_source::publish_reembed(docs, &self.embedder);
        }
        drop(_publication);
        self.embedder
            .emit(&super::EmbedderEvent::Switched(identity));
        self.reembed.finish(generation, &self.embedder);
        Ok(())
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
                manager
                    .reembed_busy
                    .store(false, std::sync::atomic::Ordering::SeqCst);
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
            .spawn(move || loop {
                match progress.recv_timeout(Duration::from_secs(180)) {
                    Ok(()) => {}
                    Err(error) => {
                        manager.reembed.fail(
                            generation,
                            format!("re-embed worker stopped advancing: {error}"),
                            &manager.embedder,
                        );
                        return;
                    }
                }
                if !manager.reembed.is_current(generation) {
                    return;
                }
            })
            .map_err(|error| format!("start re-embed watchdog: {error}"))?;
        Ok(())
    }
}
