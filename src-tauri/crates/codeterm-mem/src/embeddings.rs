use rusqlite::Connection;

use crate::embedder::index::{self, InsertOutcome};
use crate::embedder::{pending, ActiveEmbedder, EmbedError, EmbedderIdentity, StampedVector};

pub use crate::embedder::index::{clear_vectors, encode as encode_vector, vector_id};

#[cfg(feature = "embeddings")]
pub fn ensure_extension_registered() {
    use rusqlite::ffi::sqlite3_auto_extension;
    use std::sync::Once;
    static EXT_REGISTERED: Once = Once::new();
    // Installs vec0 for every Connection opened afterwards; must precede the first open.
    EXT_REGISTERED.call_once(|| unsafe {
        sqlite3_auto_extension(Some(std::mem::transmute(
            sqlite_vec::sqlite3_vec_init as *const (),
        )));
    });
}

#[cfg(not(feature = "embeddings"))]
pub fn ensure_extension_registered() {}

/// Prepare the vector index on `conn` for the process's active embedder.
pub fn init_extension(conn: &Connection) -> Result<(), String> {
    init_index(conn, &ActiveEmbedder::global().identity())
}

pub fn init_index(conn: &Connection, active: &EmbedderIdentity) -> Result<(), String> {
    ensure_extension_registered();
    let stamp = index::ensure(conn, active)?;
    if stamp != *active {
        log::warn!(
            "[mem.embedder] vector index holds {stamp}; active embedder is {active} — vectors stay unused until the switch completes"
        );
    }
    Ok(())
}

/// For rebuildable derived indexes (per-project plan indexes): a stamp from
/// another model is dropped so new vectors land under the active model.
pub fn init_derived_index(conn: &Connection) -> Result<(), String> {
    let active = ActiveEmbedder::global().identity();
    ensure_extension_registered();
    let stamp = index::ensure(conn, &active)?;
    if stamp != active {
        index::recreate(conn, &active)?;
    }
    Ok(())
}

/// Lowercase, collapse whitespace and expand abbreviations, so queries and
/// indexed text reach the embedder in the same form.
pub(crate) fn preprocess(text: &str) -> String {
    let lower = text.to_lowercase();
    let collapsed: String = lower.split_whitespace().collect::<Vec<_>>().join(" ");
    expand_abbreviations(&collapsed)
}

fn expand_abbreviations(text: &str) -> String {
    const ABBREVIATIONS: &[(&str, &str)] = &[
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
    ];
    let mut result = format!(" {text} ");
    for (abbreviation, expanded) in ABBREVIATIONS {
        result = result.replace(abbreviation, expanded);
    }
    result.trim().to_string()
}

pub fn embed(text: &str) -> Result<Vec<f32>, String> {
    ActiveEmbedder::global()
        .passage(text)
        .map(|stamped| stamped.vector)
        .map_err(|error| error.to_string())
}

pub fn query_embed(text: &str) -> Result<Vec<f32>, String> {
    ActiveEmbedder::global()
        .query(text)
        .map(|stamped| stamped.vector)
        .ok_or_else(|| "query embedding unavailable".to_string())
}

/// Preprocess and split `text` into the chunks `index` embeds, one vector each.
pub fn chunk_text(text: &str) -> Vec<String> {
    crate::chunking::split_chunks(&preprocess(text))
}

/// Embeddable text for a graph entity: id, type and flattened properties, so
/// vector recall finds it by name, email, role or any other property.
pub fn embeddable_text_for_entity(id: &str, entity_type: &str, properties_json: &str) -> String {
    let mut out = format!("{id} {entity_type}");
    if let Ok(serde_json::Value::Object(map)) =
        serde_json::from_str::<serde_json::Value>(properties_json)
    {
        for (key, value) in map.iter() {
            let value = match value {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            out.push(' ');
            out.push_str(key);
            out.push_str(": ");
            out.push_str(&value);
        }
    }
    out
}

pub fn index(conn: &Connection, mem_id: &str, body: &str) -> Result<(), String> {
    index_with(conn, &ActiveEmbedder::global(), mem_id, body)
}

/// Embed every chunk of `body` and store it for `mem_id`. An unavailable
/// provider leaves the item vector pending for the backfill.
pub fn index_with(
    conn: &Connection,
    embedder: &ActiveEmbedder,
    mem_id: &str,
    body: &str,
) -> Result<(), String> {
    let chunks = chunk_text(body);
    let vectors = embedder.passages(&chunks);
    store_vectors(conn, mem_id, vectors)
}

pub(crate) fn store_vectors(
    conn: &Connection,
    mem_id: &str,
    vectors: Vec<Result<StampedVector, EmbedError>>,
) -> Result<(), String> {
    for (chunk, vector) in vectors.into_iter().enumerate() {
        let vector = match vector {
            Ok(vector) => vector,
            Err(EmbedError::Unavailable(reason)) => {
                pending::mark(conn, mem_id, &reason)?;
                return Err(EmbedError::Unavailable(reason).to_string());
            }
            Err(error) => return Err(error.to_string()),
        };
        if index::insert_if_live(conn, mem_id, chunk, &vector)? == InsertOutcome::ItemGone {
            break;
        }
    }
    pending::clear(conn, mem_id)
}

pub fn recall(conn: &Connection, query: &str, k: usize) -> Result<Vec<(String, f64)>, String> {
    let vector = ActiveEmbedder::global()
        .query(query)
        .ok_or_else(|| "query embedding unavailable".to_string())?;
    index::nearest(conn, &vector, k)
}

pub fn recall_vector(
    conn: &Connection,
    query: &StampedVector,
    k: usize,
) -> Result<Vec<(String, f64)>, String> {
    index::nearest(conn, query, k)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(not(feature = "embeddings"))]
    fn embed_returns_error_without_feature() {
        let error = embed("hello").unwrap_err();
        assert!(error.contains("embeddings feature not enabled"));
    }

    #[test]
    fn preprocess_expands_abbreviations_idempotently() {
        let once = preprocess("Deploy  JS to K8s");
        assert_eq!(once, "deploy javascript to kubernetes");
        assert_eq!(preprocess(&once), once);
    }
}
