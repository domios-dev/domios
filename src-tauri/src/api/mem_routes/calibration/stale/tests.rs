use chrono::Utc;
use rusqlite::{params, Connection};

use super::*;

const OLD: &str = "datetime('now','-400 days')";
const RECENT: &str = "datetime('now','-10 days')";

fn open_test_conn() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    crate::mem::store::init_schema(&conn).unwrap();
    conn
}

struct Item<'a> {
    id: &'a str,
    kind: &'a str,
    importance: f64,
    pinned: i64,
    created: &'a str,
    updated: &'a str,
    accessed: &'a str,
}

impl<'a> Item<'a> {
    fn old(id: &'a str) -> Self {
        Self {
            id,
            kind: "fact",
            importance: 2.0,
            pinned: 0,
            created: OLD,
            updated: OLD,
            accessed: "NULL",
        }
    }
}

fn insert(conn: &Connection, item: Item) {
    let sql = format!(
        "INSERT INTO mem_items (id, uid, kind, scope, title, importance, created_at,
            updated_at, accessed_at, tags_json, is_pinned_wakeup)
         VALUES (?1, ?1, ?2, 'global', ?1, ?3, {}, {}, {}, '[]', ?4)",
        item.created, item.updated, item.accessed
    );
    conn.execute(
        &sql,
        params![item.id, item.kind, item.importance, item.pinned],
    )
    .unwrap();
}

fn relate(conn: &Connection, from: &str, to: &str, valid_from: &str, valid_to: &str) {
    let sql = format!(
        "INSERT INTO relations (from_id, relation_type, to_id, scope, valid_from, valid_to, created_at)
         VALUES (?1, 'applies_to', ?2, 'global', {valid_from}, {valid_to}, {valid_from})"
    );
    conn.execute(&sql, params![from, to]).unwrap();
}

fn stale(conn: &Connection, id: &str) -> bool {
    is_stale(conn, id, Utc::now()).unwrap()
}

fn kind_of(conn: &Connection, id: &str) -> String {
    conn.query_row(
        "SELECT kind FROM mem_items WHERE id = ?1",
        params![id],
        |r| r.get(0),
    )
    .unwrap()
}

fn history_rows(conn: &Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM mem_item_history", [], |r| r.get(0))
        .unwrap()
}

fn enabled(cap: u32) -> DreamHygieneConfig {
    DreamHygieneConfig {
        stale_archive: true,
        stale_archive_cap: cap,
        ..DreamHygieneConfig::default()
    }
}

#[test]
fn stale_predicate_requires_age_and_no_direct_touch() {
    let conn = open_test_conn();
    insert(&conn, Item::old("untouched"));
    insert(
        &conn,
        Item {
            created: "datetime('now','-300 days')",
            updated: "datetime('now','-300 days')",
            ..Item::old("young")
        },
    );
    insert(
        &conn,
        Item {
            accessed: RECENT,
            ..Item::old("accessed")
        },
    );
    insert(
        &conn,
        Item {
            updated: RECENT,
            ..Item::old("updated")
        },
    );
    insert(&conn, Item::old("counted"));
    conn.execute(
        "INSERT INTO mem_item_history (item_id, changed_at, changed_by, change_type)
         VALUES ('counted', ?1, 'user', 'update')",
        params![(Utc::now() - chrono::Duration::days(5)).to_rfc3339()],
    )
    .unwrap();

    assert!(stale(&conn, "untouched"));
    assert!(!stale(&conn, "young"), "younger than 365 days");
    assert!(!stale(&conn, "accessed"), "accessed_at inside the year");
    assert!(!stale(&conn, "updated"), "updated_at inside the year");
    assert!(!stale(&conn, "counted"), "history row inside the year");
}

#[test]
fn stale_predicate_rejects_recent_relation_change() {
    let conn = open_test_conn();
    insert(&conn, Item::old("anchor_old"));
    for id in ["old_edge", "new_edge", "closed_edge", "incoming_edge"] {
        insert(&conn, Item::old(id));
    }
    relate(&conn, "old_edge", "anchor_old", OLD, "NULL");
    relate(&conn, "new_edge", "anchor_old", RECENT, "NULL");
    relate(&conn, "closed_edge", "anchor_old", OLD, RECENT);
    relate(&conn, "anchor_old", "incoming_edge", RECENT, "NULL");

    assert!(stale(&conn, "old_edge"), "a year-old relation is no touch");
    assert!(
        !stale(&conn, "new_edge"),
        "relation created inside the year"
    );
    assert!(
        !stale(&conn, "closed_edge"),
        "relation invalidated inside the year"
    );
    assert!(
        !stale(&conn, "incoming_edge"),
        "incoming relation created inside the year"
    );
}

#[test]
fn stale_predicate_rejects_touched_neighbour() {
    let conn = open_test_conn();
    insert(
        &conn,
        Item {
            accessed: RECENT,
            ..Item::old("hot_item")
        },
    );
    conn.execute(
        &format!(
            "INSERT INTO entities (id, entity_type, scope, properties_json, created_at, updated_at)
             VALUES ('topic_hot', 'Topic', 'global', '{{}}', {OLD}, {RECENT}),
                    ('topic_cold', 'Topic', 'global', '{{}}', {OLD}, {OLD})"
        ),
        [],
    )
    .unwrap();
    for id in [
        "out_to_item",
        "in_from_item",
        "to_entity",
        "dead_link",
        "cold_link",
    ] {
        insert(&conn, Item::old(id));
    }
    relate(&conn, "out_to_item", "hot_item", OLD, "NULL");
    relate(&conn, "hot_item", "in_from_item", OLD, "NULL");
    relate(&conn, "to_entity", "topic_hot", OLD, "NULL");
    relate(&conn, "dead_link", "hot_item", OLD, OLD);
    relate(&conn, "cold_link", "topic_cold", OLD, "NULL");

    assert!(!stale(&conn, "out_to_item"), "outgoing neighbour touched");
    assert!(!stale(&conn, "in_from_item"), "incoming neighbour touched");
    assert!(!stale(&conn, "to_entity"), "entity neighbour updated");
    assert!(
        stale(&conn, "dead_link"),
        "an invalidated edge is not a neighbour"
    );
    assert!(stale(&conn, "cold_link"), "untouched neighbour");
}

