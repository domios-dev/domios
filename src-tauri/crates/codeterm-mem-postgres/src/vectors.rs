use crate::executor::Transaction;
use crate::{db_error, PostgresMemStore, StoreResult};
use codeterm_mem::embedder::{
    index::{vector_id, InsertOutcome},
    EmbedderIdentity, StampedVector,
};
use std::collections::BTreeMap;

pub(crate) fn decode(bytes: &[u8]) -> StoreResult<Vec<f32>> {
    let (chunks, remainder) = bytes.as_chunks::<4>();
    if !remainder.is_empty() {
        return Err("invalid vector byte count".into());
    }
    let vector: Vec<f32> = chunks.iter().map(|c| f32::from_le_bytes(*c)).collect();
    if vector.iter().any(|v| !v.is_finite()) {
        return Err("vector has a non-finite value".into());
    }
    Ok(vector)
}

pub(crate) fn stamp_in(tx: &mut Transaction<'_>) -> StoreResult<Option<EmbedderIdentity>> {
    tx.query_opt(
        "SELECT provider,model,dimension FROM mem_embedder WHERE singleton=1",
        &[],
    )
    .map_err(db_error)?
    .map(|row| {
        Ok(EmbedderIdentity {
            provider: row.get(0),
            model: row.get(1),
            dimension: usize::try_from(row.get::<_, i64>(2))
                .map_err(|_| "invalid stamped dimension")?,
        })
    })
    .transpose()
}

pub(crate) fn check_in(tx: &mut Transaction<'_>, identity: &EmbedderIdentity) -> StoreResult<()> {
    match stamp_in(tx)? {
        Some(stamp) if stamp == *identity => Ok(()),
        Some(stamp) => Err(format!(
            "mixed-model vectors refused: the index holds {stamp}, the vector is from {identity}"
        )),
        None => Err("vector index is not initialized".into()),
    }
}

pub(crate) fn initialize_in(
    tx: &mut Transaction<'_>,
    identity: &EmbedderIdentity,
) -> StoreResult<()> {
    if !(1..=16000).contains(&identity.dimension) {
        return Err("invalid vector dimension".into());
    }
    if let Some(stamp) = stamp_in(tx)? {
        if stamp != *identity {
            return Err("mixed-model vector import refused".into());
        }
        return Ok(());
    }
    tx.batch_execute(&format!(
        "CREATE TABLE mem_vectors (mem_id TEXT PRIMARY KEY, embedding vector({}) NOT NULL)",
        identity.dimension
    ))
    .map_err(db_error)?;
    tx.execute("INSERT INTO mem_embedder(singleton,provider,model,dimension,stamped_at) VALUES (1,$1,$2,$3,$4)",
        &[&identity.provider,&identity.model,&(identity.dimension as i64),&chrono::Utc::now().to_rfc3339()])
        .map_err(db_error)?;
    Ok(())
}

pub(crate) fn clear_in(tx: &mut Transaction<'_>, id: &str) -> StoreResult<u64> {
    if stamp_in(tx)?.is_none() {
        return Ok(0);
    }
    tx.execute(
        "DELETE FROM mem_vectors WHERE mem_id=$1 OR left(mem_id,length($1)+7)=$1||'_chunk_'",
        &[&id],
    )
    .map_err(db_error)
}

pub(crate) fn insert_in(
    tx: &mut Transaction<'_>,
    id: &str,
    chunk: usize,
    vector: &StampedVector,
) -> StoreResult<InsertOutcome> {
    check_in(tx, &vector.identity)?;
    codeterm_mem::embedder::provider::validate_vector(&vector.vector, vector.identity.dimension)
        .map_err(|e| e.to_string())?;
    let live: bool = tx.query_one("SELECT EXISTS(SELECT 1 FROM mem_items WHERE id=$1 UNION ALL SELECT 1 FROM entities WHERE id=$1)", &[&id])
        .map_err(db_error)?.get(0);
    if !live {
        return Ok(InsertOutcome::ItemGone);
    }
    if chunk == 0 {
        clear_in(tx, id)?;
    }
    tx.execute(
        "INSERT INTO mem_vectors(mem_id,embedding) VALUES ($1,$2)",
        &[
            &vector_id(id, chunk),
            &pgvector::Vector::from(vector.vector.clone()),
        ],
    )
    .map_err(db_error)?;
    Ok(InsertOutcome::Inserted)
}

pub(crate) fn nearest_in(
    tx: &mut Transaction<'_>,
    vector: &StampedVector,
    k: usize,
) -> StoreResult<Vec<(String, f64)>> {
    check_in(tx, &vector.identity)?;
    codeterm_mem::embedder::provider::validate_vector(&vector.vector, vector.identity.dimension)
        .map_err(|e| e.to_string())?;
    let limit = crate::pg_limit(k.saturating_mul(3));
    let rows = tx.query("SELECT mem_id, embedding <-> $1 AS distance FROM mem_vectors ORDER BY distance,mem_id LIMIT $2",
        &[&pgvector::Vector::from(vector.vector.clone()),&limit]).map_err(db_error)?;
    let mut best = BTreeMap::<String, f64>::new();
    for row in rows {
        let raw: String = row.get(0);
        let parent = raw
            .rfind("_chunk_")
            .map_or(raw.as_str(), |pos| &raw[..pos])
            .to_owned();
        let distance: f64 = row.get(1);
        best.entry(parent)
            .and_modify(|v| *v = v.min(distance))
            .or_insert(distance);
    }
    let mut rows: Vec<_> = best.into_iter().collect();
    rows.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    rows.truncate(k);
    Ok(rows)
}

impl PostgresMemStore {
    pub fn vector_stamp(&self) -> StoreResult<Option<EmbedderIdentity>> {
        self.transaction(stamp_in)
    }

    pub fn init_vectors(&self, identity: &EmbedderIdentity) -> StoreResult<()> {
        self.transaction(|tx| {
            if stamp_in(tx)?.is_none() {
                initialize_in(tx, identity)?;
            }
            Ok(())
        })
    }

    pub fn insert_vector(
        &self,
        id: &str,
        chunk: usize,
        vector: &StampedVector,
    ) -> StoreResult<InsertOutcome> {
        self.transaction(|tx| insert_in(tx, id, chunk, vector))
    }

    pub fn vector_search(
        &self,
        vector: &StampedVector,
        k: usize,
    ) -> StoreResult<Vec<(String, f64)>> {
        self.transaction(|tx| nearest_in(tx, vector, k))
    }
}
