//! Incremental kNN placement + pure layout helpers (clustering, normalization).

use std::collections::HashMap;

use rand::Rng;

#[cfg(feature = "embeddings")]
use super::cache::LayoutCache;
use super::cache::Position;

pub const COORD_RANGE: f32 = 1000.0;
#[cfg(feature = "embeddings")]
const KNN_K: usize = 8;
const JITTER: f32 = COORD_RANGE * 0.004;

/// Read a 384-dim f32 vector from a sqlite-vec blob (little-endian).
pub fn decode_vector(blob: &[u8]) -> Vec<f32> {
    blob.as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
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

pub fn normalize(points: &[(f32, f32)]) -> Vec<(f32, f32)> {
    normalize_radial(points, 0.4)
}

#[cfg(any(feature = "embeddings", test))]
pub fn normalize_seed(points: &[(f32, f32)]) -> Vec<(f32, f32)> {
    normalize_radial(points, 0.0)
}

fn normalize_radial(points: &[(f32, f32)], core_blend: f64) -> Vec<(f32, f32)> {
    if points.is_empty() {
        return Vec::new();
    }
    let mut xs: Vec<f32> = points.iter().map(|p| p.0).collect();
    let mut ys: Vec<f32> = points.iter().map(|p| p.1).collect();
    xs.sort_unstable_by(f32::total_cmp);
    ys.sort_unstable_by(f32::total_cmp);
    let middle = points.len() / 2;
    let lower = (points.len() - 1) / 2;
    let cx = (xs[lower] as f64 + xs[middle] as f64) / 2.0;
    let cy = (ys[lower] as f64 + ys[middle] as f64) / 2.0;
    let radii: Vec<f64> = points
        .iter()
        .map(|&(x, y)| (x as f64 - cx).hypot(y as f64 - cy))
        .collect();
    let mut sorted = radii.clone();
    sorted.sort_unstable_by(f64::total_cmp);
    let robust_index = (points.len() - 1) * 95 / 100;
    let radius = sorted[robust_index];
    let radius = if radius > 0.0 {
        radius
    } else {
        sorted[points.len() - 1].max(f64::EPSILON)
    };
    let halo = 0.15 * radius;
    let scale = COORD_RANGE as f64 / (radius + halo);
    points
        .iter()
        .zip(radii)
        .map(|(&(x, y), r)| {
            if r == 0.0 {
                return (0.0, 0.0);
            }
            let opened = if r <= radius {
                let lower = sorted.partition_point(|&value| value < r);
                let upper = sorted.partition_point(|&value| value <= r);
                let rank = (lower + upper - 1) as f64 / 2.0;
                let target = radius * (rank / robust_index.max(1) as f64).min(1.0).sqrt();
                r + core_blend * (target - r).max(0.0)
            } else {
                let excess = r - radius;
                radius + halo * (excess / (halo + excess))
            };
            let radial_scale = opened / r * scale;
            (
                ((x as f64 - cx) * radial_scale) as f32,
                ((y as f64 - cy) * radial_scale) as f32,
            )
        })
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
mod tests;
