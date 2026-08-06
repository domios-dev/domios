//! Full 2D projection job: vectors -> 2D seed -> grid clusters -> cache swap.

use std::collections::{HashMap, HashSet};

use super::cache::{LayoutCache, Position};
use super::placement;

/// Load primary-chunk vectors (skip `*_chunk_N`). Returns (id, vec) pairs.
pub fn load_primary_vectors(
    conn: &rusqlite::Connection,
) -> Result<Vec<(String, Vec<f32>)>, String> {
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

#[cfg(feature = "embeddings")]
fn build_hnsw(
    vectors: &[Vec<f32>],
) -> hnsw_rs::prelude::Hnsw<'static, f32, hnsw_rs::prelude::DistCosine> {
    use hnsw_rs::prelude::{DistCosine, Hnsw};

    let n = vectors.len();
    let max_nb_conn = 16usize;
    let nb_layer = 16usize;
    let ef_c = 200usize;
    let hnsw = Hnsw::<f32, DistCosine>::new(max_nb_conn, n, nb_layer, ef_c, DistCosine {});
    let data: Vec<(&Vec<f32>, usize)> = vectors.iter().enumerate().map(|(i, v)| (v, i)).collect();
    hnsw.parallel_insert(&data);
    hnsw
}

#[cfg(feature = "embeddings")]
fn mutual_knn(
    vectors: &[Vec<f32>],
    hnsw: &hnsw_rs::prelude::Hnsw<f32, hnsw_rs::prelude::DistCosine>,
) -> Vec<Vec<usize>> {
    use std::cmp::Ordering;

    let n = vectors.len();
    let mut directed: Vec<HashSet<usize>> = vec![HashSet::new(); n];
    for point in hnsw.get_point_indexation().into_iter() {
        let from = point.get_origin_id();
        if from >= n {
            continue;
        }
        let mut neighbors: Vec<_> = point
            .get_neighborhood_id()
            .into_iter()
            .flatten()
            .filter(|nb| nb.d_id != from && nb.d_id < n)
            .collect();
        neighbors.sort_unstable_by(|a, b| {
            a.distance
                .partial_cmp(&b.distance)
                .unwrap_or(Ordering::Less)
                .then_with(|| a.d_id.cmp(&b.d_id))
        });
        for nb in neighbors {
            let to = nb.d_id;
            directed[from].insert(to);
            if directed[from].len() >= 15 {
                break;
            }
        }
    }
    let mut out: Vec<Vec<usize>> = vec![Vec::new(); n];
    for from in 0..n {
        for &to in &directed[from] {
            if from < to && directed[to].contains(&from) {
                out[from].push(to);
                out[to].push(from);
            }
        }
    }
    out
}

/// Project 384-dim vectors to 2D via the annembed UMAP-style pipeline.
/// Returns (points, knn adjacency), both indexed in `vectors` order.
/// macOS-only: annembed's LAPACK backend is the Accelerate framework.
#[cfg(all(feature = "embeddings", target_os = "macos"))]
fn project_2d(vectors: &[Vec<f32>]) -> Result<(Vec<(f32, f32)>, Vec<Vec<usize>>), String> {
    use annembed::fromhnsw::kgraph_from_hnsw_all;
    use annembed::prelude::*;
    use hnsw_rs::prelude::DistCosine;

    let n = vectors.len();
    if n == 0 {
        return Ok((Vec::new(), Vec::new()));
    }
    // Tiny inputs can't form a meaningful neighborhood graph; use a stable line.
    if n < 16 {
        return Ok((
            (0..n).map(|i| (i as f32, 0.0)).collect(),
            vec![Vec::new(); n],
        ));
    }

    let hnsw = build_hnsw(vectors);
    let knn = mutual_knn(vectors, &hnsw);
    let kgraph = kgraph_from_hnsw_all::<f32, DistCosine, f64>(&hnsw, 15)
        .map_err(|e| format!("kgraph: {e}"))?;

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
    Ok((out, knn))
}

