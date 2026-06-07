//! Full 2D projection job: vectors → annembed → grid clusters → cache swap.

use std::collections::{HashMap, HashSet};

use super::cache::{LayoutCache, Position};
use super::placement;

/// Load primary-chunk vectors (skip `*_chunk_N`). Returns (id, vec) pairs.
#[cfg(feature = "embeddings")]
pub fn load_primary_vectors(conn: &rusqlite::Connection) -> Result<Vec<(String, Vec<f32>)>, String> {
    let mut stmt = conn
        .prepare("SELECT mem_id, embedding FROM mem_vectors")
        .map_err(|e| format!("prepare vectors: {e}"))?;
    let rows = stmt
        .query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
        })
        .map_err(|e| format!("query vectors: {e}"))?;
    let mut out = Vec::new();
    for r in rows {
        let (id, blob) = r.map_err(|e| e.to_string())?;
        if id.contains("_chunk_") {
            continue;
        }
        out.push((id, placement::decode_vector(&blob)));
    }
    Ok(out)
}

/// Read active relations (valid_to IS NULL) as adjacency for centroid placement.
pub fn load_active_adjacency(
    conn: &rusqlite::Connection,
) -> Result<HashMap<String, Vec<String>>, String> {
    let mut stmt = conn
        .prepare("SELECT from_id, to_id FROM relations WHERE valid_to IS NULL")
        .map_err(|e| format!("prepare relations: {e}"))?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(|e| format!("query relations: {e}"))?;
    let mut adj: HashMap<String, Vec<String>> = HashMap::new();
    for r in rows {
        let (from, to) = r.map_err(|e| e.to_string())?;
        adj.entry(from.clone()).or_default().push(to.clone());
        adj.entry(to).or_default().push(from);
    }
    Ok(adj)
}

/// All entity ids (for placing vectorless entities at related-item centroids).
pub fn load_entity_ids(conn: &rusqlite::Connection) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare("SELECT id FROM entities")
        .map_err(|e| format!("prepare entities: {e}"))?;
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| format!("query entities: {e}"))?;
    rows.map(|r| r.map_err(|e| e.to_string())).collect()
}

/// Project 384-dim vectors to 2D via the annembed UMAP-style pipeline.
/// Returns points in the same order as `vectors`.
#[cfg(feature = "embeddings")]
fn project_2d(vectors: &[Vec<f32>]) -> Result<Vec<(f32, f32)>, String> {
    use annembed::fromhnsw::kgraph_from_hnsw_all;
    use annembed::prelude::*;
    use hnsw_rs::prelude::{DistCosine, Hnsw};

    let n = vectors.len();
    if n == 0 {
        return Ok(Vec::new());
    }
    // Tiny inputs can't form a meaningful neighborhood graph — fall back to a
    // deterministic line so the cache still gets positions.
    if n < 16 {
        return Ok((0..n).map(|i| (i as f32, 0.0)).collect());
    }

    let max_nb_conn = 16usize;
    let nb_layer = 16usize;
    let ef_c = 200usize;
    let hnsw = Hnsw::<f32, DistCosine>::new(max_nb_conn, n, nb_layer, ef_c, DistCosine {});
    let data: Vec<(&Vec<f32>, usize)> = vectors.iter().enumerate().map(|(i, v)| (v, i)).collect();
    hnsw.parallel_insert(&data);

    let kgraph =
        kgraph_from_hnsw_all::<f32, DistCosine, f64>(&hnsw, 15).map_err(|e| format!("kgraph: {e}"))?;
    let mut params = EmbedderParams::default();
    params.asked_dim = 2;
    let mut embedder = Embedder::new(&kgraph, params);
    embedder
        .embed()
        .map_err(|e| format!("embed failed at iteration {e}"))?;
    let emb = embedder.get_embedded_reindexed();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        out.push((emb[[i, 0]] as f32, emb[[i, 1]] as f32));
    }
    Ok(out)
}

