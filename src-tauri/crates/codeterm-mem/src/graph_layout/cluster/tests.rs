use super::*;

fn two_blobs_adjacency() -> Vec<Vec<usize>> {
    // 0..5 fully connected, 6..11 fully connected, one bridge edge 5-6.
    let mut adj = vec![Vec::new(); 12];
    for i in 0..6 {
        for j in 0..6 {
            if i != j {
                adj[i].push(j);
            }
        }
    }
    for i in 6..12 {
        for j in 6..12 {
            if i != j {
                adj[i].push(j);
            }
        }
    }
    adj[5].push(6);
    adj[6].push(5);
    adj
}

#[test]
fn label_propagation_splits_two_blobs() {
    let labels = label_propagation(&two_blobs_adjacency());
    let a: std::collections::HashSet<u32> = labels[..6].iter().copied().collect();
    let b: std::collections::HashSet<u32> = labels[6..].iter().copied().collect();
    assert_eq!(a.len(), 1, "first blob one community: {labels:?}");
    assert_eq!(b.len(), 1, "second blob one community: {labels:?}");
    assert!(a.is_disjoint(&b), "blobs must differ: {labels:?}");
}

#[test]
fn label_propagation_is_deterministic() {
    let adj = two_blobs_adjacency();
    assert_eq!(label_propagation(&adj), label_propagation(&adj));
}

#[test]
fn merge_small_absorbs_tiny_cluster() {
    let points = vec![
        (0.0, 0.0),
        (1.0, 1.0),
        (2.0, 0.0),
        (3.0, 1.0),
        (500.0, 500.0),
    ];
    let mut labels = vec![0, 0, 0, 0, 7];
    merge_small(&mut labels, &points);
    assert!(labels.iter().all(|&l| l == labels[0]), "{labels:?}");
}

#[test]
fn centroid_pull_tightens_clusters() {
    let mut points: Vec<(f32, f32)> =
        vec![(-100.0, 0.0), (-60.0, 10.0), (100.0, 0.0), (60.0, -10.0)];
    let labels = vec![0, 0, 1, 1];
    let spread_before = (points[0].0 - points[1].0).abs();
    centroid_pull(&mut points, &labels);
    let spread_after = (points[0].0 - points[1].0).abs();
    assert!(
        spread_after < spread_before,
        "intra-cluster spread must shrink: {spread_before} -> {spread_after}"
    );
    assert!((spread_after / spread_before - 0.45).abs() < 1e-6);
    let inter_before = 160.0f32; // centroid gap before: -80 vs 80
    let inter_after = ((points[0].0 + points[1].0) / 2.0 - (points[2].0 + points[3].0) / 2.0).abs();
    assert!(
        inter_after > inter_before,
        "centroid gap must widen: {inter_before} -> {inter_after}"
    );
}

#[test]
fn centroid_pull_preserves_distinct_extremes_before_radial_normalization() {
    let original = vec![
        (3000.0, 4000.0),
        (3100.0, 4200.0),
        (-3000.0, -4000.0),
        (-3100.0, -4200.0),
    ];
    let labels = [4, 4, 9, 9];
    let mut points = original.clone();
    centroid_pull(&mut points, &labels);
    assert!(points[0].0 > 2.0 * COORD_RANGE);
    assert!(points[0].1 > 2.0 * COORD_RANGE);
    assert!(points[0].0 < points[1].0 && points[0].1 < points[1].1);
    let mut repeated = original;
    centroid_pull(&mut repeated, &labels);
    assert_eq!(points, repeated);
}

#[test]
fn attach_by_neighbors_joins_majority_community() {
    // Nodes 0-2 have edges (communities 0,0,1); 3 is edgeless, neighbors 0,1,2.
    let mut labels = vec![0, 0, 1, 99];
    let has_edge = vec![true, true, true, false];
    let knn = vec![vec![], vec![], vec![], vec![0, 1, 2]];
    attach_by_neighbors(&mut labels, &has_edge, &knn);
    assert_eq!(labels[3], 0, "{labels:?}");
    assert_eq!(
        &labels[..3],
        &[0, 0, 1],
        "edge-backed labels must not change"
    );
}

#[test]
fn split_oversized_caps_giant_cluster() {
    // 200 points in one community spread over a wide box.
    let mut rngish = 0u32;
    let mut points = Vec::new();
    for i in 0..200 {
        rngish = rngish.wrapping_mul(1664525).wrapping_add(1013904223);
        let x = (i % 20) as f32 * 50.0 + (rngish % 10) as f32;
        let y = (i / 20) as f32 * 50.0;
        points.push((x, y));
    }
    let mut labels = vec![0u32; 200];
    split_oversized(&mut labels, &points);
    let distinct: std::collections::HashSet<u32> = labels.iter().copied().collect();
    assert!(
        distinct.len() >= 4,
        "giant must split, got {}",
        distinct.len()
    );
    let max_size = ((200f64).sqrt() * 4.0).ceil() as usize;
    let mut counts: HashMap<u32, usize> = HashMap::new();
    for &l in &labels {
        *counts.entry(l).or_insert(0) += 1;
    }
    assert!(
        counts.values().all(|&c| c <= max_size + 1),
        "no piece may exceed the cap: {counts:?}"
    );
}

#[test]
fn snap_leaves_brings_leaf_to_partner() {
    let mut points: Vec<(f32, f32)> = vec![(0.0, 0.0), (900.0, 900.0), (10.0, 0.0)];
    // 1 is a leaf of hub 0; 0-2 keeps hub degree > 1.
    let edges = vec![(0usize, 1usize), (0, 2)];
    snap_leaves(&mut points, &edges);
    let d = ((points[1].0 - points[0].0).powi(2) + (points[1].1 - points[0].1).powi(2)).sqrt();
    assert!(d < COORD_RANGE * 0.05, "leaf must sit near hub, d={d}");
}

#[test]
fn empty_and_single_inputs_are_safe() {
    assert!(label_propagation(&[]).is_empty());
    let mut pts: Vec<(f32, f32)> = vec![(1.0, 1.0)];
    centroid_pull(&mut pts, &[0]);
    assert_eq!(pts[0], (1.0, 1.0));
}
