use rusqlite::{params, Connection};
use serde_json::json;

use super::graph_candidates;
use crate::graph::{insert_relation, invalidate_relation, Relation};
use crate::store::{init_schema, insert, MemKind, MemScope, NewMemItem};
use crate::{MemManager, SearchQuery};

const LINK: &str = "mentions";

fn fresh() -> (Connection, std::path::PathBuf) {
    let conn = Connection::open_in_memory().unwrap();
    init_schema(&conn).unwrap();
    let blobs = std::env::temp_dir().join(format!("mem-graph-source-{}", ulid::Ulid::new()));
    std::fs::create_dir_all(&blobs).unwrap();
    (conn, blobs)
}

fn item(title: &str, importance: f64) -> NewMemItem {
    NewMemItem {
        kind: MemKind::Fact,
        scope: MemScope::Global,
        title: title.into(),
        summary: None,
        area: None,
        tags: Vec::new(),
        body: format!("{title} body"),
        source: None,
        importance,
        source_path: None,
        pinned: false,
    }
}

/// Raw insert: `graph::insert_entity` also embeds, which would spawn the embedder.
fn entity(conn: &Connection, id: &str) {
    conn.execute(
        "INSERT INTO entities (id, entity_type, scope, properties_json, created_at, updated_at)
         VALUES (?1, 'Tech', 'global', '{}', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        params![id],
    )
    .unwrap();
}

fn link(conn: &Connection, from: &str, to: &str) {
    insert_relation(
        conn,
        &Relation {
            from_id: from.into(),
            relation_type: LINK.into(),
            to_id: to.into(),
            scope: "global".into(),
            valid_from: "2026-01-01T00:00:00Z".into(),
            valid_to: None,
            confidence: 1.0,
            properties: json!({}),
            reason: None,
        },
    )
    .unwrap();
}

fn query(text: &str) -> SearchQuery {
    SearchQuery {
        query: text.into(),
        kind: None,
        scope: None,
        area: None,
        limit: None,
    }
}

#[test]
fn graph_source_returns_linked_mem_items_ranked() {
    let (conn, blobs) = fresh();
    for id in ["tech_quasar", "person_nebula", "tech_unrelated"] {
        entity(&conn, id);
    }
    let both = insert(&conn, &item("both linked", 1.0), &blobs).unwrap();
    let heavy = insert(&conn, &item("heavy single", 5.0), &blobs).unwrap();
    let light = insert(&conn, &item("light single", 2.0), &blobs).unwrap();
    let off_topic = insert(&conn, &item("off topic", 9.0), &blobs).unwrap();
    link(&conn, &both, "tech_quasar");
    link(&conn, "person_nebula", &both);
    link(&conn, "tech_quasar", &heavy);
    link(&conn, &light, "person_nebula");
    link(&conn, &off_topic, "tech_unrelated");

    let q = query("Quasar and NEBULA");
    assert_eq!(
        graph_candidates(&conn, &q, 10).unwrap(),
        vec![both.clone(), heavy.clone(), light]
    );
    assert_eq!(graph_candidates(&conn, &q, 2).unwrap(), vec![both, heavy]);
}

#[test]
fn graph_source_excludes_entities_archived_and_invalidated() {
    let (conn, blobs) = fresh();
    for id in ["tech_quasar", "tech_neighbor"] {
        entity(&conn, id);
    }
    let live = insert(&conn, &item("live linked", 3.0), &blobs).unwrap();
    let archived = insert(&conn, &item("archived linked", 3.0), &blobs).unwrap();
    let stale = insert(&conn, &item("stale linked", 3.0), &blobs).unwrap();
    link(&conn, &live, "tech_quasar");
    link(&conn, "tech_quasar", "tech_neighbor");
    link(&conn, &archived, "tech_quasar");
    link(&conn, &stale, "tech_quasar");
    conn.execute(
        "UPDATE mem_items SET kind = 'archived' WHERE id = ?1",
        params![archived],
    )
    .unwrap();
    assert!(invalidate_relation(&conn, &stale, LINK, "tech_quasar", None).unwrap());

    assert_eq!(
        graph_candidates(&conn, &query("quasar"), 10).unwrap(),
        vec![live]
    );
    let mut other_kind = query("quasar");
    other_kind.kind = Some("preference".into());
    assert!(graph_candidates(&conn, &other_kind, 10).unwrap().is_empty());
}

#[test]
fn graph_source_empty_keywords_or_zero_m_is_empty() {
    // No schema: any SQL the source ran would fail instead of returning empty.
    let bare = Connection::open_in_memory().unwrap();
    assert!(graph_candidates(&bare, &query("is it to be"), 5)
        .unwrap()
        .is_empty());
    assert!(graph_candidates(&bare, &query("quasar"), 0)
        .unwrap()
        .is_empty());
    assert!(graph_candidates(&bare, &query("quasar"), 1).is_err());
}

#[test]
fn graph_source_hydration_records_no_access() {
    let mem = MemManager::open_in_memory().unwrap();
    let first = mem.insert_raw(&item("first linked", 4.0)).unwrap();
    let second = mem.insert_raw(&item("second linked", 2.0)).unwrap();
    mem.with_conn(|conn| {
        entity(conn, "tech_quasar");
        link(conn, &first, "tech_quasar");
        link(conn, &second, "tech_quasar");
    });
    let counts = || {
        [&first, &second]
            .map(|id| mem.access_count(id).unwrap().unwrap())
            .to_vec()
    };
    let before = counts();

    let hits = mem.graph_candidates_untouched(&query("quasar"), 5).unwrap();

    let ids: Vec<&str> = hits.iter().map(|hit| hit.item.id.as_str()).collect();
    assert_eq!(ids, vec![first.as_str(), second.as_str()]);
    assert!(hits.iter().all(|hit| hit.score == 0.0));
    assert_eq!(counts(), before);
}