/// Run the full projection and swap the cache. Returns the new layout_version.
#[cfg(feature = "embeddings")]
pub fn run_full(conn: &rusqlite::Connection, cache: &LayoutCache) -> Result<u64, String> {
    let started = std::time::Instant::now();
    let vectors = load_primary_vectors(conn)?;
    let n = vectors.len();
    log::info!("[graph_layout] full projection: {n} primary vectors");

    let raw: Vec<Vec<f32>> = vectors.iter().map(|(_, v)| v.clone()).collect();
    let projected = project_2d(&raw)?;
    let normalized = placement::normalize(&projected);
    let clusters = placement::grid_cluster(&normalized);

    let mut positions: HashMap<String, Position> = HashMap::with_capacity(n);
    for (i, (id, _)) in vectors.iter().enumerate() {
        positions.insert(
            id.clone(),
            Position {
                x: normalized[i].0,
                y: normalized[i].1,
                cluster_id: clusters.get(i).copied().unwrap_or(0),
            },
        );
    }

    place_vectorless_entities(conn, &mut positions)?;

    let computed_at = chrono::Utc::now().to_rfc3339();
    let version = cache.replace_all(positions, computed_at);
    cache.flush_now();
    log::info!(
        "[graph_layout] full projection done v{version} in {}ms",
        started.elapsed().as_millis()
    );
    Ok(version)
}

#[cfg(not(feature = "embeddings"))]
pub fn run_full(_conn: &rusqlite::Connection, _cache: &LayoutCache) -> Result<u64, String> {
    Err("embeddings feature not enabled — layout projection unavailable".into())
}

/// Entities without a vector get placed at the centroid of their graph-related
/// placed positions (+ jitter). Unplaceable entities are skipped.
fn place_vectorless_entities(
    conn: &rusqlite::Connection,
    positions: &mut HashMap<String, Position>,
) -> Result<(), String> {
    let entity_ids = load_entity_ids(conn)?;
    let missing: Vec<String> = entity_ids
        .into_iter()
        .filter(|id| !positions.contains_key(id))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    let adj = load_active_adjacency(conn)?;
    let placed: HashSet<String> = positions.keys().cloned().collect();

    for id in missing {
        let Some(neighbors) = adj.get(&id) else {
            continue;
        };
        let mut sx = 0.0f32;
        let mut sy = 0.0f32;
        let mut cnt = 0u32;
        let mut cluster = 0u32;
        for nb in neighbors {
            if !placed.contains(nb) {
                continue;
            }
            if let Some(p) = positions.get(nb) {
                sx += p.x;
                sy += p.y;
                cluster = p.cluster_id;
                cnt += 1;
            }
        }
        if cnt == 0 {
            continue;
        }
        use rand::Rng;
        let j = placement::COORD_RANGE * 0.01;
        let mut rng = rand::thread_rng();
        positions.insert(
            id,
            Position {
                x: sx / cnt as f32 + rng.gen_range(-j..=j),
                y: sy / cnt as f32 + rng.gen_range(-j..=j),
                cluster_id: cluster,
            },
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn schema(conn: &Connection) {
        crate::mem::store::init_schema(conn).unwrap();
    }

    #[test]
    fn adjacency_is_undirected_and_active_only() {
        let conn = Connection::open_in_memory().unwrap();
        schema(&conn);
        conn.execute(
            "INSERT INTO relations (from_id, relation_type, to_id, scope, valid_from, created_at) VALUES ('a','rel','b','global','t','t')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO relations (from_id, relation_type, to_id, scope, valid_from, valid_to, created_at) VALUES ('a','rel','c','global','t','ended','t')",
            [],
        ).unwrap();
        let adj = load_active_adjacency(&conn).unwrap();
        assert_eq!(adj.get("a").unwrap(), &vec!["b".to_string()]);
        assert_eq!(adj.get("b").unwrap(), &vec!["a".to_string()]);
        assert!(adj.get("c").is_none(), "invalidated edge must be excluded");
    }
}
