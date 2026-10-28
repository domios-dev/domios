use super::*;
use crate::graph_layout::cache::Position;

fn schema(conn: &Connection) {
    crate::store::init_schema(conn).unwrap();
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
