//! Read-only snapshot/delta builders for the graph viewer.

use std::collections::{HashMap, HashSet};

use rusqlite::Connection;
use serde::Serialize;

use super::cache::{LayoutCache, Position};
use super::placement::COORD_RANGE;

pub const MAX_IDS: usize = 500;

#[derive(Debug, Clone, Serialize)]
pub struct SnapshotNode {
    pub id: String,
    pub x: f32,
    pub y: f32,
    pub kind: String,
    pub importance: f64,
    pub cluster_id: u32,
    pub has_vector: bool,
    pub pinned: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClusterAgg {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    pub size: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClusterEdge {
    pub from: u32,
    pub to: u32,
    pub weight: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SnapshotStats {
    pub node_count: usize,
    pub cluster_count: usize,
    pub edge_count: usize,
    pub vectorized_without_position: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub layout_version: u64,
    pub computed_at: String,
    pub seq: u64,
    pub nodes: Vec<SnapshotNode>,
    pub clusters: Vec<ClusterAgg>,
    pub cluster_edges: Vec<ClusterEdge>,
    pub stats: SnapshotStats,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeltaNode {
    pub id: String,
    pub x: f32,
    pub y: f32,
    pub cluster_id: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum DeltaResponse {
    Unchanged {
        layout_version: u64,
        unchanged: bool,
    },
    Changed {
        layout_version: u64,
        seq: u64,
        added_or_moved: Vec<DeltaNode>,
        removed: Vec<String>,
    },
}

#[derive(Clone)]
struct ItemMeta {
    kind: String,
    importance: f64,
    has_vector: bool,
}

/// Read kind/importance for all live (non-archived) items + entities. Entities
/// carry kind="entity" and importance 0; vectors detected from mem_vectors.
fn load_item_meta(conn: &Connection) -> Result<HashMap<String, ItemMeta>, String> {
    let vector_ids: HashSet<String> = {
        let mut set = HashSet::new();
        if let Ok(mut stmt) = conn.prepare("SELECT mem_id FROM mem_vectors") {
            if let Ok(rows) = stmt.query_map([], |r| r.get::<_, String>(0)) {
                for r in rows.flatten() {
                    let canonical = super::placement::strip_chunk_suffix(&r).to_string();
                    set.insert(canonical);
                }
            }
        }
        set
    };

    let mut meta: HashMap<String, ItemMeta> = HashMap::new();
    let mut stmt = conn
        .prepare("SELECT id, kind, importance FROM mem_items WHERE kind != 'archived'")
        .map_err(|e| format!("prepare meta: {e}"))?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, f64>(2)?,
            ))
        })
        .map_err(|e| format!("query meta: {e}"))?;
    for r in rows {
        let (id, kind, importance) = r.map_err(|e| e.to_string())?;
        let has_vector = vector_ids.contains(&id);
        meta.insert(
            id,
            ItemMeta {
                kind,
                importance,
                has_vector,
            },
        );
    }

    if let Ok(mut estmt) = conn.prepare("SELECT id FROM entities") {
        if let Ok(rows) = estmt.query_map([], |r| r.get::<_, String>(0)) {
            for id in rows.flatten() {
                meta.entry(id.clone()).or_insert(ItemMeta {
                    kind: "entity".into(),
                    importance: 0.0,
                    has_vector: vector_ids.contains(&id),
                });
            }
        }
    }
    Ok(meta)
}

fn fallback_position(id: &str, index: usize, count: usize) -> Position {
    let h = stable_hash(id);
    let angle = (h as f64 / u64::MAX as f64) * std::f64::consts::TAU;
    let radius = (count.max(1) as f64).sqrt() * (COORD_RANGE as f64 * 0.025);
    let jitter = (((h >> 32) as f64 / u32::MAX as f64) - 0.5) * radius * 0.35;
    Position {
        x: (angle.cos() * (radius + jitter)) as f32,
        y: (angle.sin() * (radius + jitter)) as f32,
        cluster_id: (index / 64) as u32,
    }
}

fn stable_hash(s: &str) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn load_pinned(conn: &Connection) -> HashMap<String, (f32, f32)> {
    let mut out = HashMap::new();
    if let Ok(mut stmt) = conn.prepare("SELECT mem_id, x, y FROM graph_pinned_positions") {
        if let Ok(rows) = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, f64>(1)?,
                r.get::<_, f64>(2)?,
            ))
        }) {
            for r in rows.flatten() {
                out.insert(r.0, (r.1 as f32, r.2 as f32));
            }
        }
    }
    out
}