#[cfg(all(feature = "embeddings", not(target_os = "macos")))]
fn project_2d(vectors: &[Vec<f32>]) -> Result<(Vec<(f32, f32)>, Vec<Vec<usize>>), String> {
    let n = vectors.len();
    if n == 0 {
        return Ok((Vec::new(), Vec::new()));
    }
    if n < 16 {
        return Ok((
            (0..n).map(|i| (i as f32, 0.0)).collect(),
            vec![Vec::new(); n],
        ));
    }

    let hnsw = build_hnsw(vectors);
    let knn = mutual_knn(vectors, &hnsw);
    Ok((pca_seed(vectors), knn))
}

#[cfg(all(feature = "embeddings", not(target_os = "macos")))]
fn pca_seed(vectors: &[Vec<f32>]) -> Vec<(f32, f32)> {
    let n = vectors.len();
    let dim = vectors.iter().map(Vec::len).min().unwrap_or(0);
    if dim == 0 {
        return (0..n).map(|i| (i as f32, 0.0)).collect();
    }

    let mut mean = vec![0.0f64; dim];
    for v in vectors {
        for (i, &x) in v.iter().take(dim).enumerate() {
            mean[i] += x as f64;
        }
    }
    for x in &mut mean {
        *x /= n as f64;
    }

    let mut cov = vec![0.0f64; dim * dim];
    for v in vectors {
        for i in 0..dim {
            let xi = v[i] as f64 - mean[i];
            for j in i..dim {
                cov[i * dim + j] += xi * (v[j] as f64 - mean[j]);
            }
        }
    }
    let scale = 1.0 / (n.saturating_sub(1).max(1) as f64);
    for i in 0..dim {
        for j in i..dim {
            let value = cov[i * dim + j] * scale;
            cov[i * dim + j] = value;
            cov[j * dim + i] = value;
        }
    }

    let pc1 = power_iter(&cov, dim);
    let lambda1 = rayleigh(&cov, &pc1, dim);
    for i in 0..dim {
        for j in 0..dim {
            cov[i * dim + j] -= lambda1 * pc1[i] * pc1[j];
        }
    }
    let pc2 = power_iter(&cov, dim);

    vectors
        .iter()
        .map(|v| {
            let mut x = 0.0f64;
            let mut y = 0.0f64;
            for i in 0..dim {
                let centered = v[i] as f64 - mean[i];
                x += centered * pc1[i];
                y += centered * pc2[i];
            }
            (x as f32, y as f32)
        })
        .collect()
}

#[cfg(all(feature = "embeddings", not(target_os = "macos")))]
fn power_iter(cov: &[f64], dim: usize) -> Vec<f64> {
    let mut v: Vec<f64> = (0..dim).map(|i| 1.0 / ((i + 1) as f64)).collect();
    normalize_vec(&mut v);
    for _ in 0..64 {
        let mut next = vec![0.0f64; dim];
        for i in 0..dim {
            for j in 0..dim {
                next[i] += cov[i * dim + j] * v[j];
            }
        }
        if normalize_vec(&mut next) <= 1e-12 {
            break;
        }
        v = next;
    }
    v
}

#[cfg(all(feature = "embeddings", not(target_os = "macos")))]
fn normalize_vec(v: &mut [f64]) -> f64 {
    let norm = v.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm > 0.0 {
        for x in v {
            *x /= norm;
        }
    }
    norm
}

#[cfg(all(feature = "embeddings", not(target_os = "macos")))]
fn rayleigh(cov: &[f64], v: &[f64], dim: usize) -> f64 {
    let mut out = 0.0f64;
    for i in 0..dim {
        for j in 0..dim {
            out += v[i] * cov[i * dim + j] * v[j];
        }
    }
    out
}

/// Run the full projection and swap the cache. Returns the new layout_version.
#[cfg(not(feature = "embeddings"))]
pub fn run_full(_conn: &rusqlite::Connection, _cache: &LayoutCache) -> Result<u64, String> {
    Err("layout projection unavailable on this build (needs the embeddings feature)".into())
}

