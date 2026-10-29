use super::*;
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;

pub trait RunMediaResolver: Send + Sync {
    fn fingerprint(&self, plugin: &str, media_id: &str, kind: PortKind) -> Result<String, String>;
}

pub trait StepMedia: Send + Sync {
    fn input(
        &self,
        context: &StepContext,
        receiver: &str,
        handle: &str,
        kind: PortKind,
    ) -> Result<InputLineage, String>;
    fn publish(
        &self,
        context: &StepContext,
        producer: &str,
        port: &str,
        id: &str,
        kind: PortKind,
    ) -> Result<String, String>;
}

pub struct RunMediaRegistry {
    db: Mutex<Connection>,
    resolver: Arc<dyn RunMediaResolver>,
}

#[derive(Serialize, Deserialize)]
struct Reference {
    scope: RunScope,
    lineage: InputLineage,
    media_id: String,
    kind: PortKind,
    fingerprint: String,
}

fn refusal(_: rusqlite::Error) -> String {
    "run media persistence unavailable".into()
}

impl RunMediaRegistry {
    pub fn open(
        path: &std::path::Path,
        resolver: Arc<dyn RunMediaResolver>,
    ) -> Result<Self, String> {
        let db = Connection::open(path).map_err(refusal)?;
        db.execute_batch("PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS blueprint_run_media (handle TEXT PRIMARY KEY, source_key TEXT UNIQUE NOT NULL, scope TEXT NOT NULL, record TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS blueprint_media_handoffs (handle TEXT NOT NULL, receiver TEXT NOT NULL, node TEXT NOT NULL, PRIMARY KEY(handle,receiver,node));").map_err(refusal)?;
        Ok(Self {
            db: Mutex::new(db),
            resolver,
        })
    }

    pub fn authorize_handoff(
        &self,
        scope: &RunScope,
        handle: &str,
        receiver: &str,
        node: &str,
    ) -> Result<(), String> {
        let reference = self.reference(handle)?;
        if &reference.scope != scope || !token(receiver) || !token(node) {
            return Err("run media handoff refused".into());
        }
        self.fresh(&reference)?;
        let mut db = self.db.lock();
        let tx = db.transaction().map_err(refusal)?;
        let count: u64 = tx
            .query_row(
                "SELECT count(*) FROM blueprint_media_handoffs WHERE handle=?1",
                [handle],
                |r| r.get(0),
            )
            .map_err(refusal)?;
        if count >= 64 {
            return Err("run media handoff limit reached".into());
        }
        tx.execute(
            "INSERT OR IGNORE INTO blueprint_media_handoffs VALUES (?1,?2,?3)",
            params![handle, receiver, node],
        )
        .map_err(refusal)?;
        tx.commit().map_err(refusal)?;
        Ok(())
    }

    fn reference(&self, handle: &str) -> Result<Reference, String> {
        let record: Option<String> = self
            .db
            .lock()
            .query_row(
                "SELECT record FROM blueprint_run_media WHERE handle=?1",
                [handle],
                |r| r.get(0),
            )
            .optional()
            .map_err(refusal)?;
        serde_json::from_str(&record.ok_or("run media handle unavailable")?)
            .map_err(|_| "run media record invalid".into())
    }

    fn fresh(&self, reference: &Reference) -> Result<(), String> {
        let fingerprint = self
            .resolver
            .fingerprint(
                &reference.lineage.plugin,
                &reference.media_id,
                reference.kind,
            )
            .map_err(|_| "run media source unavailable")?;
        if fingerprint != reference.fingerprint {
            return Err("run media source changed".into());
        }
        Ok(())
    }
}

impl StepMedia for RunMediaRegistry {
    fn input(
        &self,
        context: &StepContext,
        receiver: &str,
        handle: &str,
        kind: PortKind,
    ) -> Result<InputLineage, String> {
        let reference = self.reference(handle)?;
        if reference.scope != context.scope || reference.kind != kind {
            return Err("run media scope/type refused".into());
        }
        let permitted: bool = self.db.lock().query_row("SELECT EXISTS(SELECT 1 FROM blueprint_media_handoffs WHERE handle=?1 AND receiver=?2 AND node=?3)", params![handle, receiver, context.node], |r| r.get(0)).map_err(refusal)?;
        if !permitted {
            return Err("run media receiving step not authorized".into());
        }
        self.fresh(&reference)?;
        Ok(reference.lineage)
    }

    fn publish(
        &self,
        context: &StepContext,
        producer: &str,
        port: &str,
        id: &str,
        kind: PortKind,
    ) -> Result<String, String> {
        let fingerprint = self
            .resolver
            .fingerprint(producer, id, kind)
            .map_err(|_| "run media output unavailable")?;
        if fingerprint.len() != 64 || !fingerprint.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("run media fingerprint invalid".into());
        }
        let reference = Reference {
            scope: context.scope.clone(),
            lineage: InputLineage {
                plugin: producer.into(),
                operation: context.operation.clone(),
                port: port.into(),
            },
            media_id: id.into(),
            kind,
            fingerprint,
        };
        let handle = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let record =
            serde_json::to_string(&reference).map_err(|_| "invalid run media reference")?;
        let source_key = Sha256::digest(record.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let scope = serde_json::to_string(&context.scope).map_err(|_| "invalid run media scope")?;
        let mut db = self.db.lock();
        let tx = db.transaction().map_err(refusal)?;
        if let Some(existing) = tx
            .query_row(
                "SELECT handle FROM blueprint_run_media WHERE source_key=?1",
                [&source_key],
                |r| r.get::<_, String>(0),
            )
            .optional()
            .map_err(refusal)?
        {
            return Ok(existing);
        }
        let count: u64 = tx
            .query_row(
                "SELECT count(*) FROM blueprint_run_media WHERE scope=?1",
                [&scope],
                |r| r.get(0),
            )
            .map_err(refusal)?;
        if count >= 16_384 {
            return Err("run media reference limit reached".into());
        }
        tx.execute(
            "INSERT INTO blueprint_run_media VALUES (?1,?2,?3,?4)",
            params![handle, source_key, scope, record],
        )
        .map_err(refusal)?;
        tx.commit().map_err(refusal)?;
        Ok(handle)
    }
}
