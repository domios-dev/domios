use crate::cmd::map::store::Store;
use rusqlite::params;
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

#[derive(Debug, Serialize, Clone)]
pub struct GraphPayload {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub stats: GraphStats,
    pub truncated: bool,
}

#[derive(Debug, Serialize, Clone)]
pub struct GraphNode {
    pub id: i64,
    pub kind: String,
    pub display_name: String,
    pub serialized_name: String,
    pub path: Option<String>,
    pub start_line: Option<i64>,
    pub start_col: Option<i64>,
    pub end_line: Option<i64>,
    pub end_col: Option<i64>,
}

#[derive(Debug, Serialize, Clone)]
pub struct GraphEdge {
    pub source: i64,
    pub target: i64,
    pub kind: String,
    pub weight: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Clone, Copy)]
pub struct GraphStats {
    pub files: i64,
    pub nodes: i64,
    pub edges: i64,
}

pub const DEFAULT_NODE_CAP: usize = 5_000;

pub struct GraphQuery {
    store: Store,
}

impl GraphQuery {
    pub fn open(workspace_root: &Path) -> Result<GraphQuery, String> {
        let store = Store::open(workspace_root).map_err(|e| format!("open store: {e}"))?;
        Ok(GraphQuery { store })
    }

    pub fn full_graph(&self, node_cap: usize) -> Result<GraphPayload, String> {
        let counts = self.store.counts().map_err(|e| format!("counts: {e}"))?;
        let total_nodes = counts.nodes;
        let truncated = (total_nodes as usize) > node_cap;

        let mut stmt = self
            .store
            .conn
            .prepare(
                "SELECT n.id, n.kind, n.display_name, n.serialized_name, f.path, \
                        n.start_line, n.start_col, n.end_line, n.end_col \
                 FROM node n LEFT JOIN file f ON n.file_id = f.id \
                 ORDER BY (CASE WHEN n.kind = 'file' THEN 0 ELSE 1 END), n.id LIMIT ?1",
            )
            .map_err(|e| format!("prepare nodes: {e}"))?;

        let cap = if truncated { node_cap as i64 } else { total_nodes };
        let nodes = collect_nodes(&mut stmt, cap)?;
        if nodes.is_empty() {
            return Ok(GraphPayload {
                nodes,
                edges: Vec::new(),
                stats: GraphStats {
                    files: counts.files,
                    nodes: counts.nodes,
                    edges: counts.edges,
                },
                truncated,
            });
        }

        let id_set: HashSet<i64> = nodes.iter().map(|n| n.id).collect();
        let edges = collect_edges_within(&self.store, &id_set)?;

        Ok(GraphPayload {
            nodes,
            edges,
            stats: GraphStats {
                files: counts.files,
                nodes: counts.nodes,
                edges: counts.edges,
            },
            truncated,
        })
    }

    pub fn focus_subgraph(&self, node_id: i64, k_hops: u32) -> Result<GraphPayload, String> {
        let mut visited: HashSet<i64> = HashSet::new();
        let mut frontier: VecDeque<(i64, u32)> = VecDeque::new();
        frontier.push_back((node_id, 0));
        visited.insert(node_id);

        while let Some((id, hop)) = frontier.pop_front() {
            if hop >= k_hops {
                continue;
            }
            let mut stmt = self
                .store
                .conn
                .prepare(
                    "SELECT source_node_id, target_node_id FROM edge \
                     WHERE source_node_id = ?1 OR target_node_id = ?1",
                )
                .map_err(|e| format!("prepare neighbours: {e}"))?;
            let rows = stmt
                .query_map(params![id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))
                .map_err(|e| format!("query neighbours: {e}"))?;
            for row in rows.flatten() {
                let neighbour = if row.0 == id { row.1 } else { row.0 };
                if visited.insert(neighbour) {
                    frontier.push_back((neighbour, hop + 1));
                }
            }
        }

        if visited.is_empty() {
            return Ok(empty_payload());
        }

        let nodes = collect_nodes_by_ids(&self.store, &visited)?;
        let edges = collect_edges_within(&self.store, &visited)?;
        let counts = self.store.counts().map_err(|e| format!("counts: {e}"))?;

        Ok(GraphPayload {
            nodes,
            edges,
            stats: GraphStats {
                files: counts.files,
                nodes: counts.nodes,
                edges: counts.edges,
            },
            truncated: false,
        })
    }

    pub fn node_detail(&self, node_id: i64) -> Result<Option<GraphNode>, String> {
        let mut stmt = self
            .store
            .conn
            .prepare(
                "SELECT n.id, n.kind, n.display_name, n.serialized_name, f.path, \
                        n.start_line, n.start_col, n.end_line, n.end_col \
                 FROM node n LEFT JOIN file f ON n.file_id = f.id WHERE n.id = ?1",
            )
            .map_err(|e| format!("prepare node detail: {e}"))?;
        let mut rows = stmt
            .query(params![node_id])
            .map_err(|e| format!("query node detail: {e}"))?;
        if let Some(row) = rows.next().map_err(|e| format!("row: {e}"))? {
            return Ok(Some(GraphNode {
                id: row.get(0).map_err(|e| e.to_string())?,
                kind: row.get(1).map_err(|e| e.to_string())?,
                display_name: row.get(2).map_err(|e| e.to_string())?,
                serialized_name: row.get(3).map_err(|e| e.to_string())?,
                path: row.get(4).map_err(|e| e.to_string())?,
                start_line: row.get(5).map_err(|e| e.to_string())?,
                start_col: row.get(6).map_err(|e| e.to_string())?,
                end_line: row.get(7).map_err(|e| e.to_string())?,
                end_col: row.get(8).map_err(|e| e.to_string())?,
            }));
        }
        Ok(None)
    }
}

