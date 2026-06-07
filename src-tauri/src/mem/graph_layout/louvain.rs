//! Louvain community detection (Blondel et al. 2008) over the weighted
//! relation graph, with a Leiden-style post-pass: communities are split into
//! connected components so no community is internally disconnected.

use std::collections::HashMap;

const MAX_LEVELS: usize = 10;
const MAX_LOCAL_PASSES: usize = 16;

pub struct LouvainEdge {
    pub a: usize,
    pub b: usize,
    pub weight: f32,
}

/// Returns a community id per node (0..n), compacted. Isolated nodes (no
/// edges) each get their own singleton community.
pub fn louvain(n: usize, edges: &[LouvainEdge]) -> Vec<u32> {
    if n == 0 {
        return Vec::new();
    }
    // node → community at the finest level, refined through aggregation levels.
    let mut membership: Vec<usize> = (0..n).collect();
    let mut level_nodes = n;
    let mut level_edges: Vec<(usize, usize, f64)> = edges
        .iter()
        .map(|e| (e.a, e.b, e.weight as f64))
        .collect();

    for _ in 0..MAX_LEVELS {
        let (assign, moved) = one_level(level_nodes, &level_edges);
        if !moved {
            break;
        }
        let compacted = compact_assign(&assign);
        let n_comms = compacted.iter().copied().max().map(|m| m + 1).unwrap_or(0);
        for m in membership.iter_mut() {
            *m = compacted[*m];
        }
        let mut agg: HashMap<(usize, usize), f64> = HashMap::new();
        for &(a, b, w) in &level_edges {
            let (ca, cb) = (compacted[a], compacted[b]);
            let key = if ca <= cb { (ca, cb) } else { (cb, ca) };
            *agg.entry(key).or_insert(0.0) += w;
        }
        level_nodes = n_comms;
        level_edges = agg.into_iter().map(|((a, b), w)| (a, b, w)).collect();
        if n_comms == 1 {
            break;
        }
    }

    let mut labels: Vec<u32> = membership.iter().map(|&m| m as u32).collect();
    split_disconnected(&mut labels, n, edges);
    compact_u32(&mut labels);
    labels
}

/// One Louvain level: greedy modularity local moves until convergence.
/// Returns (assignment per node, whether anything moved).
fn one_level(n: usize, edges: &[(usize, usize, f64)]) -> (Vec<usize>, bool) {
    let mut adj: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
    let mut self_loops = vec![0.0f64; n];
    let mut two_m = 0.0f64;
    for &(a, b, w) in edges {
        two_m += 2.0 * w;
        if a == b {
            self_loops[a] += w;
            continue;
        }
        adj[a].push((b, w));
        adj[b].push((a, w));
    }
    if two_m <= 0.0 {
        return ((0..n).collect(), false);
    }

    let degree: Vec<f64> = (0..n)
        .map(|i| adj[i].iter().map(|(_, w)| w).sum::<f64>() + 2.0 * self_loops[i])
        .collect();
    let mut community: Vec<usize> = (0..n).collect();
    let mut comm_total: Vec<f64> = degree.clone();
    let mut moved_any = false;

    for _ in 0..MAX_LOCAL_PASSES {
        let mut moved = false;
        for i in 0..n {
            let cur = community[i];
            comm_total[cur] -= degree[i];

            let mut links: HashMap<usize, f64> = HashMap::new();
            links.insert(cur, 0.0);
            for &(j, w) in &adj[i] {
                *links.entry(community[j]).or_insert(0.0) += w;
            }

            let mut best = cur;
            let mut best_gain = 0.0f64;
            let base = links.get(&cur).copied().unwrap_or(0.0)
                - comm_total[cur] * degree[i] / two_m;
            for (&c, &k_in) in &links {
                if c == cur {
                    continue;
                }
                let gain = (k_in - comm_total[c] * degree[i] / two_m) - base;
                if gain > best_gain + 1e-12 {
                    best_gain = gain;
                    best = c;
                }
            }

            community[i] = best;
            comm_total[best] += degree[i];
            if best != cur {
                moved = true;
                moved_any = true;
            }
        }
        if !moved {
            break;
        }
    }
    (community, moved_any)
}