/// Build the full snapshot from the cache + read-only DB metadata.
pub fn build_snapshot(conn: &Connection, cache: &LayoutCache) -> Result<Snapshot, String> {
    let meta = load_item_meta(conn)?;
    let pinned = load_pinned(conn);
    let keep: HashSet<String> = meta.keys().cloned().collect();
    let mut positions = cache.positions_filtered(&keep);

    let placed_ids: HashSet<String> = positions.iter().map(|(id, _)| id.clone()).collect();
    let vectorized_without_position = meta
        .iter()
        .filter(|(id, m)| m.has_vector && !placed_ids.contains(*id))
        .count();
    let mut missing: Vec<String> = meta
        .keys()
        .filter(|id| !placed_ids.contains(*id))
        .cloned()
        .collect();
    missing.sort();
    let total = meta.len();
    positions.extend(
        missing
            .into_iter()
            .enumerate()
            .map(|(i, id)| (id.clone(), fallback_position(&id, i, total))),
    );

    let mut nodes = Vec::with_capacity(positions.len());
    let mut cluster_sum: HashMap<u32, (f64, f64, usize)> = HashMap::new();
    let mut node_cluster: HashMap<String, u32> = HashMap::new();

    for (id, pos) in &positions {
        let m = match meta.get(id) {
            Some(m) => m,
            None => continue,
        };
        let (x, y, is_pinned) = match pinned.get(id) {
            Some((px, py)) => (*px, *py, true),
            None => (pos.x, pos.y, false),
        };
        let e = cluster_sum.entry(pos.cluster_id).or_insert((0.0, 0.0, 0));
        e.0 += x as f64;
        e.1 += y as f64;
        e.2 += 1;
        node_cluster.insert(id.clone(), pos.cluster_id);
        nodes.push(SnapshotNode {
            id: id.clone(),
            x,
            y,
            kind: m.kind.clone(),
            importance: m.importance,
            cluster_id: pos.cluster_id,
            has_vector: m.has_vector,
            pinned: is_pinned,
        });
    }

    let mut clusters: Vec<ClusterAgg> = cluster_sum
        .into_iter()
        .map(|(id, (sx, sy, n))| ClusterAgg {
            id,
            x: (sx / n as f64) as f32,
            y: (sy / n as f64) as f32,
            size: n,
        })
        .collect();
    clusters.sort_by_key(|c| c.id);

    let cluster_edges = aggregate_cluster_edges(conn, &node_cluster)?;

    // The "X nodes · Y edges" header pairs raw nodes with raw edges, so count
    // actual active relations — not the inter-cluster aggregate (which is far
    // smaller and made the counter look broken).
    let edge_count: usize = conn
        .query_row(
            "SELECT COUNT(*) FROM relations WHERE valid_to IS NULL",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0) as usize;

    let stats = SnapshotStats {
        node_count: nodes.len(),
        cluster_count: clusters.len(),
        edge_count,
        vectorized_without_position,
    };

    Ok(Snapshot {
        layout_version: cache.layout_version(),
        computed_at: cache.computed_at(),
        seq: cache.current_seq(),
        nodes,
        clusters,
        cluster_edges,
        stats,
    })
}

/// Aggregate active relations into cluster→cluster weighted edges (undirected,
/// self-loops dropped). Each end mapped to its node's cluster.
fn aggregate_cluster_edges(
    conn: &Connection,
    node_cluster: &HashMap<String, u32>,
) -> Result<Vec<ClusterEdge>, String> {
    let mut stmt = conn
        .prepare("SELECT from_id, to_id FROM relations WHERE valid_to IS NULL")
        .map_err(|e| format!("prepare cluster edges: {e}"))?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(|e| format!("query cluster edges: {e}"))?;

    let mut weights: HashMap<(u32, u32), u64> = HashMap::new();
    for r in rows {
        let (from, to) = r.map_err(|e| e.to_string())?;
        let (Some(&cf), Some(&ct)) = (node_cluster.get(&from), node_cluster.get(&to)) else {
            continue;
        };
        if cf == ct {
            continue;
        }
        let key = if cf < ct { (cf, ct) } else { (ct, cf) };
        *weights.entry(key).or_insert(0) += 1;
    }
    let mut out: Vec<ClusterEdge> = weights
        .into_iter()
        .map(|((from, to), weight)| ClusterEdge { from, to, weight })
        .collect();
    out.sort_by(|a, b| (a.from, a.to).cmp(&(b.from, b.to)));
    Ok(out)
}

