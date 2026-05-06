use std::collections::HashMap;

const DAMPING: f64 = 0.85;
const ITERATIONS: usize = 100;
const CONVERGENCE_DELTA: f64 = 1e-6;

pub struct EdgeWeight {
    pub source: i64,
    pub target: i64,
    pub weight: f64,
}

pub fn personalized(
    nodes: &[i64],
    edges: &[EdgeWeight],
    personalization: Option<&HashMap<i64, f64>>,
) -> HashMap<i64, f64> {
    let n = nodes.len();
    if n == 0 {
        return HashMap::new();
    }
    let uniform = 1.0 / n as f64;

    let teleport: HashMap<i64, f64> = match personalization {
        Some(p) if p.values().any(|&v| v > 0.0) => {
            let total: f64 = p.values().sum();
            nodes
                .iter()
                .map(|id| (*id, p.get(id).copied().unwrap_or(0.0) / total))
                .collect()
        }
        _ => nodes.iter().map(|id| (*id, uniform)).collect(),
    };

    let mut out_weights: HashMap<i64, f64> = HashMap::new();
    let mut adjacency: HashMap<i64, Vec<(i64, f64)>> = HashMap::new();
    for e in edges {
        if e.weight <= 0.0 {
            continue;
        }
        adjacency.entry(e.source).or_default().push((e.target, e.weight));
        *out_weights.entry(e.source).or_insert(0.0) += e.weight;
    }

    let mut rank: HashMap<i64, f64> = nodes.iter().map(|id| (*id, uniform)).collect();

    for _ in 0..ITERATIONS {
        let mut next: HashMap<i64, f64> = nodes
            .iter()
            .map(|id| (*id, (1.0 - DAMPING) * teleport.get(id).copied().unwrap_or(0.0)))
            .collect();
        let mut dangling_mass = 0.0;
        for id in nodes {
            let r = *rank.get(id).unwrap_or(&0.0);
            match adjacency.get(id) {
                Some(neighbours) if !neighbours.is_empty() => {
                    let total = *out_weights.get(id).unwrap_or(&1.0);
                    for (target, weight) in neighbours {
                        let share = DAMPING * r * (weight / total);
                        *next.entry(*target).or_insert(0.0) += share;
                    }
                }
                _ => dangling_mass += r,
            }
        }
        if dangling_mass > 0.0 {
            for id in nodes {
                let teleport_share = teleport.get(id).copied().unwrap_or(0.0);
                *next.entry(*id).or_insert(0.0) += DAMPING * dangling_mass * teleport_share;
            }
        }

        let delta: f64 = next
            .iter()
            .map(|(k, v)| (v - rank.get(k).copied().unwrap_or(0.0)).abs())
            .sum();
        rank = next;
        if delta < CONVERGENCE_DELTA {
            break;
        }
    }

    rank
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_graph() {
        let r = personalized(&[], &[], None);
        assert!(r.is_empty());
    }

    #[test]
    fn no_edges_uniform() {
        let r = personalized(&[1, 2, 3], &[], None);
        let v: Vec<f64> = r.values().copied().collect();
        for x in &v {
            assert!((x - 1.0 / 3.0).abs() < 1e-6, "got {x}");
        }
    }

    #[test]
    fn hub_gets_higher_rank() {
        let edges = vec![
            EdgeWeight { source: 2, target: 1, weight: 1.0 },
            EdgeWeight { source: 3, target: 1, weight: 1.0 },
            EdgeWeight { source: 4, target: 1, weight: 1.0 },
        ];
        let r = personalized(&[1, 2, 3, 4], &edges, None);
        let r1 = r[&1];
        let r2 = r[&2];
        assert!(r1 > r2, "hub {r1} should outrank leaf {r2}");
    }

    #[test]
    fn personalization_biases_target() {
        let edges = vec![EdgeWeight { source: 2, target: 1, weight: 1.0 }];
        let mut p = HashMap::new();
        p.insert(2, 1.0);
        let r = personalized(&[1, 2], &edges, Some(&p));
        assert!(r[&2] > r[&1], "personalized seed should outrank unbiased");
    }

    #[test]
    fn ranks_sum_close_to_one() {
        let edges = vec![
            EdgeWeight { source: 1, target: 2, weight: 1.0 },
            EdgeWeight { source: 2, target: 3, weight: 1.0 },
            EdgeWeight { source: 3, target: 1, weight: 1.0 },
        ];
        let r = personalized(&[1, 2, 3], &edges, None);
        let sum: f64 = r.values().sum();
        assert!((sum - 1.0).abs() < 1e-3, "sum={sum}");
    }
}
