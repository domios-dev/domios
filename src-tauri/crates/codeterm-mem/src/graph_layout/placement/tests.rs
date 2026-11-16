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

#[test]
fn normalize_outlier_does_not_shrink_cloud() {
    let cluster: Vec<(f32, f32)> = (0..200).map(|i| (i as f32, i as f32 * 0.5)).collect();
    let baseline = normalize(&cluster);
    for outlier in [(1e9, -1e9), (-1e9, 1e9)] {
        let mut points = cluster.clone();
        points.push(outlier);
        let normalized = normalize(&points);
        let baseline_width = baseline[150].0 - baseline[50].0;
        let width = normalized[150].0 - normalized[50].0;
        assert!((width / baseline_width - 1.0).abs() < 0.02);
        assert!(normalized
            .iter()
            .all(|&(x, y)| x.abs() <= COORD_RANGE && y.abs() <= COORD_RANGE));
        assert!(normalized[200].0.abs() > COORD_RANGE - 1.0);
        assert_eq!(normalized, normalize(&points));
    }
}

#[test]
fn normalize_empty_singleton_and_coincident_points() {
    assert!(normalize(&[]).is_empty());
    assert_eq!(normalize(&[(12.0, -30.0)]), vec![(0.0, 0.0)]);
    assert_eq!(normalize(&[(12.0, -30.0); 100]), vec![(0.0, 0.0); 100]);
}