/// Compute the delta since a client's `since_seq` within the same layout
/// version. Removed ids = positions the client had that are no longer live;
/// caller passes none (we filter at read time so removed is derived from
/// stale-id filtering at the client). We return added/moved only here.
pub fn build_delta(
    conn: &Connection,
    cache: &LayoutCache,
    since_seq: u64,
) -> Result<DeltaResponse, String> {
    let version = cache.layout_version();
    let current_seq = cache.current_seq();
    if current_seq == since_seq {
        return Ok(DeltaResponse::Unchanged {
            layout_version: version,
            unchanged: true,
        });
    }
    let meta = load_item_meta(conn)?;
    let keep: HashSet<String> = meta.keys().cloned().collect();
    let moved = cache.positions_since(since_seq, &keep);
    let added_or_moved = moved
        .into_iter()
        .map(|(id, p): (String, Position)| DeltaNode {
            id,
            x: p.x,
            y: p.y,
            cluster_id: p.cluster_id,
        })
        .collect();
    Ok(DeltaResponse::Changed {
        layout_version: version,
        seq: current_seq,
        added_or_moved,
        removed: Vec::new(),
    })
}

/// Lazy labels for the viewport. Capped at MAX_IDS, read-only.
pub fn load_labels(conn: &Connection, ids: &[String]) -> Result<Vec<(String, String)>, String> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::with_capacity(ids.len());
    let mut stmt = conn
        .prepare("SELECT title FROM mem_items WHERE id = ?1")
        .map_err(|e| format!("prepare labels: {e}"))?;
    let mut ent_stmt = conn
        .prepare("SELECT properties_json FROM entities WHERE id = ?1")
        .map_err(|e| format!("prepare entity labels: {e}"))?;
    for id in ids.iter().take(MAX_IDS) {
        let title: Option<String> = stmt.query_row([id], |r| r.get::<_, String>(0)).ok();
        if let Some(t) = title {
            out.push((id.clone(), t));
            continue;
        }
        // Entities have no title column — derive a name from properties or id.
        let props: Option<String> = ent_stmt.query_row([id], |r| r.get::<_, String>(0)).ok();
        let label = props
            .as_deref()
            .and_then(|p| serde_json::from_str::<serde_json::Value>(p).ok())
            .and_then(|v| {
                v.get("name")
                    .or_else(|| v.get("title"))
                    .and_then(|n| n.as_str().map(String::from))
            })
            .unwrap_or_else(|| id.clone());
        if conn
            .query_row("SELECT 1 FROM entities WHERE id = ?1", [id], |_| Ok(()))
            .is_ok()
        {
            out.push((id.clone(), label));
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, Serialize)]
pub struct EdgeRow {
    pub from: String,
    pub to: String,
    pub rel: String,
    pub confidence: f64,
}

