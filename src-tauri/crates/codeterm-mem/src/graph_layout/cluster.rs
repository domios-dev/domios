//! Community clustering over the kNN graph + clump-amplifying post-pass.

use std::collections::{BTreeMap, HashMap};

use super::placement::COORD_RANGE;

/// Fraction of a point's offset from its cluster centroid that survives the
/// pull — lower = tighter clumps.
const PULL_FACTOR: f32 = 0.45;
/// How far cluster centroids are spread out from the global center.
const SPREAD_FACTOR: f32 = 1.15;
const LP_ITERATIONS: usize = 8;
const MIN_CLUSTER_SIZE: usize = 3;

/// Asynchronous label propagation over an undirected kNN adjacency.
/// Tie-breaks and scan order use a fixed-seed RNG: deterministic for a given
/// input, but free of the smallest-label bias that lets one community cascade
/// across a bridge edge and swallow its neighbor.
pub fn label_propagation(neighbors: &[Vec<usize>]) -> Vec<u32> {
    use rand::seq::SliceRandom;
    use rand::{Rng, SeedableRng};

    let n = neighbors.len();
    let mut labels: Vec<u32> = (0..n as u32).collect();
    let mut rng = rand::rngs::StdRng::seed_from_u64(0x5EED_C0DE);
    let mut order: Vec<usize> = (0..n).collect();

    for _ in 0..LP_ITERATIONS {
        order.shuffle(&mut rng);
        let mut changed = false;
        for &i in &order {
            let mut votes: HashMap<u32, usize> = HashMap::new();
            // Half-weight self-vote stabilizes ties without freezing convergence.
            for &nb in &neighbors[i] {
                *votes.entry(labels[nb]).or_insert(0) += 2;
            }
            *votes.entry(labels[i]).or_insert(0) += 1;
            let max = match votes.values().max() {
                Some(&m) => m,
                None => continue,
            };
            let mut tied: Vec<u32> = votes
                .into_iter()
                .filter(|(_, v)| *v == max)
                .map(|(l, _)| l)
                .collect();
            if tied.contains(&labels[i]) {
                continue;
            }
            tied.sort_unstable();
            let best = tied[rng.gen_range(0..tied.len())];
            if best != labels[i] {
                labels[i] = best;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    compact(&mut labels);
    labels
}

/// Attach nodes that have no relation edges (Louvain gave them singletons) to
/// the majority community among their embedding kNN neighbors. A few passes
/// let attachment cascade through edgeless regions; never overrides a node
/// that has real edges.
pub fn attach_by_neighbors(labels: &mut [u32], has_edge: &[bool], knn: &[Vec<usize>]) {
    for _ in 0..3 {
        let mut changed = false;
        for i in 0..labels.len() {
            if has_edge[i] {
                continue;
            }
            let mut votes: HashMap<u32, usize> = HashMap::new();
            for &nb in knn.get(i).map(|v| v.as_slice()).unwrap_or(&[]) {
                if has_edge[nb] || labels[nb] != labels[i] {
                    *votes.entry(labels[nb]).or_insert(0) += if has_edge[nb] { 2 } else { 1 };
                }
            }
            if let Some((&best, _)) = votes.iter().max_by_key(|(_, &v)| v) {
                if best != labels[i] {
                    labels[i] = best;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
}

/// Split communities larger than ~4*sqrt(n) into spatial chunks using the
/// projected positions — the projection is semantically local, so chunks stay
/// coherent while the map gains visible granularity.
pub fn split_oversized(labels: &mut [u32], points: &[(f32, f32)]) {
    let n = labels.len();
    if n == 0 {
        return;
    }
    let max_size = ((n as f64).sqrt() * 4.0).ceil().max(8.0) as usize;
    let target = ((n as f64).sqrt() * 2.0).ceil().max(4.0) as usize;
    let mut next = labels.iter().copied().max().unwrap_or(0) + 1;

    let mut members: HashMap<u32, Vec<usize>> = HashMap::new();
    for (i, &l) in labels.iter().enumerate() {
        members.entry(l).or_default().push(i);
    }
    for (_, idxs) in members {
        if idxs.len() <= max_size {
            continue;
        }
        let (mut min_x, mut min_y) = (f32::MAX, f32::MAX);
        let (mut max_x, mut max_y) = (f32::MIN, f32::MIN);
        for &i in &idxs {
            min_x = min_x.min(points[i].0);
            min_y = min_y.min(points[i].1);
            max_x = max_x.max(points[i].0);
            max_y = max_y.max(points[i].1);
        }
        let pieces = (idxs.len() as f64 / target as f64).sqrt().ceil().max(2.0) as usize;
        let cell_x = ((max_x - min_x) / pieces as f32).max(1e-3);
        let cell_y = ((max_y - min_y) / pieces as f32).max(1e-3);
        let mut cells: HashMap<(usize, usize), u32> = HashMap::new();
        for &i in &idxs {
            let gx = (((points[i].0 - min_x) / cell_x) as usize).min(pieces - 1);
            let gy = (((points[i].1 - min_y) / cell_y) as usize).min(pieces - 1);
            let id = *cells.entry((gx, gy)).or_insert_with(|| {
                let id = next;
                next += 1;
                id
            });
            labels[i] = id;
        }
    }
    compact_slice(labels);
}

/// Merge clusters below MIN_CLUSTER_SIZE into the nearest surviving cluster
/// by 2D centroid distance, then re-compact ids.
pub fn merge_small(labels: &mut [u32], points: &[(f32, f32)]) {
    let cents = centroids(labels, points);
    let survivors: Vec<(u32, (f32, f32))> = cents
        .iter()
        .filter(|(_, (_, _, cnt))| *cnt >= MIN_CLUSTER_SIZE)
        .map(|(&c, &(x, y, cnt))| (c, (x / cnt as f32, y / cnt as f32)))
        .collect();
    if survivors.is_empty() {
        compact_slice(labels);
        return;
    }
    for (i, l) in labels.iter_mut().enumerate() {
        let cnt = cents.get(l).map(|c| c.2).unwrap_or(0);
        if cnt >= MIN_CLUSTER_SIZE {
            continue;
        }
        let (x, y) = points[i];
        *l = survivors
            .iter()
            .min_by(|a, b| {
                let da = (a.1 .0 - x).powi(2) + (a.1 .1 - y).powi(2);
                let db = (b.1 .0 - x).powi(2) + (b.1 .1 - y).powi(2);
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(c, _)| *c)
            .unwrap_or(*l);
    }
    compact_slice(labels);
}

/// Pull points toward their cluster centroid and spread centroids apart.
/// Output stays roughly within the coordinate range; caller re-normalizes.
pub fn centroid_pull(points: &mut [(f32, f32)], labels: &[u32]) {
    let cents = centroids(labels, points);
    if cents.len() <= 1 {
        return;
    }
    let means: BTreeMap<u32, (f32, f32)> = cents
        .iter()
        .map(|(&c, &(sx, sy, cnt))| (c, (sx / cnt as f32, sy / cnt as f32)))
        .collect();
    let (mut gx, mut gy) = (0.0f32, 0.0f32);
    for (x, y) in means.values() {
        gx += x;
        gy += y;
    }
    gx /= means.len() as f32;
    gy /= means.len() as f32;

    for (i, p) in points.iter_mut().enumerate() {
        let Some(&(cx, cy)) = means.get(&labels[i]) else {
            continue;
        };
        let sx = gx + (cx - gx) * SPREAD_FACTOR;
        let sy = gy + (cy - gy) * SPREAD_FACTOR;
        p.0 = sx + (p.0 - cx) * PULL_FACTOR;
        p.1 = sy + (p.1 - cy) * PULL_FACTOR;
    }
}

/// Leaf placement: a node with exactly one relation edge sits on a small ring
/// around its only partner instead of drifting to its embedding position.
pub fn snap_leaves(points: &mut [(f32, f32)], edges: &[(usize, usize)]) {
    let n = points.len();
    let mut degree = vec![0u32; n];
    let mut partner = vec![usize::MAX; n];
    for &(a, b) in edges {
        degree[a] += 1;
        degree[b] += 1;
        partner[a] = b;
        partner[b] = a;
    }
    let radius = COORD_RANGE * 0.03;
    let mut ring_slot = vec![0u32; n];
    for i in 0..n {
        if degree[i] != 1 {
            continue;
        }
        let p = partner[i];
        if p == usize::MAX || degree[p] == 1 && p > i {
            // Leaf pairs (both degree 1): keep one anchored, snap the other.
            continue;
        }
        let slot = ring_slot[p];
        ring_slot[p] += 1;
        let angle = slot as f32 * 2.399_963; // golden angle keeps satellites spread
        points[i] = (
            points[p].0 + radius * angle.cos(),
            points[p].1 + radius * angle.sin(),
        );
    }
}

fn centroids(labels: &[u32], points: &[(f32, f32)]) -> HashMap<u32, (f32, f32, usize)> {
    let mut out: HashMap<u32, (f32, f32, usize)> = HashMap::new();
    for (i, &l) in labels.iter().enumerate() {
        let e = out.entry(l).or_insert((0.0, 0.0, 0));
        e.0 += points[i].0;
        e.1 += points[i].1;
        e.2 += 1;
    }
    out
}

fn compact(labels: &mut Vec<u32>) {
    compact_slice(labels);
}

fn compact_slice(labels: &mut [u32]) {
    let mut dense: HashMap<u32, u32> = HashMap::new();
    let mut next = 0u32;
    for l in labels.iter_mut() {
        *l = *dense.entry(*l).or_insert_with(|| {
            let id = next;
            next += 1;
            id
        });
    }
}

#[cfg(test)]
mod tests;