/// Run the full projection and swap the cache. Returns the new layout_version.
#[cfg(feature = "embeddings")]
pub fn run_full(conn: &rusqlite::Connection, cache: &LayoutCache) -> Result<u64, String> {
    let started = std::time::Instant::now();
    let vectors = load_primary_vectors(conn)?;
    let n = vectors.len();
    log::info!("[graph_layout] full projection: {n} primary vectors");

    let raw: Vec<Vec<f32>> = vectors.iter().map(|(_, v)| v.clone()).collect();
    let (projected, knn) = project_2d(&raw)?;
    let mut normalized = placement::normalize(&projected);

    // mutual-kNN edges keep the semantic neighborhoods from drifting apart.
    let id_index: HashMap<&str, usize> = vectors
        .iter()
        .enumerate()
        .map(|(i, (id, _))| (id.as_str(), i))
        .collect();
    let adj = load_active_adjacency(conn)?;
    let mut edges: Vec<super::linlog::WeightedEdge> = Vec::new();
    for (from, tos) in &adj {
        let Some(&a) = id_index.get(from.as_str()) else {
            continue;
        };
        for to in tos {
            let Some(&b) = id_index.get(to.as_str()) else {
                continue;
            };
            if a < b {
                edges.push(super::linlog::WeightedEdge { a, b, weight: 2.0 });
            }
        }
    }
    for (a, nbs) in knn.iter().enumerate() {
        for &b in nbs {
            if a < b {
                edges.push(super::linlog::WeightedEdge { a, b, weight: 0.3 });
            }
        }
    }
    super::linlog::refine(&mut normalized, &edges);
    let mut normalized = placement::normalize(&normalized);

    let relation_edges: Vec<super::louvain::LouvainEdge> = edges
        .iter()
        .filter(|e| e.weight > 0.5)
        .map(|e| super::louvain::LouvainEdge {
            a: e.a,
            b: e.b,
            weight: e.weight,
        })
        .collect();
    let mut clusters = if relation_edges.is_empty() {
        super::cluster::label_propagation(&knn)
    } else {
        let mut has_edge = vec![false; n];
        for e in &relation_edges {
            has_edge[e.a] = true;
            has_edge[e.b] = true;
        }
        let mut labels = super::louvain::louvain(n, &relation_edges);
        super::cluster::attach_by_neighbors(&mut labels, &has_edge, &knn);
        labels
    };
    super::cluster::split_oversized(&mut clusters, &normalized);
    super::cluster::merge_small(&mut clusters, &normalized);
    super::cluster::centroid_pull(&mut normalized, &clusters);
    let relation_pairs: Vec<(usize, usize)> = relation_edges.iter().map(|e| (e.a, e.b)).collect();
    super::cluster::snap_leaves(&mut normalized, &relation_pairs);
    let normalized = placement::normalize(&normalized);

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
        crate::store::init_schema(conn).unwrap();
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

    #[cfg(all(feature = "embeddings", not(target_os = "macos")))]
    #[test]
    fn non_macos_project_2d_pca_seed_and_knn_are_populated() {
        let mut vectors = Vec::new();
        for i in 0..12 {
            vectors.push(vec![1.0 + i as f32 * 0.01, 0.1, 0.0, 0.0]);
            vectors.push(vec![-1.0 - i as f32 * 0.01, -0.1, 0.0, 0.0]);
        }

        let (points, knn) = project_2d(&vectors).unwrap();
        assert_eq!(points.len(), vectors.len());
        assert_eq!(knn.len(), vectors.len());
        assert!(knn.iter().any(|nbs| !nbs.is_empty()));
        assert!(points.iter().all(|(x, y)| x.is_finite() && y.is_finite()));

        let distinct = points
            .iter()
            .any(|p| (p.0 - points[0].0).abs() > 1e-4 || (p.1 - points[0].1).abs() > 1e-4);
        assert!(distinct, "PCA seed must produce distinct coordinates");
    }
}