/// Active relations touching any of `ids`. Capped at MAX_IDS input ids.
pub fn load_edges(conn: &Connection, ids: &[String]) -> Result<Vec<EdgeRow>, String> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let id_set: HashSet<&String> = ids.iter().take(MAX_IDS).collect();
    let mut stmt = conn
        .prepare(
            "SELECT from_id, to_id, relation_type, confidence FROM relations WHERE valid_to IS NULL",
        )
        .map_err(|e| format!("prepare edges: {e}"))?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, f64>(3)?,
            ))
        })
        .map_err(|e| format!("query edges: {e}"))?;
    let mut out = Vec::new();
    let mut seen: HashSet<(String, String, String)> = HashSet::new();
    for r in rows {
        let (from, to, rel, confidence) = r.map_err(|e| e.to_string())?;
        if !id_set.contains(&from) && !id_set.contains(&to) {
            continue;
        }
        if seen.insert((from.clone(), to.clone(), rel.clone())) {
            out.push(EdgeRow {
                from,
                to,
                rel,
                confidence,
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::graph_layout::cache::Position;

    fn schema(conn: &Connection) {
        crate::mem::store::init_schema(conn).unwrap();
    }

    fn insert_item(conn: &Connection, id: &str, kind: &str, importance: f64) {
        conn.execute(
            "INSERT INTO mem_items (id, uid, kind, scope, title, tags_json, importance, body_inline, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'global', ?4, '[]', ?5, 'b', 't', 't')",
            rusqlite::params![id, ulid::Ulid::new().to_string(), kind, id, importance],
        )
        .unwrap();
    }

    fn cache_at(dir: &std::path::Path) -> LayoutCache {
        LayoutCache::load(dir.join(format!("snap_{}.json", ulid::Ulid::new())))
    }

    #[test]
    fn snapshot_excludes_stale_ids() {
        let conn = Connection::open_in_memory().unwrap();
        schema(&conn);
        insert_item(&conn, "fact_live", "fact", 3.0);
        let cache = cache_at(&std::env::temp_dir());
        let mut map = HashMap::new();
        map.insert(
            "fact_live".to_string(),
            Position {
                x: 1.0,
                y: 2.0,
                cluster_id: 0,
            },
        );
        map.insert(
            "fact_deleted".to_string(),
            Position {
                x: 9.0,
                y: 9.0,
                cluster_id: 0,
            },
        );
        cache.replace_all(map, "now".into());

        let snap = build_snapshot(&conn, &cache).unwrap();
        assert_eq!(snap.nodes.len(), 1);
        assert_eq!(snap.nodes[0].id, "fact_live");
    }

    #[test]
    fn snapshot_positions_live_nodes_when_cache_empty() {
        let conn = Connection::open_in_memory().unwrap();
        schema(&conn);
        insert_item(&conn, "fact_a", "fact", 3.0);
        insert_item(&conn, "rule_b", "rule", 5.0);
        let cache = cache_at(&std::env::temp_dir());

        let snap = build_snapshot(&conn, &cache).unwrap();
        assert_eq!(snap.nodes.len(), 2);
        assert_eq!(snap.stats.node_count, 2);
        assert!(snap
            .nodes
            .iter()
            .all(|n| n.x.is_finite() && n.y.is_finite()));
        assert_eq!(snap.clusters.iter().map(|c| c.size).sum::<usize>(), 2);
    }

    #[test]
    fn pinned_position_overrides_computed() {
        let conn = Connection::open_in_memory().unwrap();
        schema(&conn);
        insert_item(&conn, "fact_pin", "fact", 2.0);
        conn.execute(
            "INSERT INTO graph_pinned_positions (mem_id, x, y, updated_at) VALUES ('fact_pin', 42.0, 43.0, 1)",
            [],
        )
        .unwrap();
        let cache = cache_at(&std::env::temp_dir());
        let mut map = HashMap::new();
        map.insert(
            "fact_pin".to_string(),
            Position {
                x: 1.0,
                y: 2.0,
                cluster_id: 0,
            },
        );
        cache.replace_all(map, "now".into());

        let snap = build_snapshot(&conn, &cache).unwrap();
        assert_eq!(snap.nodes.len(), 1);
        assert!(snap.nodes[0].pinned);
        assert_eq!(snap.nodes[0].x, 42.0);
        assert_eq!(snap.nodes[0].y, 43.0);
    }

    #[test]
    fn delta_unchanged_when_seq_matches() {
        let conn = Connection::open_in_memory().unwrap();
        schema(&conn);
        let cache = cache_at(&std::env::temp_dir());
        let seq = cache.current_seq();
        let d = build_delta(&conn, &cache, seq).unwrap();
        assert!(matches!(
            d,
            DeltaResponse::Unchanged {
                unchanged: true,
                ..
            }
        ));
    }

    #[test]
    fn delta_returns_moved_after_upsert() {
        let conn = Connection::open_in_memory().unwrap();
        schema(&conn);
        insert_item(&conn, "fact_x", "fact", 1.0);
        let cache = cache_at(&std::env::temp_dir());
        let before = cache.current_seq();
        cache.upsert(
            "fact_x".to_string(),
            Position {
                x: 7.0,
                y: 8.0,
                cluster_id: 0,
            },
        );
        let d = build_delta(&conn, &cache, before).unwrap();
        match d {
            DeltaResponse::Changed { added_or_moved, .. } => {
                assert_eq!(added_or_moved.len(), 1);
                assert_eq!(added_or_moved[0].id, "fact_x");
            }
            _ => panic!("expected changed delta"),
        }
    }

    #[test]
    fn cluster_edges_aggregate_and_skip_self_loops() {
        let conn = Connection::open_in_memory().unwrap();
        schema(&conn);
        insert_item(&conn, "a", "fact", 1.0);
        insert_item(&conn, "b", "fact", 1.0);
        insert_item(&conn, "c", "fact", 1.0);
        conn.execute("INSERT INTO relations (from_id, relation_type, to_id, scope, valid_from, created_at) VALUES ('a','r','b','global','t','t')", []).unwrap();
        conn.execute("INSERT INTO relations (from_id, relation_type, to_id, scope, valid_from, created_at) VALUES ('a','r','c','global','t','t')", []).unwrap();

        let cache = cache_at(&std::env::temp_dir());
        let mut map = HashMap::new();
        map.insert(
            "a".to_string(),
            Position {
                x: 0.0,
                y: 0.0,
                cluster_id: 0,
            },
        );
        map.insert(
            "b".to_string(),
            Position {
                x: 0.0,
                y: 0.0,
                cluster_id: 1,
            },
        );
        map.insert(
            "c".to_string(),
            Position {
                x: 0.0,
                y: 0.0,
                cluster_id: 1,
            },
        );
        cache.replace_all(map, "now".into());

        let snap = build_snapshot(&conn, &cache).unwrap();
        // a(0)->b(1) and a(0)->c(1) both collapse to edge (0,1) weight 2.
        assert_eq!(snap.cluster_edges.len(), 1);
        assert_eq!(snap.cluster_edges[0].weight, 2);
    }
}