#[test]
fn stale_predicate_excludes_pinned_skill_rule_preference_archived_and_high_importance() {
    let conn = open_test_conn();
    insert(&conn, Item::old("control"));
    insert(
        &conn,
        Item {
            pinned: 1,
            ..Item::old("pinned")
        },
    );
    for kind in ["skill", "rule", "preference", "archived"] {
        insert(
            &conn,
            Item {
                kind,
                ..Item::old(kind)
            },
        );
    }
    insert(
        &conn,
        Item {
            importance: 4.0,
            ..Item::old("important")
        },
    );
    insert(
        &conn,
        Item {
            importance: 3.9,
            ..Item::old("just_below")
        },
    );

    assert!(stale(&conn, "control"));
    assert!(stale(&conn, "just_below"));
    for id in [
        "pinned",
        "skill",
        "rule",
        "preference",
        "archived",
        "important",
    ] {
        assert!(!stale(&conn, id), "{id} must be excluded");
    }
}

#[test]
fn stale_archive_respects_nightly_cap() {
    let conn = open_test_conn();
    for i in 0..25 {
        insert(&conn, Item::old(&format!("stale_{i:02}")));
    }
    let items_before: i64 = conn
        .query_row("SELECT COUNT(*) FROM mem_items", [], |r| r.get(0))
        .unwrap();

    let first = run_stale_archive(&conn, &enabled(20), Utc::now(), StaleMode::Apply, None).unwrap();
    assert_eq!(first["archived"].as_array().unwrap().len(), 20);
    assert_eq!(first["matching"], 25);

    let second =
        run_stale_archive(&conn, &enabled(20), Utc::now(), StaleMode::Apply, None).unwrap();
    assert_eq!(second["skipped_reason"], "nightly_cap");
    assert!(second["archived"].as_array().unwrap().is_empty());

    let archived: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM mem_items WHERE kind = 'archived'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(archived, 20);
    let audited: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM mem_item_history
             WHERE changed_by = ?1 AND change_type = 'archive' AND old_kind = 'fact'
               AND new_kind = 'archived' AND reason IS NOT NULL",
            params![STALE_ACTOR],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(audited, 20);
    let items_after: i64 = conn
        .query_row("SELECT COUNT(*) FROM mem_items", [], |r| r.get(0))
        .unwrap();
    assert_eq!(items_after, items_before, "archive never deletes a row");
}

#[test]
fn stale_archive_disabled_by_default() {
    let conn = open_test_conn();
    // Importance 3 keeps the Sunday weekly archive rule out of this test.
    insert(
        &conn,
        Item {
            importance: 3.0,
            ..Item::old("would_be_stale")
        },
    );
    let cfg = DreamHygieneConfig::default();
    assert!(!cfg.stale_archive);

    let report = run_stale_archive(&conn, &cfg, Utc::now(), StaleMode::Apply, None).unwrap();
    assert_eq!(report["skipped_reason"], "disabled");
    assert_eq!(kind_of(&conn, "would_be_stale"), "fact");
    assert_eq!(history_rows(&conn), 0);

    let nightly = super::super::run_nightly_calibration(&conn, &cfg, Utc::now()).unwrap();
    assert_eq!(nightly["stale"]["skipped_reason"], "disabled");
    assert_eq!(kind_of(&conn, "would_be_stale"), "fact");
}

#[test]
fn stale_dry_run_changes_nothing() {
    let conn = open_test_conn();
    insert(&conn, Item::old("stale_a"));
    insert(&conn, Item::old("stale_b"));
    insert(
        &conn,
        Item {
            accessed: RECENT,
            ..Item::old("fresh")
        },
    );

    let report = run_stale_archive(
        &conn,
        &DreamHygieneConfig::default(),
        Utc::now(),
        StaleMode::DryRun,
        None,
    )
    .unwrap();
    let selected = report["selected"].as_array().unwrap();
    let ids: Vec<&str> = selected.iter().map(|s| s["id"].as_str().unwrap()).collect();
    assert_eq!(ids, vec!["stale_a", "stale_b"]);
    assert!(selected
        .iter()
        .all(|s| !s["reason"].as_str().unwrap().is_empty()));
    assert_eq!(report["matching"], 2);
    assert_eq!(kind_of(&conn, "stale_a"), "fact");
    assert_eq!(history_rows(&conn), 0);
}

#[test]
fn stale_archive_rows_do_not_trip_weekly_reentry_guard() {
    let conn = open_test_conn();
    insert(&conn, Item::old("stale_guard"));
    run_stale_archive(&conn, &enabled(5), Utc::now(), StaleMode::Apply, None).unwrap();
    let weekly = super::super::run_calibration_sweep(&conn, true).unwrap();
    assert_ne!(weekly["skipped_reason"], "recent_run");
}
