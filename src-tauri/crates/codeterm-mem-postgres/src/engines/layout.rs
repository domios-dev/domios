use std::collections::{HashMap, HashSet};

use super::*;
use codeterm_mem::graph_layout::snapshot::*;

use codeterm_mem::graph_layout::cache::Position;
use codeterm_mem::graph_layout::placement::{strip_chunk_suffix, COORD_RANGE};

pub const MAX_IDS: usize = 500;

#[derive(Clone)]
struct ItemMeta {
    kind: String,
    importance: f64,
    has_vector: bool,
}

fn load_item_meta(conn: &mut Transaction<'_>) -> Result<HashMap<String, ItemMeta>, String> {
    let vector_ids: HashSet<String> = if vectors::stamp_in(conn)?.is_some() {
        conn.query("SELECT mem_id FROM mem_vectors", &[])
            .map_err(db_error)?
            .into_iter()
            .map(|r| strip_chunk_suffix(&r.get::<_, String>(0)).to_owned())
            .collect()
    } else {
        HashSet::new()
    };
    let mut meta = HashMap::new();
    for r in conn.query("SELECT id,kind,importance FROM mem_items WHERE kind<>'archived' UNION ALL SELECT id,'entity',0.0::double precision FROM entities", &[]).map_err(db_error)? {
        let id:String=r.get(0); let has_vector=vector_ids.contains(&id);
        meta.entry(id).or_insert(ItemMeta { kind:r.get(1), importance:r.get(2),has_vector });
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

fn load_pinned(conn: &mut Transaction<'_>) -> HashMap<String, (f32, f32)> {
    conn.query("SELECT mem_id,x,y FROM graph_pinned_positions", &[])
        .map(|rows| {
            rows.into_iter()
                .map(|r| {
                    (
                        r.get(0),
                        (r.get::<_, f64>(1) as f32, r.get::<_, f64>(2) as f32),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn snapshot(conn: &mut Transaction<'_>, cache: &LayoutCache) -> Result<Snapshot, String> {
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

    let edge_count = conn
        .query_one("SELECT count(*) FROM relations WHERE valid_to IS NULL", &[])
        .map_err(db_error)?
        .get::<_, i64>(0) as usize;

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

fn aggregate_cluster_edges(
    conn: &mut Transaction<'_>,
    node_cluster: &HashMap<String, u32>,
) -> Result<Vec<ClusterEdge>, String> {
    let rows = conn
        .query(
            "SELECT from_id,to_id FROM relations WHERE valid_to IS NULL",
            &[],
        )
        .map_err(db_error)?;
    let mut weights: HashMap<(u32, u32), u64> = HashMap::new();
    for r in rows {
        let (from, to): (String, String) = (r.get(0), r.get(1));
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

pub fn delta(
    conn: &mut Transaction<'_>,
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

pub fn labels(conn: &mut Transaction<'_>, ids: &[String]) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    for id in ids.iter().take(MAX_IDS) {
        if let Some(r) = conn
            .query_opt("SELECT title FROM mem_items WHERE id=$1", &[&id])
            .map_err(db_error)?
        {
            out.push((id.clone(), r.get(0)));
            continue;
        }
        if let Some(r) = conn
            .query_opt("SELECT properties_json FROM entities WHERE id=$1", &[&id])
            .map_err(db_error)?
        {
            let label = serde_json::from_str::<serde_json::Value>(&r.get::<_, String>(0))
                .ok()
                .and_then(|v| {
                    v.get("name")
                        .or_else(|| v.get("title"))
                        .and_then(|v| v.as_str().map(str::to_owned))
                })
                .unwrap_or_else(|| id.clone());
            out.push((id.clone(), label));
        }
    }
    Ok(out)
}

pub fn edges(conn: &mut Transaction<'_>, ids: &[String]) -> Result<Vec<EdgeRow>, String> {
    let ids: Vec<_> = ids.iter().take(MAX_IDS).cloned().collect();
    let mut seen = HashSet::new();
    Ok(conn.query("SELECT from_id,to_id,relation_type,confidence FROM relations WHERE valid_to IS NULL AND (from_id=ANY($1) OR to_id=ANY($1))", &[&ids]).map_err(db_error)?.into_iter().filter_map(|r| {
        let (from,to,rel):(String,String,String)=(r.get(0),r.get(1),r.get(2));
        seen.insert((from.clone(),to.clone(),rel.clone())).then(||EdgeRow {from,to,rel,confidence:r.get(3)})
    }).collect())
}

pub(super) fn vectors(tx: &mut Transaction<'_>) -> Result<Vec<(String, Vec<f32>)>, String> {
    if crate::vectors::stamp_in(tx)?.is_none() {
        return Ok(Vec::new());
    }
    Ok(tx
        .query(
            "SELECT mem_id,embedding FROM mem_vectors ORDER BY mem_id",
            &[],
        )
        .map_err(db_error)?
        .into_iter()
        .filter_map(|r| {
            let id: String = r.get(0);
            (!id.contains("_chunk_")).then(|| (id, r.get::<_, pgvector::Vector>(1).to_vec()))
        })
        .collect())
}
pub(super) fn stored_vector(
    tx: &mut Transaction<'_>,
    id: &str,
) -> Result<Option<Vec<f32>>, String> {
    if crate::vectors::stamp_in(tx)?.is_none() {
        return Ok(None);
    }
    Ok(tx
        .query_opt("SELECT embedding FROM mem_vectors WHERE mem_id=$1", &[&id])
        .map_err(db_error)?
        .map(|r| r.get::<_, pgvector::Vector>(0).to_vec()))
}
pub(super) fn run_full(store: &PostgresMemStore, cache: &LayoutCache) -> Result<u64, String> {
    let (vectors, adjacency, entities) = store.transaction(|tx| {
        let mut adjacency = HashMap::<String, Vec<String>>::new();
        for r in tx
            .query(
                "SELECT from_id,to_id FROM relations WHERE valid_to IS NULL",
                &[],
            )
            .map_err(db_error)?
        {
            let (from, to): (String, String) = (r.get(0), r.get(1));
            adjacency.entry(from.clone()).or_default().push(to.clone());
            adjacency.entry(to).or_default().push(from);
        }
        let entities = tx
            .query("SELECT id FROM entities ORDER BY id", &[])
            .map_err(db_error)?
            .into_iter()
            .map(|r| r.get(0))
            .collect();
        Ok((vectors(tx)?, adjacency, entities))
    })?;
    let positions =
        codeterm_mem::graph_layout::project::project_positions(&vectors, &adjacency, entities)?;
    let version = cache.replace_all(positions, crate::items::now());
    cache.flush_now();
    Ok(version)
}
pub(super) fn place(
    store: &PostgresMemStore,
    cache: &LayoutCache,
    id: &str,
    vector: &[f32],
) -> Result<bool, String> {
    use codeterm_mem::graph_layout::placement::weighted_average;
    let neighbors=store.transaction(|tx| {
        let Some(identity)=crate::vectors::stamp_in(tx)? else { return Ok(Vec::new()); };
        codeterm_mem::embedder::provider::validate_vector(vector,identity.dimension).map_err(|e|e.to_string())?;
        let mut best=HashMap::<String,f64>::new();
        for row in tx.query("SELECT mem_id,embedding <-> $1 AS distance FROM mem_vectors ORDER BY distance LIMIT 32", &[&pgvector::Vector::from(vector.to_owned())]).map_err(db_error)? {
            let raw:String=row.get(0);let other=strip_chunk_suffix(&raw);
            if other!=id {let distance:f64=row.get(1);best.entry(other.to_owned()).and_modify(|v|*v=v.min(distance)).or_insert(distance);}
        }
        let mut pairs:Vec<_>=best.into_iter().collect();pairs.sort_by(|a,b|a.1.total_cmp(&b.1));pairs.truncate(8);
        Ok(pairs.into_iter().filter_map(|(other,d)|cache.get(&other).map(|p|(p,d))).collect::<Vec<_>>())
    })?;
    match weighted_average(&neighbors) {
        Some(pos) => {
            cache.upsert(id.to_owned(), pos);
            Ok(true)
        }
        None => Ok(false),
    }
}
