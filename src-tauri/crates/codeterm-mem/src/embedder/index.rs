use rusqlite::{params, Connection, OptionalExtension};

use super::provider::EmbedderIdentity;
use super::StampedVector;

pub(crate) const STAMP_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS mem_embedder (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    dimension INTEGER NOT NULL,
    stamped_at TEXT NOT NULL
)";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertOutcome {
    Inserted,
    ItemGone,
}

pub fn ensure(
    conn: &Connection,
    active: &EmbedderIdentity,
) -> Result<EmbedderIdentity, String> {
    backend::register(conn)?;
    conn.execute_batch(STAMP_SCHEMA)
        .map_err(|e| format!("create mem_embedder: {e}"))?;
    super::pending::init(conn)?;
    let stamp = match stamp(conn)? {
        Some(stamp) => stamp,
        None => {
            if table_exists(conn)? {
                return Err("unstamped vector index refused; rebuild the index before use".into());
            }
            write_stamp(conn, active)?;
            active.clone()
        }
    };
    if !table_exists(conn)? {
        create_table(conn, stamp.dimension)?;
    }
    Ok(stamp)
}

pub fn stamp(conn: &Connection) -> Result<Option<EmbedderIdentity>, String> {
    conn.query_row(
        "SELECT provider, model, dimension FROM mem_embedder WHERE singleton = 1",
        [],
        |row| {
            Ok(EmbedderIdentity {
                provider: row.get(0)?,
                model: row.get(1)?,
                dimension: row.get::<_, i64>(2)? as usize,
            })
        },
    )
    .optional()
    .map_err(|e| format!("read embedder stamp: {e}"))
}

/// Replace the whole index with an empty one stamped for `identity`, so the
/// index never holds vectors from two models.
pub fn recreate(conn: &Connection, identity: &EmbedderIdentity) -> Result<(), String> {
    backend::register(conn)?;
    conn.execute_batch(STAMP_SCHEMA)
        .map_err(|e| format!("create mem_embedder: {e}"))?;
    super::pending::init(conn)?;
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| format!("begin index recreate: {e}"))?;
    recreate_in(&tx, identity)?;
    tx.commit()
        .map_err(|e| format!("commit index recreate: {e}"))
}

pub(crate) fn recreate_in(conn: &Connection, identity: &EmbedderIdentity) -> Result<(), String> {
    conn.execute_batch("DROP TABLE IF EXISTS mem_vectors")
        .map_err(|e| format!("drop mem_vectors: {e}"))?;
    create_table(conn, identity.dimension)?;
    write_stamp(conn, identity)?;
    super::pending::clear_all(conn)
}

pub fn check(conn: &Connection, identity: &EmbedderIdentity) -> Result<(), String> {
    match stamp(conn)? {
        Some(stamp) if stamp == *identity => Ok(()),
        Some(stamp) => Err(format!(
            "mixed-model vectors refused: the index holds {stamp}, the vector is from {identity}"
        )),
        None => Err("vector index is not initialized".into()),
    }
}

/// Insert a chunk only while its memory item or graph entity still exists and
/// only when the vector's model matches the index stamp.
pub fn insert_if_live(
    conn: &Connection,
    mem_id: &str,
    chunk_index: usize,
    vector: &StampedVector,
) -> Result<InsertOutcome, String> {
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| format!("begin vector insert: {e}"))?;
    check(&tx, &vector.identity)?;
    let live: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM mem_items WHERE id = ?1 UNION ALL SELECT 1 FROM entities WHERE id = ?1)",
            params![mem_id],
            |r| r.get(0),
        )
        .map_err(|e| format!("check item {mem_id}: {e}"))?;
    if !live {
        return Ok(InsertOutcome::ItemGone);
    }
    if chunk_index == 0 {
        clear_vectors(&tx, mem_id);
    }
    tx.execute(
        "INSERT INTO mem_vectors (mem_id, embedding) VALUES (?1, ?2)",
        params![vector_id(mem_id, chunk_index), encode(&vector.vector)],
    )
    .map_err(|e| format!("insert vector chunk {chunk_index}: {e}"))?;
    tx.commit()
        .map_err(|e| format!("commit vector insert: {e}"))?;
    Ok(InsertOutcome::Inserted)
}

pub fn clear_vectors(conn: &Connection, mem_id: &str) -> usize {
    conn.execute(
        "DELETE FROM mem_vectors WHERE mem_id = ?1 OR mem_id LIKE ?2",
        params![mem_id, format!("{mem_id}_chunk_%")],
    )
    .unwrap_or(0)
}

pub fn vector_id(mem_id: &str, chunk_index: usize) -> String {
    if chunk_index == 0 {
        mem_id.to_string()
    } else {
        format!("{mem_id}_chunk_{chunk_index}")
    }
}

