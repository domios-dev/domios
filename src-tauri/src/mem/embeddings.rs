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
    use parking_lot::Mutex;
    use rusqlite::{ffi::sqlite3_auto_extension, params, Connection};
    use serde::{Deserialize, Serialize};
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
    use std::sync::{Once, OnceLock};

    static EXT_REGISTERED: Once = Once::new();

    // How many inferences to run in one child before recycling it. Bounds the fastembed/ort memory leak on Windows (~100 MB per call) to a
    // predictable ceiling (~CHILD_MAX_CALLS × 100 MB), then kills and respawns the child to drop all leaked pages.
    const CHILD_MAX_CALLS: u64 = 200;

    #[derive(Serialize)]
    struct EmbedRequest<'a> {
        id: &'a str,
        text: &'a str,
    }

    #[derive(Deserialize)]
    struct EmbedResponse {
        #[allow(dead_code)]
        id: String,
        embedding: Option<Vec<f32>>,
        error: Option<String>,
    }

    struct EmbedChild {
        child: Child,
        stdin: ChildStdin,
        stdout: BufReader<ChildStdout>,
        calls: u64,
    }

    impl EmbedChild {
        fn spawn() -> Result<Self, String> {
            let exe = codeterm_embed_path()?;
            let mut cmd = Command::new(&exe);
            cmd.stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit());
            crate::process_util::no_window_std(&mut cmd);
            let mut child = cmd
                .spawn()
                .map_err(|e| format!("spawn {}: {}", exe.display(), e))?;
            let stdin = child.stdin.take().ok_or("no stdin")?;
            let stdout = BufReader::new(child.stdout.take().ok_or("no stdout")?);
            log::info!("[embed] spawned child pid={}", child.id());
            Ok(Self {
                child,
                stdin,
                stdout,
                calls: 0,
            })
        }

        fn request(&mut self, id: &str, text: &str) -> Result<Vec<f32>, String> {
            let req = EmbedRequest { id, text };
            let line = serde_json::to_string(&req).map_err(|e| e.to_string())?;
            writeln!(self.stdin, "{}", line).map_err(|e| format!("write: {}", e))?;
            self.stdin.flush().map_err(|e| format!("flush: {}", e))?;

            let mut resp_line = String::new();
            self.stdout
                .read_line(&mut resp_line)
                .map_err(|e| format!("read: {}", e))?;
            if resp_line.is_empty() {
                return Err("child closed stdout".into());
            }
            let resp: EmbedResponse = serde_json::from_str(resp_line.trim())
                .map_err(|e| format!("parse response: {}", e))?;
            self.calls += 1;
            if let Some(err) = resp.error {
                return Err(err);
            }
            resp.embedding.ok_or_else(|| "no embedding".into())
        }
    }

    impl Drop for EmbedChild {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    fn codeterm_embed_path() -> Result<std::path::PathBuf, String> {
        // Try the Tauri externalBin location first (sidecar next to main exe), then fall back to a sibling binary in the cargo target dir for `cargo run` / dev scenarios.
        let exe_name = if cfg!(windows) {
            "codeterm-embed.exe"
        } else {
            "codeterm-embed"
        };
        let current = std::env::current_exe().map_err(|e| format!("current_exe: {}", e))?;
        let dir = current.parent().ok_or("no parent dir")?;

        let candidates = [
            dir.join(exe_name),
            dir.join("codeterm-embed"),
            dir.join("bundled-embed").join(exe_name),
        ];
        for path in &candidates {
            if path.exists() {
                return Ok(path.clone());
            }
        }
        Err(format!("codeterm-embed not found near {}", dir.display()))
    }

    fn child_handle() -> &'static Mutex<Option<EmbedChild>> {
        static CELL: OnceLock<Mutex<Option<EmbedChild>>> = OnceLock::new();
        CELL.get_or_init(|| Mutex::new(None))
    }

    /// Register sqlite-vec as an auto-extension. This installs it into the
    /// global auto-extension list — every Connection opened after this call
    /// will have the `vec0` virtual table module available. Must be called
    /// before the first `Connection::open`. Idempotent via `Once`.
    fn register_auto_extension() {
        EXT_REGISTERED.call_once(|| unsafe {
            sqlite3_auto_extension(Some(std::mem::transmute(
                sqlite_vec::sqlite3_vec_init as *const (),
            )));
        });
    }

    /// Create the mem_vectors table on `conn`. Auto-registers the extension
    /// if not already registered, but note that on an existing connection
    /// opened BEFORE registration the module won't be loaded — caller is
    /// responsible for opening the connection after register_auto_extension.
    pub fn init_extension(conn: &Connection) -> Result<(), String> {
        register_auto_extension();
        conn.execute_batch(
            r#"
            CREATE VIRTUAL TABLE IF NOT EXISTS mem_vectors USING vec0(
                mem_id TEXT PRIMARY KEY,
                embedding FLOAT[384]
            );
            "#,
        )
        .map_err(|e| format!("create mem_vectors: {}", e))?;
        Ok(())
    }

    /// Pre-register the sqlite-vec auto-extension. Call this BEFORE opening
    /// any connection that needs vec0. Idempotent. Public so MemManager::open
    /// can hit it before its `Connection::open` call.
    pub fn ensure_extension_registered() {
        register_auto_extension();
    }

    /// Normalize text before embedding: lowercase, collapse whitespace, expand
    /// common abbreviations. Ensures query and indexed text are in the same form.
    fn preprocess(text: &str) -> String {
        let lower = text.to_lowercase();
        let collapsed: String = lower.split_whitespace().collect::<Vec<_>>().join(" ");
        // Expand common abbreviations for better embedding alignment
        expand_abbreviations(&collapsed)
    }

    /// Expand tech abbreviations that trip up embedding models.
    /// "pg" → "postgresql", "js" → "javascript", etc.
    fn expand_abbreviations(text: &str) -> String {
        use std::sync::OnceLock;
        static ABBRS: OnceLock<Vec<(&'static str, &'static str)>> = OnceLock::new();
        let abbrs = ABBRS.get_or_init(|| {
            vec![
                (" pg ", " postgresql "),
                (" js ", " javascript "),
                (" ts ", " typescript "),
                (" py ", " python "),
                (" rb ", " ruby "),
                (" ha ", " home assistant "),
                (" k8s ", " kubernetes "),
                (" tf ", " terraform "),
                (" gh ", " github "),
                (" ci/cd ", " continuous integration continuous deployment "),
            ]
        });
        let mut result = format!(" {} ", text); // pad for word boundary matching
        for (abbr, expanded) in abbrs {
            result = result.replace(abbr, expanded);
        }
        result.trim().to_string()
    }

    /// Compute an embedding for `text`. Delegates to the `codeterm-embed`
    /// subprocess so the fastembed/ort memory leak on Windows is isolated
    /// and bounded by periodic child recycling.
    pub fn embed(text: &str) -> Result<Vec<f32>, String> {
        let normalized = preprocess(text);
        let slot = child_handle();
        let mut guard = slot.lock();

        // Recycle child when call budget is exhausted, to bound ort leak.
        if let Some(c) = guard.as_ref() {
            if c.calls >= CHILD_MAX_CALLS {
                log::info!("[embed] recycling child after {} calls", c.calls);
                *guard = None;
            }
        }
        if guard.is_none() {
            *guard = Some(EmbedChild::spawn()?);
        }

        let child = guard.as_mut().unwrap();
        match child.request("", &normalized) {
            Ok(v) => Ok(v),
            Err(e) => {
                // Child may have died — drop it so next call respawns.
                log::warn!("[embed] child request failed ({}), dropping", e);
                *guard = None;
                Err(e)
            }
        }
    }

    const CHUNK_SIZE: usize = 800;
    const CHUNK_OVERLAP: usize = 100;
    const MIN_CHUNK_LEN: usize = 50;

    /// Split long text into overlapping chunks for better embedding granularity.
    /// Prefers paragraph boundaries (\n\n), then sentence boundaries (. ), then word boundaries.
    fn chunk_text(text: &str) -> Vec<String> {
        let preprocessed = preprocess(text);
        if preprocessed.len() <= CHUNK_SIZE {
            return vec![preprocessed];
        }

        // First try paragraph-aware splitting
        let paragraphs: Vec<&str> = preprocessed
            .split("\n\n")
            .map(|p| p.trim())
            .filter(|p| p.len() >= MIN_CHUNK_LEN)
            .collect();

        // If paragraphs are small enough, use them directly
        if paragraphs.len() > 1 && paragraphs.iter().all(|p| p.len() <= CHUNK_SIZE) {
            return paragraphs.into_iter().map(String::from).collect();
        }

        // Otherwise: sliding window with smart boundary detection. Works on char indices (not bytes) to safely handle multi-byte UTF-8.
        let chars: Vec<char> = preprocessed.chars().collect();
        let total_chars = chars.len();
        let chunk_chars = CHUNK_SIZE; // chars, not bytes
        let overlap_chars = CHUNK_OVERLAP;
        let mut chunks = Vec::new();
        let mut start = 0;

        while start < total_chars {
            let end = (start + chunk_chars).min(total_chars);
            let actual_end = if end < total_chars {
                let window: String = chars[start..end].iter().collect();
                if let Some(pos) = window.rfind("\n\n") {
                    start + pos + 2
                } else if let Some(pos) = window.rfind(". ") {
                    start + pos + 2
                } else if let Some(pos) = window.rfind(' ') {
                    start + pos
                } else {
                    end
                }
            } else {
                end
            };
            let actual_end = actual_end.max(start + 1);
            let chunk: String = chars[start..actual_end].iter().collect();
            let trimmed = chunk.trim();
            if trimmed.len() >= MIN_CHUNK_LEN {
                chunks.push(trimmed.to_string());
            }
            // End of input — last chunk pushed, stop.
            if actual_end >= total_chars {
                break;
            }
            let next_start = if actual_end > overlap_chars {
                actual_end - overlap_chars
            } else {
                actual_end
            };
            // Guard against zero-advance loops: require strict progress.
            if next_start <= start {
                break;
            }
            start = next_start;
        }
        if chunks.is_empty() {
            chunks.push(preprocessed);
        }
        chunks
    }

    /// Build the embeddable text for a graph entity. Concatenates id + type
    /// + flattened properties so vector recall finds the entity by id substring,
    /// type, name, email, role, etc. — whatever was stuffed into properties_json.
    pub fn embeddable_text_for_entity(
        id: &str,
        entity_type: &str,
        properties_json: &str,
    ) -> String {
        let mut out = format!("{} {}", id, entity_type);
        if let Ok(serde_json::Value::Object(map)) =
            serde_json::from_str::<serde_json::Value>(properties_json)
        {
            for (k, v) in map.iter() {
                let v_str = match v {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                out.push(' ');
                out.push_str(k);
                out.push_str(": ");
                out.push_str(&v_str);
            }
        }
        out
    }

    /// Index a single mem_item by id. Chunks long bodies into overlapping
    /// segments and embeds each separately for better recall granularity.
    /// Uses DELETE + INSERT because vec0 virtual tables don't support
    /// `INSERT OR REPLACE` semantics.
    pub fn index(conn: &Connection, mem_id: &str, body: &str) -> Result<(), String> {
        let deleted = conn
            .execute(
                "DELETE FROM mem_vectors WHERE mem_id = ?1 OR mem_id LIKE ?2",
                params![mem_id, format!("{}_chunk_%", mem_id)],
            )
            .unwrap_or(0);
        log::info!("[mem.index] {} DELETE affected {} rows", mem_id, deleted);

        let chunks = chunk_text(body);
        log::info!("[mem.index] {} chunks={}", mem_id, chunks.len());
        for (i, chunk) in chunks.iter().enumerate() {
            let t0 = std::time::Instant::now();
            let vec = embed(chunk)?;
            log::info!(
                "[mem.index] {} chunk {} embed={}ms",
                mem_id,
                i,
                t0.elapsed().as_millis()
            );
            let bytes: Vec<u8> = vec.iter().flat_map(|f| f.to_le_bytes()).collect();
            let chunk_id = if i == 0 {
                mem_id.to_string()
            } else {
                format!("{}_chunk_{}", mem_id, i)
            };
            let t1 = std::time::Instant::now();
            conn.execute(
                "INSERT INTO mem_vectors (mem_id, embedding) VALUES (?1, ?2)",
                params![chunk_id, bytes],
            )
            .map_err(|e| format!("insert vector chunk {}: {}", i, e))?;
            log::info!(
                "[mem.index] {} chunk {} insert={}ms",
                mem_id,
                i,
                t1.elapsed().as_millis()
            );
        }
        Ok(())
    }

    /// Strip chunk suffix from vector mem_id to get the canonical mem_item id.
    fn strip_chunk_suffix(id: &str) -> &str {
        if let Some(pos) = id.rfind("_chunk_") {
            &id[..pos]
        } else {
            id
        }
    }

    /// Pure vector recall: top-K nearest neighbours by cosine distance.
    /// Returns deduplicated (mem_id, distance) pairs — chunks are collapsed
    /// to their parent mem_item, keeping the best (lowest) distance.
    pub fn recall(conn: &Connection, query: &str, k: usize) -> Result<Vec<(String, f64)>, String> {
        let qvec = embed(query)?;
        let bytes: Vec<u8> = qvec.iter().flat_map(|f| f.to_le_bytes()).collect();
        // Overfetch to account for chunk deduplication
        let overfetch = k * 3;
        let mut stmt = conn
            .prepare(
                r#"SELECT mem_id, distance
               FROM mem_vectors
               WHERE embedding MATCH ?1 AND k = ?2
               ORDER BY distance"#,
            )
            .map_err(|e| format!("prepare: {}", e))?;
        let rows = stmt
            .query_map(params![bytes, overfetch as i64], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?))
            })
            .map_err(|e| format!("query: {}", e))?;

        // Deduplicate chunks → parent mem_item, keep best distance
        let mut best: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
        for r in rows {
            let (raw_id, dist) = r.map_err(|e| e.to_string())?;
            let canonical = strip_chunk_suffix(&raw_id).to_string();
            let entry = best.entry(canonical).or_insert(f64::MAX);
            if dist < *entry {
                *entry = dist;
            }
        }

        let mut out: Vec<(String, f64)> = best.into_iter().collect();
        out.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        out.truncate(k);
        Ok(out)
    }
}

#[cfg(not(feature = "embeddings"))]
pub mod inner {
    use rusqlite::Connection;

    pub fn init_extension(_conn: &Connection) -> Result<(), String> {
        Ok(())
    }
    pub fn ensure_extension_registered() {}
    pub fn embed(_text: &str) -> Result<Vec<f32>, String> {
        Err("embeddings feature not enabled — rebuild with --features embeddings".into())
    }
    pub fn index(_conn: &Connection, _mem_id: &str, _body: &str) -> Result<(), String> {
        Ok(())
    }
    pub fn recall(
        _conn: &Connection,
        _query: &str,
        _k: usize,
    ) -> Result<Vec<(String, f64)>, String> {
        Err("embeddings feature not enabled".into())
    }
    pub fn embeddable_text_for_entity(id: &str, entity_type: &str, _props: &str) -> String {
        format!("{} {}", id, entity_type)
    }
}

pub use inner::{
    embed, embeddable_text_for_entity, ensure_extension_registered, index, init_extension, recall,
};

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
