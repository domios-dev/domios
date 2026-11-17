use super::*;

#[test]
fn connected_nodes_get_closer() {
    // Two pairs far apart; each pair connected. Refinement must shorten edges.
    let mut points = vec![(-500.0, 0.0), (500.0, 0.0), (-500.0, 300.0), (500.0, 300.0)];
    // Too few nodes for refine (n<3 guard passes at 4) — connect 0-1 and 2-3.
    let edges = vec![
        WeightedEdge {
            a: 0,
            b: 1,
            weight: 1.0,
        },
        WeightedEdge {
            a: 2,
            b: 3,
            weight: 1.0,
        },
    ];
    let before01 = dist(points[0], points[1]);
    refine(&mut points, &edges);
    let after01 = dist(points[0], points[1]);
    assert!(
        after01 < before01,
        "edge must shorten: {before01} -> {after01}"
    );
}

#[test]
fn disconnected_nodes_stay_apart() {
    let mut points: Vec<(f32, f32)> = (0..40)
        .map(|i| {
            (
                ((i % 8) as f32) * 100.0 - 350.0,
                ((i / 8) as f32) * 100.0 - 200.0,
            )
        })
        .collect();
    let edges = vec![WeightedEdge {
        a: 0,
        b: 1,
        weight: 1.0,
    }];
    refine(&mut points, &edges);
    let mut min_d = f32::MAX;
    for i in 0..points.len() {
        for j in (i + 1)..points.len() {
            if (i, j) == (0, 1) {
                continue;
            }
            min_d = min_d.min(dist(points[i], points[j]));
        }
    }
    assert!(
        min_d > 1.0,
        "unconnected nodes must not collapse: min={min_d}"
    );
}

#[test]
fn empty_and_tiny_inputs_are_safe() {
    let mut empty: Vec<(f32, f32)> = Vec::new();
    refine(&mut empty, &[]);
    let mut two = vec![(0.0, 0.0), (1.0, 1.0)];
    refine(
        &mut two,
        &[WeightedEdge {
            a: 0,
            b: 1,
            weight: 1.0,
        }],
    );
    assert_eq!(two.len(), 2);
}

fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

#[test]
fn isolated_nodes_keep_semantic_seed_and_radial_spread() {
    for count in [20, 100, 300] {
        let mut seeds = vec![(-100.0, -100.0), (100.0, -100.0), (0.0, 100.0)];
        seeds.extend((0..count).map(|i| {
            let angle = i as f32 * 2.399_963_1;
            let radius = 20.0 + i as f32 * 3.0;
            (radius * angle.cos(), radius * angle.sin())
        }));
        let edges = [
            WeightedEdge {
                a: 0,
                b: 1,
                weight: 2.0,
            },
            WeightedEdge {
                a: 1,
                b: 2,
                weight: 0.3,
            },
            WeightedEdge {
                a: 2,
                b: 0,
                weight: 2.0,
            },
        ];
        let mut points = seeds.clone();
        refine(&mut points, &edges);
        assert_eq!(&points[3..], &seeds[3..]);
        assert_ne!(&points[..3], &seeds[..3]);
        let radii: Vec<f32> = points[3..].iter().map(|&p| dist(p, (0.0, 0.0))).collect();
        let mean = radii.iter().sum::<f32>() / count as f32;
        let variance = radii.iter().map(|r| (r - mean).powi(2)).sum::<f32>() / count as f32;
        assert!(
            variance.sqrt() / mean > 0.35,
            "isolates must not form a ring"
        );
        let mut repeated = seeds;
        refine(&mut repeated, &edges);
        assert_eq!(points, repeated);
    }
}