/// Leiden-style guarantee: a community must be connected. BFS each community
/// over the original edges; extra components get fresh ids.
fn split_disconnected(labels: &mut [u32], n: usize, edges: &[LouvainEdge]) {
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for e in edges {
        if e.a != e.b && labels[e.a] == labels[e.b] {
            adj[e.a].push(e.b);
            adj[e.b].push(e.a);
        }
    }
    let mut next = labels.iter().copied().max().unwrap_or(0) + 1;
    let mut seen = vec![false; n];
    let mut comp_of_label: HashMap<u32, bool> = HashMap::new();
    for start in 0..n {
        if seen[start] {
            continue;
        }
        let label = labels[start];
        let first_component = !comp_of_label.get(&label).copied().unwrap_or(false);
        comp_of_label.insert(label, true);
        let assign = if first_component {
            label
        } else {
            let id = next;
            next += 1;
            id
        };
        let mut queue = vec![start];
        seen[start] = true;
        while let Some(v) = queue.pop() {
            labels[v] = assign;
            for &u in &adj[v] {
                if !seen[u] {
                    seen[u] = true;
                    queue.push(u);
                }
            }
        }
    }
}

fn compact_assign(assign: &[usize]) -> Vec<usize> {
    let mut dense: HashMap<usize, usize> = HashMap::new();
    let mut next = 0usize;
    assign
        .iter()
        .map(|&a| {
            *dense.entry(a).or_insert_with(|| {
                let id = next;
                next += 1;
                id
            })
        })
        .collect()
}

fn compact_u32(labels: &mut [u32]) {
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
mod tests {
    use super::*;

    fn clique_edges(nodes: &[usize]) -> Vec<LouvainEdge> {
        let mut out = Vec::new();
        for (i, &a) in nodes.iter().enumerate() {
            for &b in &nodes[i + 1..] {
                out.push(LouvainEdge { a, b, weight: 1.0 });
            }
        }
        out
    }

    #[test]
    fn two_cliques_with_bridge_split() {
        let mut edges = clique_edges(&[0, 1, 2, 3, 4]);
        edges.extend(clique_edges(&[5, 6, 7, 8, 9]));
        edges.push(LouvainEdge { a: 4, b: 5, weight: 1.0 });
        let labels = louvain(10, &edges);
        let a: std::collections::HashSet<u32> = labels[..5].iter().copied().collect();
        let b: std::collections::HashSet<u32> = labels[5..].iter().copied().collect();
        assert_eq!(a.len(), 1, "{labels:?}");
        assert_eq!(b.len(), 1, "{labels:?}");
        assert!(a.is_disjoint(&b), "{labels:?}");
    }

    #[test]
    fn three_communities_detected() {
        let mut edges = clique_edges(&[0, 1, 2, 3]);
        edges.extend(clique_edges(&[4, 5, 6, 7]));
        edges.extend(clique_edges(&[8, 9, 10, 11]));
        edges.push(LouvainEdge { a: 3, b: 4, weight: 0.2 });
        edges.push(LouvainEdge { a: 7, b: 8, weight: 0.2 });
        let labels = louvain(12, &edges);
        let distinct: std::collections::HashSet<u32> = labels.iter().copied().collect();
        assert_eq!(distinct.len(), 3, "{labels:?}");
    }

    #[test]
    fn isolated_nodes_get_singletons() {
        let edges = clique_edges(&[0, 1, 2]);
        let labels = louvain(5, &edges);
        assert_eq!(labels[0], labels[1]);
        assert_eq!(labels[1], labels[2]);
        assert_ne!(labels[3], labels[0]);
        assert_ne!(labels[4], labels[3], "isolated nodes must not share: {labels:?}");
    }

    #[test]
    fn disconnected_community_is_split() {
        // Force a pathological label set via a graph where modularity merges
        // can't happen: two separate cliques, no bridge. Louvain should give
        // two communities anyway; the split pass guarantees it.
        let mut edges = clique_edges(&[0, 1, 2]);
        edges.extend(clique_edges(&[3, 4, 5]));
        let labels = louvain(6, &edges);
        let a: std::collections::HashSet<u32> = labels[..3].iter().copied().collect();
        let b: std::collections::HashSet<u32> = labels[3..].iter().copied().collect();
        assert!(a.is_disjoint(&b), "{labels:?}");
    }

    #[test]
    fn weights_drive_membership() {
        // Node 2 weakly tied to clique {0,1}, strongly to clique {3,4}.
        let edges = vec![
            LouvainEdge { a: 0, b: 1, weight: 1.0 },
            LouvainEdge { a: 0, b: 2, weight: 0.1 },
            LouvainEdge { a: 3, b: 4, weight: 1.0 },
            LouvainEdge { a: 2, b: 3, weight: 1.0 },
            LouvainEdge { a: 2, b: 4, weight: 1.0 },
        ];
        let labels = louvain(5, &edges);
        assert_eq!(labels[2], labels[3], "{labels:?}");
        assert_eq!(labels[2], labels[4], "{labels:?}");
        assert_ne!(labels[2], labels[0], "{labels:?}");
    }
}
