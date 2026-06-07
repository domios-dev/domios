//! LinLog force refinement (Noack's lin-log model, ForceAtlas2 variant):
//! attraction log(1+d) along edges, repulsion m/d via Barnes-Hut. Pulls
//! relation-connected nodes together so drawn edges shorten and cross less.

const ITERATIONS: usize = 150;
const REPULSION_K: f32 = 60.0;
const GRAVITY_K: f32 = 0.015;
const THETA: f32 = 0.9;
const SPEED: f32 = 0.12;
const SPEED_DECAY: f32 = 0.985;
const MAX_STEP: f32 = 30.0;

pub struct WeightedEdge {
    pub a: usize,
    pub b: usize,
    pub weight: f32,
}

/// Refine positions in place. `edges` are undirected; indices into `points`.
pub fn refine(points: &mut [(f32, f32)], edges: &[WeightedEdge]) {
    let n = points.len();
    if n < 3 || edges.is_empty() {
        return;
    }
    let mut speed = SPEED;
    let mut forces = vec![(0.0f32, 0.0f32); n];

    for _ in 0..ITERATIONS {
        for f in forces.iter_mut() {
            *f = (0.0, 0.0);
        }

        let tree = QuadTree::build(points);
        for (i, &(x, y)) in points.iter().enumerate() {
            let (fx, fy) = tree.repulsion(x, y, i);
            forces[i].0 += fx * REPULSION_K;
            forces[i].1 += fy * REPULSION_K;
        }

        for e in edges {
            let (ax, ay) = points[e.a];
            let (bx, by) = points[e.b];
            let dx = bx - ax;
            let dy = by - ay;
            let d = (dx * dx + dy * dy).sqrt().max(1e-3);
            // FA2 LinLog attraction: |F| = log(1 + d), along the edge.
            let f = (1.0 + d).ln() * e.weight / d;
            forces[e.a].0 += dx * f;
            forces[e.a].1 += dy * f;
            forces[e.b].0 -= dx * f;
            forces[e.b].1 -= dy * f;
        }

        for (i, &(x, y)) in points.iter().enumerate() {
            forces[i].0 -= x * GRAVITY_K;
            forces[i].1 -= y * GRAVITY_K;
        }

        for (i, p) in points.iter_mut().enumerate() {
            let fx = (forces[i].0 * speed).clamp(-MAX_STEP, MAX_STEP);
            let fy = (forces[i].1 * speed).clamp(-MAX_STEP, MAX_STEP);
            p.0 += fx;
            p.1 += fy;
        }
        speed *= SPEED_DECAY;
    }
}

/// Barnes-Hut quadtree over the point set; repulsion kernel m/d per node.
struct QuadTree {
    nodes: Vec<QtNode>,
}

#[derive(Clone)]
struct QtNode {
    cx: f32,
    cy: f32,
    half: f32,
    mass: f32,
    mx: f32,
    my: f32,
    children: Option<[usize; 4]>,
    point: Option<(f32, f32, usize)>,
}

