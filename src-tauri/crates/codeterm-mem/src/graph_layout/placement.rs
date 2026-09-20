//! Incremental kNN placement + pure layout helpers (clustering, normalization).

use std::collections::HashMap;

use rand::Rng;

use super::cache::{LayoutCache, Position};

pub const COORD_RANGE: f32 = 1000.0;
#[cfg(feature = "embeddings")]
const KNN_K: usize = 8;
const JITTER: f32 = COORD_RANGE * 0.004;

/// Read a 384-dim f32 vector from a sqlite-vec blob (little-endian).
pub fn decode_vector(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

pub fn encode_vector(vec: &[f32]) -> Vec<u8> {
    vec.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// Strip the `_chunk_N` suffix to get the canonical mem_item id.
pub fn strip_chunk_suffix(id: &str) -> &str {
    match id.rfind("_chunk_") {
        Some(pos) => &id[..pos],
        None => id,
    }
}

fn jitter() -> f32 {
    rand::thread_rng().gen_range(-JITTER..=JITTER)
}

/// Distance-weighted average of neighbor positions. `neighbors` are
/// (position, distance) pairs; closer neighbors pull harder.
pub fn weighted_average(neighbors: &[(Position, f64)]) -> Option<Position> {
    if neighbors.is_empty() {
        return None;
    }
    let mut wx = 0.0f64;
    let mut wy = 0.0f64;
    let mut wsum = 0.0f64;
    let mut cluster_votes: HashMap<u32, f64> = HashMap::new();
    for (p, dist) in neighbors {
        let w = 1.0 / (dist + 1e-3);
        wx += p.x as f64 * w;
        wy += p.y as f64 * w;
        wsum += w;
        *cluster_votes.entry(p.cluster_id).or_insert(0.0) += w;
    }
    if wsum <= 0.0 {
        return None;
    }
    let cluster_id = cluster_votes
        .into_iter()
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(c, _)| c)
        .unwrap_or(0);
    Some(Position {
        x: (wx / wsum) as f32 + jitter(),
        y: (wy / wsum) as f32 + jitter(),
        cluster_id,
    })
}

/// Normalize projected 2D points into `[-COORD_RANGE, COORD_RANGE]` on both
/// axes, preserving aspect via a single shared scale. Returns points in input order.
pub fn normalize(points: &[(f32, f32)]) -> Vec<(f32, f32)> {
    if points.is_empty() {
        return Vec::new();
    }
    let (mut min_x, mut min_y) = (f32::MAX, f32::MAX);
    let (mut max_x, mut max_y) = (f32::MIN, f32::MIN);
    for &(x, y) in points {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    let span = ((max_x - min_x).max(max_y - min_y)).max(1e-6);
    let scale = (2.0 * COORD_RANGE) / span;
    let cx = (min_x + max_x) / 2.0;
    let cy = (min_y + max_y) / 2.0;
    points
        .iter()
        .map(|&(x, y)| ((x - cx) * scale, (y - cy) * scale))
        .collect()
}

/// Grid-bucket cluster assignment over normalized points. Targets ~sqrt(n)
/// clusters; buckets smaller than `min_size` are merged into the nearest
/// surviving bucket centroid. Returns a cluster id per input point.
pub fn grid_cluster(points: &[(f32, f32)]) -> Vec<u32> {
    let n = points.len();
    if n == 0 {
        return Vec::new();
    }
    let target = (n as f64).sqrt().round().max(1.0) as usize;
    let grid = (target as f64).sqrt().ceil().max(1.0) as usize;
    let cell = (2.0 * COORD_RANGE) / grid as f32;

    let cell_of = |x: f32, y: f32| -> (usize, usize) {
        let gx = (((x + COORD_RANGE) / cell) as usize).min(grid - 1);
        let gy = (((y + COORD_RANGE) / cell) as usize).min(grid - 1);
        (gx, gy)
    };

    let mut bucket_id: HashMap<(usize, usize), u32> = HashMap::new();
    let mut next = 0u32;
    let mut raw: Vec<u32> = Vec::with_capacity(n);
    for &(x, y) in points {
        let key = cell_of(x, y);
        let id = *bucket_id.entry(key).or_insert_with(|| {
            let id = next;
            next += 1;
            id
        });
        raw.push(id);
    }

    let min_size = ((n as f64).sqrt() / 4.0).ceil().max(1.0) as usize;
    let mut counts: HashMap<u32, usize> = HashMap::new();
    for &c in &raw {
        *counts.entry(c).or_insert(0) += 1;
    }
    let mut centroids: HashMap<u32, (f32, f32, usize)> = HashMap::new();
    for (i, &c) in raw.iter().enumerate() {
        let e = centroids.entry(c).or_insert((0.0, 0.0, 0));
        e.0 += points[i].0;
        e.1 += points[i].1;
        e.2 += 1;
    }
    let survivors: Vec<(u32, (f32, f32))> = centroids
        .iter()
        .filter(|(c, _)| counts.get(c).copied().unwrap_or(0) >= min_size)
        .map(|(c, (sx, sy, cnt))| (*c, (sx / *cnt as f32, sy / *cnt as f32)))
        .collect();

    let remap_tiny = |x: f32, y: f32, own: u32| -> u32 {
        if counts.get(&own).copied().unwrap_or(0) >= min_size {
            return own;
        }
        survivors
            .iter()
            .min_by(|a, b| {
                let da = (a.1 .0 - x).powi(2) + (a.1 .1 - y).powi(2);
                let db = (b.1 .0 - x).powi(2) + (b.1 .1 - y).powi(2);
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(c, _)| *c)
            .unwrap_or(own)
    };

    let mut remapped: Vec<u32> = raw
        .iter()
        .enumerate()
        .map(|(i, &c)| remap_tiny(points[i].0, points[i].1, c))
        .collect();

    // Compact ids to a dense 0..k range.
    let mut dense: HashMap<u32, u32> = HashMap::new();
    let mut dn = 0u32;
    for c in remapped.iter_mut() {
        *c = *dense.entry(*c).or_insert_with(|| {
            let id = dn;
            dn += 1;
            id
        });
    }
    remapped
}

/// Place one (re)embedded item via its raw vector: query the vector index for
/// k nearest primary-chunk neighbors, dedup chunk ids, then weighted-average
/// their cached positions. Updates the in-memory cache (debounced flush).
#[cfg(feature = "embeddings")]
pub fn place_incremental(
    conn: &rusqlite::Connection,
    cache: &LayoutCache,
    mem_id: &str,
    vector: &[f32],
) -> Result<bool, String> {
    use rusqlite::params;
    let bytes = encode_vector(vector);
    let overfetch = (KNN_K * 4) as i64;
    let mut stmt = conn
        .prepare(
            "SELECT mem_id, distance FROM mem_vectors
             WHERE embedding MATCH ?1 AND k = ?2 ORDER BY distance",
        )
        .map_err(|e| format!("prepare knn: {e}"))?;
    let rows = stmt
        .query_map(params![bytes, overfetch], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?))
        })
        .map_err(|e| format!("query knn: {e}"))?;

    let mut best: HashMap<String, f64> = HashMap::new();
    for r in rows {
        let (raw_id, dist) = r.map_err(|e| e.to_string())?;
        let canonical = strip_chunk_suffix(&raw_id).to_string();
        if canonical == mem_id {
            continue;
        }
        let e = best.entry(canonical).or_insert(f64::MAX);
        if dist < *e {
            *e = dist;
        }
    }
    let mut pairs: Vec<(String, f64)> = best.into_iter().collect();
    pairs.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    pairs.truncate(KNN_K);

    let neighbors: Vec<(Position, f64)> = pairs
        .into_iter()
        .filter_map(|(id, d)| cache.get(&id).map(|p| (p, d)))
        .collect();

    match weighted_average(&neighbors) {
        Some(pos) => {
            cache.upsert(mem_id.to_string(), pos);
            Ok(true)
        }
        None => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_encode_round_trip() {
        let v = vec![1.5f32, -2.0, 3.25];
        assert_eq!(decode_vector(&encode_vector(&v)), v);
    }

    #[test]
    fn strip_chunk_suffix_works() {
        assert_eq!(strip_chunk_suffix("fact_abc_chunk_3"), "fact_abc");
        assert_eq!(strip_chunk_suffix("fact_abc"), "fact_abc");
    }

    #[test]
    fn normalize_centers_and_scales() {
        let pts = vec![(0.0, 0.0), (10.0, 10.0), (-10.0, -10.0)];
        let out = normalize(&pts);
        assert!(out
            .iter()
            .all(|&(x, y)| x.abs() <= COORD_RANGE + 1.0 && y.abs() <= COORD_RANGE + 1.0));
        // Center point maps near origin.
        assert!(out[0].0.abs() < 1.0 && out[0].1.abs() < 1.0);
    }

    #[test]
    fn grid_cluster_separates_two_far_blobs() {
        let mut pts = Vec::new();
        for i in 0..50 {
            let f = i as f32 * 0.1;
            pts.push((-800.0 + f, -800.0 + f));
        }
        for i in 0..50 {
            let f = i as f32 * 0.1;
            pts.push((800.0 + f, 800.0 + f));
        }
        let clusters = grid_cluster(&pts);
        let left: std::collections::HashSet<u32> = clusters[..50].iter().copied().collect();
        let right: std::collections::HashSet<u32> = clusters[50..].iter().copied().collect();
        assert!(
            left.is_disjoint(&right),
            "far blobs must land in different clusters"
        );
    }

    #[test]
    fn weighted_average_lands_near_closest_neighbor() {
        let neighbors = vec![
            (
                Position {
                    x: 100.0,
                    y: 100.0,
                    cluster_id: 5,
                },
                0.01,
            ),
            (
                Position {
                    x: -500.0,
                    y: -500.0,
                    cluster_id: 9,
                },
                2.0,
            ),
        ];
        let p = weighted_average(&neighbors).unwrap();
        // Closest neighbor dominates → near (100,100) and inherits its cluster.
        assert!((p.x - 100.0).abs() < 60.0, "x={}", p.x);
        assert!((p.y - 100.0).abs() < 60.0, "y={}", p.y);
        assert_eq!(p.cluster_id, 5);
    }
}
