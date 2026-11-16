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
        assert!(normalized[200].0.hypot(normalized[200].1) > COORD_RANGE - 1.0);
        assert_eq!(normalized, normalize(&points));
    }
}

#[test]
fn normalize_empty_singleton_and_coincident_points() {
    assert!(normalize(&[]).is_empty());
    assert_eq!(normalize(&[(12.0, -30.0)]), vec![(0.0, 0.0)]);
    assert_eq!(normalize(&[(12.0, -30.0); 100]), vec![(0.0, 0.0); 100]);
}

fn clustered_fixture() -> Vec<(f32, f32)> {
    let mut points = Vec::new();
    for i in 0..200 {
        let angle = i as f32 * 2.399_963_1;
        let radius = 0.01 + (i as f32 / 200.0).sqrt() * 0.4;
        let point = (1.0 + radius * angle.cos(), 0.6 + radius * angle.sin());
        points.extend([point, (-point.0, -point.1)]);
    }
    for i in 0..50 {
        let angle = i as f32 * 2.399_963_1;
        let radius = 10.0 + i as f32 * 0.1;
        let point = (radius * angle.cos(), radius * angle.sin());
        points.extend([point, (-point.0, -point.1)]);
    }
    points
}

fn legacy_normalize(points: &[(f32, f32)]) -> Vec<(f32, f32)> {
    let mut xs: Vec<_> = points.iter().map(|p| p.0).collect();
    let mut ys: Vec<_> = points.iter().map(|p| p.1).collect();
    xs.sort_unstable_by(f32::total_cmp);
    ys.sort_unstable_by(f32::total_cmp);
    let trim = points.len() / 50;
    let (min_x, max_x) = (xs[trim], xs[points.len() - 1 - trim]);
    let (min_y, max_y) = (ys[trim], ys[points.len() - 1 - trim]);
    let scale = 2.0 * COORD_RANGE / (max_x - min_x).max(max_y - min_y).max(1e-6);
    points
        .iter()
        .map(|&(x, y)| {
            (
                (x.clamp(min_x, max_x) - (min_x + max_x) / 2.0) * scale,
                (y.clamp(min_y, max_y) - (min_y + max_y) / 2.0) * scale,
            )
        })
        .collect()
}

fn median_nearest_neighbor(points: &[(f32, f32)]) -> f32 {
    let mut distances: Vec<_> = points
        .iter()
        .enumerate()
        .map(|(i, &(x, y))| {
            points
                .iter()
                .enumerate()
                .filter(|(j, _)| i != *j)
                .map(|(_, &(nx, ny))| (nx - x).hypot(ny - y))
                .fold(f32::INFINITY, f32::min)
        })
        .collect();
    distances.sort_unstable_by(f32::total_cmp);
    distances[distances.len() / 2]
}

#[test]
fn normalize_opens_clustered_core_against_previous_layout() {
    let points = clustered_fixture();
    let previous = legacy_normalize(&points);
    let radial = normalize(&points);
    let unopened = normalize_seed(&points);
    let previous_spacing = median_nearest_neighbor(&previous[..400]);
    let spacing = median_nearest_neighbor(&radial[..400]);
    assert!(
        spacing > previous_spacing * 1.2,
        "{spacing} vs {previous_spacing}"
    );
    assert!(spacing > median_nearest_neighbor(&unopened[..400]) * 1.2);
    assert_eq!(radial, normalize(&points));
    let reversed: Vec<_> = points.iter().copied().rev().collect();
    assert_eq!(
        radial,
        normalize(&reversed).into_iter().rev().collect::<Vec<_>>()
    );
}

#[test]
fn normalize_preserves_directions_with_a_tight_round_halo() {
    let points = clustered_fixture();
    let radial = normalize(&points);
    let mut radii: Vec<_> = radial.iter().map(|&(x, y)| x.hypot(y)).collect();
    radii.sort_unstable_by(f32::total_cmp);
    let robust = radii[(radii.len() - 1) * 95 / 100];
    assert!(radii[radii.len() - 1] <= robust * 1.15 + 0.001);
    assert!(radii[radii.len() - 1] <= COORD_RANGE);
    for (&(x, y), &(nx, ny)) in points.iter().zip(&radial) {
        let cross = (x * ny - y * nx).abs() / (x.hypot(y) * nx.hypot(ny));
        assert!(cross < 1e-6);
        assert!(x * nx + y * ny > 0.0);
    }
    for axis in 0..2 {
        let mut coordinates: Vec<_> = radial
            .iter()
            .map(|p| if axis == 0 { p.0 } else { p.1 })
            .collect();
        coordinates.sort_unstable_by(f32::total_cmp);
        assert!(coordinates[0] < coordinates[1]);
        assert!(coordinates[coordinates.len() - 2] < coordinates[coordinates.len() - 1]);
    }
}

#[test]
fn normalize_tail_compresses_without_a_shared_boundary() {
    let mut points: Vec<_> = (0..200)
        .flat_map(|i| {
            let angle = i as f32 * 2.399_963_1;
            let point = (angle.cos(), angle.sin());
            [point, (-point.0, -point.1)]
        })
        .collect();
    for radius in [2.0, 3.0, 5.0, 10.0] {
        points.extend([(radius, 0.0), (-radius, 0.0)]);
    }
    let radial = normalize(&points);
    let tail: Vec<_> = radial[400..].iter().step_by(2).map(|p| p.0).collect();
    assert!(tail.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(tail[3] < COORD_RANGE);
}