impl QuadTree {
    fn build(points: &[(f32, f32)]) -> Self {
        let (mut min_x, mut min_y) = (f32::MAX, f32::MAX);
        let (mut max_x, mut max_y) = (f32::MIN, f32::MIN);
        for &(x, y) in points {
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
        let half = ((max_x - min_x).max(max_y - min_y) / 2.0).max(1.0) * 1.01;
        let root = QtNode {
            cx: (min_x + max_x) / 2.0,
            cy: (min_y + max_y) / 2.0,
            half,
            mass: 0.0,
            mx: 0.0,
            my: 0.0,
            children: None,
            point: None,
        };
        let mut tree = QuadTree { nodes: vec![root] };
        for (i, &(x, y)) in points.iter().enumerate() {
            tree.insert(0, x, y, i, 0);
        }
        tree.aggregate(0);
        tree
    }

    fn insert(&mut self, node: usize, x: f32, y: f32, id: usize, depth: usize) {
        if depth > 48 {
            return;
        }
        if self.nodes[node].children.is_none() {
            match self.nodes[node].point {
                None => {
                    self.nodes[node].point = Some((x, y, id));
                    return;
                }
                Some(existing) => {
                    self.split(node);
                    self.nodes[node].point = None;
                    let q = self.quadrant(node, existing.0, existing.1);
                    let child = self.nodes[node].children.unwrap()[q];
                    self.insert(child, existing.0, existing.1, existing.2, depth + 1);
                }
            }
        }
        let q = self.quadrant(node, x, y);
        let child = self.nodes[node].children.unwrap()[q];
        self.insert(child, x, y, id, depth + 1);
    }

    fn split(&mut self, node: usize) {
        let (cx, cy, h) = {
            let nd = &self.nodes[node];
            (nd.cx, nd.cy, nd.half / 2.0)
        };
        let mut ids = [0usize; 4];
        for (q, id) in ids.iter_mut().enumerate() {
            let ox = if q % 2 == 0 { -h } else { h };
            let oy = if q < 2 { -h } else { h };
            self.nodes.push(QtNode {
                cx: cx + ox,
                cy: cy + oy,
                half: h,
                mass: 0.0,
                mx: 0.0,
                my: 0.0,
                children: None,
                point: None,
            });
            *id = self.nodes.len() - 1;
        }
        self.nodes[node].children = Some(ids);
    }

    fn quadrant(&self, node: usize, x: f32, y: f32) -> usize {
        let nd = &self.nodes[node];
        let mut q = 0;
        if x >= nd.cx {
            q += 1;
        }
        if y >= nd.cy {
            q += 2;
        }
        q
    }

    fn aggregate(&mut self, node: usize) -> (f32, f32, f32) {
        let children = self.nodes[node].children;
        let mut mass = 0.0;
        let mut mx = 0.0;
        let mut my = 0.0;
        if let Some((x, y, _)) = self.nodes[node].point {
            mass += 1.0;
            mx += x;
            my += y;
        }
        if let Some(ids) = children {
            for c in ids {
                let (m, x, y) = self.aggregate(c);
                mass += m;
                mx += x;
                my += y;
            }
        }
        self.nodes[node].mass = mass;
        self.nodes[node].mx = mx;
        self.nodes[node].my = my;
        (mass, mx, my)
    }

    fn repulsion(&self, x: f32, y: f32, self_id: usize) -> (f32, f32) {
        let mut fx = 0.0;
        let mut fy = 0.0;
        let mut stack = vec![0usize];
        while let Some(node) = stack.pop() {
            let nd = &self.nodes[node];
            if nd.mass <= 0.0 {
                continue;
            }
            let (cmx, cmy) = (nd.mx / nd.mass, nd.my / nd.mass);
            let dx = x - cmx;
            let dy = y - cmy;
            let d = (dx * dx + dy * dy).sqrt().max(1e-3);
            let far_enough = (nd.half * 2.0) / d < THETA;
            if nd.children.is_none() || far_enough {
                if let Some((_, _, id)) = nd.point {
                    if id == self_id && nd.children.is_none() && nd.mass <= 1.0 {
                        continue;
                    }
                }
                // LinLog repulsion kernel: mass / d, falls off with distance.
                let f = nd.mass / (d * d);
                fx += dx * f;
                fy += dy * f;
            } else if let Some(ids) = nd.children {
                stack.extend_from_slice(&ids);
            }
        }
        (fx, fy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connected_nodes_get_closer() {
        // Two pairs far apart; each pair connected. Refinement must shorten edges.
        let mut points = vec![(-500.0, 0.0), (500.0, 0.0), (-500.0, 300.0), (500.0, 300.0)];
        // Too few nodes for refine (n<3 guard passes at 4) — connect 0-1 and 2-3.
        let edges = vec![
            WeightedEdge { a: 0, b: 1, weight: 1.0 },
            WeightedEdge { a: 2, b: 3, weight: 1.0 },
        ];
        let before01 = dist(points[0], points[1]);
        refine(&mut points, &edges);
        let after01 = dist(points[0], points[1]);
        assert!(after01 < before01, "edge must shorten: {before01} -> {after01}");
    }

    #[test]
    fn disconnected_nodes_stay_apart() {
        let mut points: Vec<(f32, f32)> = (0..40)
            .map(|i| (((i % 8) as f32) * 100.0 - 350.0, ((i / 8) as f32) * 100.0 - 200.0))
            .collect();
        let edges = vec![WeightedEdge { a: 0, b: 1, weight: 1.0 }];
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
        assert!(min_d > 1.0, "unconnected nodes must not collapse: min={min_d}");
    }

    #[test]
    fn empty_and_tiny_inputs_are_safe() {
        let mut empty: Vec<(f32, f32)> = Vec::new();
        refine(&mut empty, &[]);
        let mut two = vec![(0.0, 0.0), (1.0, 1.0)];
        refine(&mut two, &[WeightedEdge { a: 0, b: 1, weight: 1.0 }]);
        assert_eq!(two.len(), 2);
    }

    fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
        ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
    }
}