fn collect_nodes(
    stmt: &mut rusqlite::Statement<'_>,
    cap: i64,
) -> Result<Vec<GraphNode>, String> {
    let rows = stmt
        .query_map(params![cap], row_to_node)
        .map_err(|e| format!("query nodes: {e}"))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| format!("node row: {e}"))?);
    }
    Ok(out)
}

fn collect_nodes_by_ids(
    store: &Store,
    ids: &HashSet<i64>,
) -> Result<Vec<GraphNode>, String> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = std::iter::repeat("?").take(ids.len()).collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT n.id, n.kind, n.display_name, n.serialized_name, f.path, \
                n.start_line, n.start_col, n.end_line, n.end_col \
         FROM node n LEFT JOIN file f ON n.file_id = f.id WHERE n.id IN ({})",
        placeholders
    );
    let mut stmt = store.conn.prepare(&sql).map_err(|e| format!("prepare ids: {e}"))?;
    let id_vec: Vec<i64> = ids.iter().copied().collect();
    let rows = stmt
        .query_map(rusqlite::params_from_iter(id_vec.iter()), row_to_node)
        .map_err(|e| format!("query ids: {e}"))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| format!("id row: {e}"))?);
    }
    Ok(out)
}

fn collect_edges_within(
    store: &Store,
    id_set: &HashSet<i64>,
) -> Result<Vec<GraphEdge>, String> {
    if id_set.is_empty() {
        return Ok(Vec::new());
    }
    let mut stmt = store
        .conn
        .prepare("SELECT source_node_id, target_node_id, kind, weight, metadata FROM edge")
        .map_err(|e| format!("prepare edges: {e}"))?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, f64>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        })
        .map_err(|e| format!("query edges: {e}"))?;
    let mut edges = Vec::new();
    for row in rows.flatten() {
        if id_set.contains(&row.0) && id_set.contains(&row.1) {
            edges.push(GraphEdge {
                source: row.0,
                target: row.1,
                kind: row.2,
                weight: row.3,
                metadata: row.4.and_then(|s| serde_json::from_str(&s).ok()),
            });
        }
    }
    Ok(edges)
}

fn row_to_node(row: &rusqlite::Row<'_>) -> rusqlite::Result<GraphNode> {
    Ok(GraphNode {
        id: row.get(0)?,
        kind: row.get(1)?,
        display_name: row.get(2)?,
        serialized_name: row.get(3)?,
        path: row.get(4)?,
        start_line: row.get(5)?,
        start_col: row.get(6)?,
        end_line: row.get(7)?,
        end_col: row.get(8)?,
    })
}

fn empty_payload() -> GraphPayload {
    GraphPayload {
        nodes: Vec::new(),
        edges: Vec::new(),
        stats: GraphStats { files: 0, nodes: 0, edges: 0 },
        truncated: false,
    }
}

// Helper used by tests — silence dead-code in non-test builds.
#[allow(dead_code)]
fn _suppress_unused_hashmap() -> HashMap<i64, i64> { HashMap::new() }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::map::index::{run_index, IndexOptions};

    fn build_fixture() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("src/lib.rs"),
            "pub fn alpha_helper() -> i32 { 1 }\npub fn beta_helper() -> i32 { 2 }\n",
        )
        .unwrap();
        std::fs::write(
            root.join("src/main.rs"),
            "fn main() { let _ = alpha_helper(); let _ = beta_helper(); }\n",
        )
        .unwrap();
        run_index(root, IndexOptions { force: true, verbose: false }).unwrap();
        tmp
    }

    #[test]
    fn full_graph_returns_nodes_and_edges() {
        let tmp = build_fixture();
        let q = GraphQuery::open(tmp.path()).unwrap();
        let p = q.full_graph(DEFAULT_NODE_CAP).unwrap();
        assert!(p.nodes.iter().any(|n| n.display_name == "alpha_helper"));
        assert!(p.nodes.iter().any(|n| n.display_name == "beta_helper"));
        assert!(p.stats.nodes >= 4);
        assert!(!p.truncated);
        let kinds: HashSet<String> = p.edges.iter().map(|e| e.kind.clone()).collect();
        assert!(kinds.contains("contain"));
    }

    #[test]
    fn full_graph_truncates_above_cap() {
        let tmp = build_fixture();
        let q = GraphQuery::open(tmp.path()).unwrap();
        let p = q.full_graph(2).unwrap();
        assert_eq!(p.nodes.len(), 2);
        assert!(p.truncated);
    }

    #[test]
    fn focus_subgraph_returns_subset() {
        let tmp = build_fixture();
        let q = GraphQuery::open(tmp.path()).unwrap();
        let full = q.full_graph(DEFAULT_NODE_CAP).unwrap();
        let alpha = full.nodes.iter().find(|n| n.display_name == "alpha_helper").unwrap();
        let sub = q.focus_subgraph(alpha.id, 1).unwrap();
        assert!(sub.nodes.iter().any(|n| n.id == alpha.id));
        assert!(sub.nodes.len() < full.nodes.len() || full.nodes.len() <= 2);
    }

    #[test]
    fn node_detail_returns_metadata() {
        let tmp = build_fixture();
        let q = GraphQuery::open(tmp.path()).unwrap();
        let full = q.full_graph(DEFAULT_NODE_CAP).unwrap();
        let any_node = full.nodes.first().unwrap().id;
        let detail = q.node_detail(any_node).unwrap().expect("node detail");
        assert_eq!(detail.id, any_node);
    }
}
