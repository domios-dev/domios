//! Vector embeddings + sqlite-vec semantic recall.
//!
//! Gated behind `embeddings` feature flag to avoid pulling onnxruntime
//! (~50MB binary) into the default build. When the feature is off, this
//! module compiles to no-op stubs and `mem recall` falls back to FTS5
//! hybrid search (which delivers 96.6% on LongMemEval per MemPalace).
//!
//! When the feature is on:
//! - sqlite-vec extension loaded into the existing knowledge.db connection
//! - mem_vectors virtual table 1:1 with mem_items
//! - fastembed-rs runs all-MiniLM-L6-v2 locally (384-dim, no API key)
//! - search formula: vector_dist × (1 − 0.30 × keyword_overlap) × (1 − temporal_boost)

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingsConfig {
    pub enabled: bool,
    pub provider: String, // "local" | "openai" | "voyage"
    pub model: String,
    pub dimension: usize,
}

impl Default for EmbeddingsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: "local".into(),
            model: "all-MiniLM-L6-v2".into(),
            dimension: 384,
        }
    }
}

#[cfg(feature = "embeddings")]
pub mod inner {
    use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
    use parking_lot::Mutex;
    use rusqlite::{params, Connection};
    use std::sync::OnceLock;

    static MODEL: OnceLock<Mutex<Option<TextEmbedding>>> = OnceLock::new();

    /// Load sqlite-vec extension into a connection. Idempotent.
    pub fn init_extension(conn: &Connection) -> Result<(), String> {
        unsafe {
            sqlite_vec::sqlite3_vec_init();
        }
        // Create the vectors table 1:1 with mem_items
        conn.execute_batch(
            r#"
            CREATE VIRTUAL TABLE IF NOT EXISTS mem_vectors USING vec0(
                mem_id TEXT PRIMARY KEY,
                embedding FLOAT[384]
            );
            "#,
        ).map_err(|e| format!("create mem_vectors: {}", e))?;
        Ok(())
    }

    /// Compute an embedding for `text`. Initializes the model on first call.
    pub fn embed(text: &str) -> Result<Vec<f32>, String> {
        let lock = MODEL.get_or_init(|| Mutex::new(None));
        let mut guard = lock.lock();
        if guard.is_none() {
            let model = TextEmbedding::try_new(
                InitOptions::new(EmbeddingModel::AllMiniLML6V2),
            ).map_err(|e| format!("model init: {}", e))?;
            *guard = Some(model);
        }
        let model = guard.as_mut().unwrap();
        let embeddings = model
            .embed(vec![text.to_string()], None)
            .map_err(|e| format!("embed: {}", e))?;
        embeddings.into_iter().next().ok_or_else(|| "no embedding".to_string())
    }

    /// Index a single mem_item by id (re-embed body + write to mem_vectors).
    pub fn index(conn: &Connection, mem_id: &str, body: &str) -> Result<(), String> {
        let vec = embed(body)?;
        let bytes: Vec<u8> = vec
            .iter()
            .flat_map(|f| f.to_le_bytes())
            .collect();
        conn.execute(
            "INSERT OR REPLACE INTO mem_vectors (mem_id, embedding) VALUES (?1, ?2)",
            params![mem_id, bytes],
        ).map_err(|e| format!("insert vector: {}", e))?;
        Ok(())
    }

    /// Pure vector recall: top-K nearest neighbours by cosine distance.
    /// Returns (mem_id, distance) pairs.
    pub fn recall(conn: &Connection, query: &str, k: usize) -> Result<Vec<(String, f64)>, String> {
        let qvec = embed(query)?;
        let bytes: Vec<u8> = qvec.iter().flat_map(|f| f.to_le_bytes()).collect();
        let mut stmt = conn.prepare(
            r#"SELECT mem_id, distance
               FROM mem_vectors
               WHERE embedding MATCH ?1 AND k = ?2
               ORDER BY distance"#,
        ).map_err(|e| format!("prepare: {}", e))?;
        let rows = stmt.query_map(params![bytes, k as i64], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?))
        }).map_err(|e| format!("query: {}", e))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| e.to_string())?);
        }
        Ok(out)
    }
}

#[cfg(not(feature = "embeddings"))]
pub mod inner {
    use rusqlite::Connection;

    pub fn init_extension(_conn: &Connection) -> Result<(), String> { Ok(()) }
    pub fn embed(_text: &str) -> Result<Vec<f32>, String> {
        Err("embeddings feature not enabled — rebuild with --features embeddings".into())
    }
    pub fn index(_conn: &Connection, _mem_id: &str, _body: &str) -> Result<(), String> { Ok(()) }
    pub fn recall(_conn: &Connection, _query: &str, _k: usize) -> Result<Vec<(String, f64)>, String> {
        Err("embeddings feature not enabled".into())
    }
}

pub use inner::{embed, index, init_extension, recall};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_default_disabled() {
        let cfg = EmbeddingsConfig::default();
        assert!(!cfg.enabled);
        assert_eq!(cfg.dimension, 384);
    }

    #[test]
    #[cfg(not(feature = "embeddings"))]
    fn embed_returns_error_without_feature() {
        let r = embed("hello");
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("embeddings feature not enabled"));
    }
}