pub fn encode(vector: &[f32]) -> Vec<u8> {
    vector.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn strip_chunk_suffix(id: &str) -> &str {
    id.rfind("_chunk_").map_or(id, |pos| &id[..pos])
}

/// Top-K nearest items, chunks collapsed to their parent at the best distance.
pub fn nearest(
    conn: &Connection,
    query: &StampedVector,
    k: usize,
) -> Result<Vec<(String, f64)>, String> {
    backend::register(conn)?;
    check(conn, &query.identity)?;
    let rows = backend::knn(conn, &encode(&query.vector), k * 3)?;
    let mut best: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    for (raw_id, distance) in rows {
        let entry = best
            .entry(strip_chunk_suffix(&raw_id).to_string())
            .or_insert(f64::MAX);
        if distance < *entry {
            *entry = distance;
        }
    }
    let mut out: Vec<(String, f64)> = best.into_iter().collect();
    out.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    out.truncate(k);
    Ok(out)
}

pub fn vector_count(conn: &Connection) -> u64 {
    conn.query_row("SELECT COUNT(*) FROM mem_vectors", [], |r| {
        r.get::<_, i64>(0)
    })
    .map(|count| count as u64)
    .unwrap_or(0)
}

fn table_exists(conn: &Connection) -> Result<bool, String> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name = 'mem_vectors')",
        [],
        |r| r.get(0),
    )
    .map_err(|e| format!("inspect mem_vectors: {e}"))
}

fn create_table(conn: &Connection, dimension: usize) -> Result<(), String> {
    conn.execute_batch(&backend::create_sql(dimension))
        .map_err(|e| format!("create mem_vectors: {e}"))
}

fn write_stamp(conn: &Connection, identity: &EmbedderIdentity) -> Result<(), String> {
    conn.execute(
        "INSERT INTO mem_embedder (singleton, provider, model, dimension, stamped_at)
         VALUES (1, ?1, ?2, ?3, ?4)
         ON CONFLICT(singleton) DO UPDATE SET provider = excluded.provider,
             model = excluded.model, dimension = excluded.dimension,
             stamped_at = excluded.stamped_at",
        params![
            identity.provider,
            identity.model,
            identity.dimension as i64,
            chrono::Utc::now().to_rfc3339()
        ],
    )
    .map_err(|e| format!("write embedder stamp: {e}"))?;
    Ok(())
}

#[cfg(feature = "embeddings")]
mod backend {
    use rusqlite::{params, Connection};

    pub(super) fn register(_conn: &Connection) -> Result<(), String> {
        crate::embeddings::ensure_extension_registered();
        Ok(())
    }

    pub(super) fn create_sql(dimension: usize) -> String {
        format!(
            "CREATE VIRTUAL TABLE mem_vectors USING vec0(mem_id TEXT PRIMARY KEY, embedding FLOAT[{dimension}])"
        )
    }

    pub(super) fn knn(
        conn: &Connection,
        query: &[u8],
        k: usize,
    ) -> Result<Vec<(String, f64)>, String> {
        let mut stmt = conn
            .prepare(
                "SELECT mem_id, distance FROM mem_vectors
                 WHERE embedding MATCH ?1 AND k = ?2 ORDER BY distance",
            )
            .map_err(|e| format!("prepare: {e}"))?;
        let rows = stmt
            .query_map(params![query, k as i64], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|e| format!("query: {e}"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }
}

/// Exact-scan index for builds without sqlite-vec; same table shape and the
/// same distance functions, so every caller's SQL is unchanged.
#[cfg(not(feature = "embeddings"))]
mod backend {
    use rusqlite::functions::FunctionFlags;
    use rusqlite::{params, Connection};

    pub(super) fn register(conn: &Connection) -> Result<(), String> {
        let flags = FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC;
        conn.create_scalar_function("vec_distance_cosine", 2, flags, |ctx| {
            let (a, b) = (
                decode(&ctx.get::<Vec<u8>>(0)?),
                decode(&ctx.get::<Vec<u8>>(1)?),
            );
            let dot: f64 = a.iter().zip(&b).map(|(x, y)| x * y).sum();
            let norm = |v: &[f64]| v.iter().map(|x| x * x).sum::<f64>().sqrt();
            Ok(1.0 - dot / (norm(&a) * norm(&b)).max(f64::MIN_POSITIVE))
        })
        .map_err(|e| format!("register vec_distance_cosine: {e}"))?;
        conn.create_scalar_function("vec_distance_l2", 2, flags, |ctx| {
            let (a, b) = (
                decode(&ctx.get::<Vec<u8>>(0)?),
                decode(&ctx.get::<Vec<u8>>(1)?),
            );
            Ok(a.iter()
                .zip(&b)
                .map(|(x, y)| (x - y).powi(2))
                .sum::<f64>()
                .sqrt())
        })
        .map_err(|e| format!("register vec_distance_l2: {e}"))
    }

    fn decode(blob: &[u8]) -> Vec<f64> {
        blob.as_chunks::<4>()
            .0
            .iter()
            .map(|c| f64::from(f32::from_le_bytes(*c)))
            .collect()
    }

    pub(super) fn create_sql(_dimension: usize) -> String {
        "CREATE TABLE mem_vectors (mem_id TEXT PRIMARY KEY, embedding BLOB NOT NULL)".into()
    }

    pub(super) fn knn(
        conn: &Connection,
        query: &[u8],
        k: usize,
    ) -> Result<Vec<(String, f64)>, String> {
        let mut stmt = conn
            .prepare(
                "SELECT mem_id, vec_distance_l2(embedding, ?1) AS distance
                 FROM mem_vectors ORDER BY distance, mem_id LIMIT ?2",
            )
            .map_err(|e| format!("prepare: {e}"))?;
        let rows = stmt
            .query_map(params![query, k as i64], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|e| format!("query: {e}"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }
}
