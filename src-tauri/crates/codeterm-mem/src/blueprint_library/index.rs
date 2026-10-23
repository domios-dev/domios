use crate::blueprint_wl::Signature;
use crate::embedder::EmbedderIdentity;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(crate) fn initialize(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS blueprint_index (item_id TEXT PRIMARY KEY REFERENCES mem_items(id) ON DELETE CASCADE, document_hash TEXT NOT NULL, canon TEXT NOT NULL, gists TEXT NOT NULL, signature_json TEXT NOT NULL, enrichment_json TEXT);
         CREATE TABLE IF NOT EXISTS blueprint_vectors (item_id TEXT NOT NULL REFERENCES mem_items(id) ON DELETE CASCADE, slot TEXT NOT NULL, ordinal INTEGER NOT NULL, text TEXT NOT NULL, embedding BLOB NOT NULL, provider TEXT NOT NULL, model TEXT NOT NULL, dimension INTEGER NOT NULL, PRIMARY KEY (item_id, slot, ordinal));",
    )
    .map_err(|error| error.to_string())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub model: String,
    pub document_hash: String,
    pub created_at: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Enrichment {
    pub summary: String,
    pub intents: Vec<String>,
    pub provenance: Provenance,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IndexRow {
    pub document_hash: String,
    pub canon: String,
    pub gists: String,
    pub signature: Signature,
    pub enrichment: Option<Enrichment>,
}

impl IndexRow {
    pub fn current_enrichment(&self) -> Option<&Enrichment> {
        self.enrichment
            .as_ref()
            .filter(|enrichment| enrichment.provenance.document_hash == self.document_hash)
    }

    pub fn enrichment_pending(&self) -> bool {
        self.current_enrichment().is_none()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Slot {
    Intent,
    Canon,
    WorkerIntent,
    Summary,
}

impl Slot {
    fn name(self) -> &'static str {
        match self {
            Self::Intent => "intent",
            Self::Canon => "canon",
            Self::WorkerIntent => "worker_intent",
            Self::Summary => "summary",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        [Self::Intent, Self::Canon, Self::WorkerIntent, Self::Summary]
            .into_iter()
            .find(|slot| slot.name() == name)
    }
}

pub type VectorKey = (Slot, usize);

#[derive(Clone, Debug, Default)]
pub struct Vectors {
    pub by_slot: BTreeMap<VectorKey, (String, Vec<f32>)>,
}

impl Vectors {
    pub fn slot(&self, slot: Slot) -> impl Iterator<Item = (&str, &[f32])> {
        self.by_slot
            .iter()
            .filter(move |((kind, _), _)| *kind == slot)
            .map(|(_, (text, vector))| (text.as_str(), vector.as_slice()))
    }
}

pub(crate) fn write_row(conn: &Connection, item_id: &str, row: &IndexRow) -> Result<(), String> {
    conn.execute(
        "INSERT INTO blueprint_index(item_id,document_hash,canon,gists,signature_json,enrichment_json) VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(item_id) DO UPDATE SET document_hash=excluded.document_hash, canon=excluded.canon, gists=excluded.gists, signature_json=excluded.signature_json, enrichment_json=excluded.enrichment_json",
        params![
            item_id,
            row.document_hash,
            row.canon,
            row.gists,
            serde_json::to_string(&row.signature).map_err(|error| error.to_string())?,
            row.enrichment
                .as_ref()
                .map(serde_json::to_string)
                .transpose()
                .map_err(|error| error.to_string())?,
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

pub(crate) fn read_row(conn: &Connection, item_id: &str) -> Result<Option<IndexRow>, String> {
    let raw = conn
        .query_row(
            "SELECT document_hash,canon,gists,signature_json,enrichment_json FROM blueprint_index WHERE item_id=?1",
            [item_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            },
        )
        .optional()
        .map_err(|error| error.to_string())?;
    raw.map(|(document_hash, canon, gists, signature, enrichment)| {
        Ok(IndexRow {
            document_hash,
            canon,
            gists,
            signature: serde_json::from_str(&signature)
                .map_err(|error| format!("invalid Blueprint signature for {item_id}: {error}"))?,
            enrichment: enrichment
                .map(|json| serde_json::from_str(&json))
                .transpose()
                .map_err(|error| format!("invalid Blueprint enrichment for {item_id}: {error}"))?,
        })
    })
    .transpose()
}

pub(crate) fn indexed_items(conn: &Connection) -> Result<Vec<String>, String> {
    let mut statement = conn
        .prepare("SELECT b.item_id FROM blueprint_index b JOIN mem_items m ON m.id=b.item_id WHERE m.kind='skill' AND m.source=?1 ORDER BY b.item_id")
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([super::SOURCE], |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

pub(crate) fn read_vectors(
    conn: &Connection,
    item_id: &str,
    identity: &EmbedderIdentity,
) -> Result<Vectors, String> {
    let mut statement = conn
        .prepare("SELECT slot,ordinal,text,embedding FROM blueprint_vectors WHERE item_id=?1 AND provider=?2 AND model=?3 AND dimension=?4")
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(
            params![
                item_id,
                identity.provider,
                identity.model,
                identity.dimension as i64
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Vec<u8>>(3)?,
                ))
            },
        )
        .map_err(|error| error.to_string())?;
    let mut vectors = Vectors::default();
    for row in rows {
        let (slot, ordinal, text, blob) = row.map_err(|error| error.to_string())?;
        let (Some(slot), Ok(ordinal)) = (Slot::parse(&slot), usize::try_from(ordinal)) else {
            continue;
        };
        if let Some(vector) = decode(&blob, identity.dimension) {
            vectors.by_slot.insert((slot, ordinal), (text, vector));
        }
    }
    Ok(vectors)
}

pub(crate) fn replace_vectors(
    conn: &Connection,
    item_id: &str,
    embedder: &crate::embedder::ActiveEmbedder,
    identity: &EmbedderIdentity,
    keep: &[VectorKey],
    written: &[(VectorKey, String, Vec<f32>)],
) -> Result<(), String> {
    if embedder.identity() != *identity {
        return Err("mixed-model Blueprint vectors refused".into());
    }
    // The embedding worker shares this WAL database; acquire the writer before
    // reading so its commit cannot invalidate a deferred read-to-write upgrade.
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let live: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM blueprint_index WHERE item_id=?1)",
            [item_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if !live {
        return Ok(());
    }
    tx.execute(
        "DELETE FROM blueprint_vectors WHERE item_id=?1 AND (provider!=?2 OR model!=?3 OR dimension!=?4)",
        params![item_id, identity.provider, identity.model, identity.dimension as i64],
    ).map_err(|error| error.to_string())?;
    let existing = read_vectors(&tx, item_id, identity)?;
    for key in existing.by_slot.keys() {
        if !keep.contains(key) {
            tx.execute(
                "DELETE FROM blueprint_vectors WHERE item_id=?1 AND slot=?2 AND ordinal=?3",
                params![item_id, key.0.name(), key.1 as i64],
            )
            .map_err(|error| error.to_string())?;
        }
    }
    for ((slot, ordinal), text, vector) in written {
        crate::embedder::provider::validate_vector(vector, identity.dimension)
            .map_err(|error| error.to_string())?;
        tx.execute(
            "INSERT INTO blueprint_vectors(item_id,slot,ordinal,text,embedding,provider,model,dimension) VALUES (?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(item_id,slot,ordinal) DO UPDATE SET text=excluded.text, embedding=excluded.embedding, provider=excluded.provider, model=excluded.model, dimension=excluded.dimension",
            params![item_id, slot.name(), *ordinal as i64, text, encode(vector), identity.provider, identity.model, identity.dimension as i64],
        )
        .map_err(|error| error.to_string())?;
    }
    tx.commit().map_err(|error| error.to_string())
}

pub(crate) fn valid_dimension(vector: &[f32], dimension: usize) -> bool {
    dimension > 0 && vector.len() == dimension && vector.iter().all(|value| value.is_finite())
}

fn encode(vector: &[f32]) -> Vec<u8> {
    vector
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn decode(blob: &[u8], dimension: usize) -> Option<Vec<f32>> {
    if blob.len() != dimension.checked_mul(4)? {
        return None;
    }
    let vector = blob
        .as_chunks::<4>()
        .0
        .iter()
        .copied()
        .map(f32::from_le_bytes)
        .collect::<Vec<_>>();
    valid_dimension(&vector, dimension).then_some(vector)
}

pub fn cosine(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() {
        return 0.0;
    }
    let dot: f64 = a
        .iter()
        .zip(b)
        .map(|(x, y)| f64::from(*x) * f64::from(*y))
        .sum();
    let norm = |vector: &[f32]| {
        vector
            .iter()
            .map(|value| f64::from(*value).powi(2))
            .sum::<f64>()
            .sqrt()
    };
    let denominator = norm(a) * norm(b);
    if denominator > 0.0 {
        dot / denominator
    } else {
        0.0
    }
}
