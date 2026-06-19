use super::common::*;
use crate::mem::graph_repair::*;
use rusqlite::Connection;

#[test]
fn repair_orphan_vectors_finds_and_deletes() {
    let conn = fresh_conn();
    conn.execute_batch("CREATE TABLE mem_vectors (mem_id TEXT, embedding BLOB)")
        .unwrap();
    use crate::mem::graph::{insert_entity, Entity};
    insert_entity(
        &conn,
        &Entity {
            id: "person_real".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::json!({}),
        },
    )
    .unwrap();
    conn.execute(
        "INSERT INTO mem_vectors VALUES
            ('person_real', X''),
            ('person_real_chunk_1', X''),
            ('person_ghost', X''),
            ('person_ghost_chunk_2', X'')",
        [],
    )
    .unwrap();

    let dry = repair_orphan_vectors(&conn, true, "test").unwrap();
    assert!(dry.dry_run);
    assert_eq!(dry.deleted, 0);
    assert_eq!(dry.orphan_ids.len(), 2);
    assert!(dry.orphan_ids.contains(&"person_ghost".to_string()));

    let report = repair_orphan_vectors(&conn, false, "test").unwrap();
    assert_eq!(report.deleted, 2);
    assert_eq!(report.orphan_ids.len(), 2);

    let remaining: i64 = conn
        .query_row("SELECT COUNT(*) FROM mem_vectors", [], |r| r.get(0))
        .unwrap();
    assert_eq!(remaining, 2, "only person_real's vectors should remain");

    let (count, actor): (i64, String) = conn
        .query_row(
            "SELECT count, actor FROM graph_purge_log ORDER BY id DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(count, 2);
    assert_eq!(actor, "test");
}

#[test]
fn repair_orphan_vectors_no_orphans_is_noop() {
    let conn = fresh_conn();
    conn.execute_batch("CREATE TABLE mem_vectors (mem_id TEXT, embedding BLOB)")
        .unwrap();
    use crate::mem::graph::{insert_entity, Entity};
    insert_entity(
        &conn,
        &Entity {
            id: "person_real".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::json!({}),
        },
    )
    .unwrap();
    conn.execute("INSERT INTO mem_vectors VALUES ('person_real', X'')", [])
        .unwrap();

    let report = repair_orphan_vectors(&conn, false, "test").unwrap();
    assert_eq!(report.deleted, 0);
    assert!(report.orphan_ids.is_empty());

    let audits: i64 = conn
        .query_row("SELECT COUNT(*) FROM graph_purge_log", [], |r| r.get(0))
        .unwrap();
    assert_eq!(audits, 0, "no orphans → no audit row");
}

fn insert_item_inline(conn: &Connection, id: &str, body: &str) {
    conn.execute(
        "INSERT INTO mem_items (id, uid, kind, scope, title, body_inline, created_at, updated_at, accessed_at, importance)
         VALUES (?1, ?1, 'note', 'global', 'title', ?2, '2026-01-01', '2026-01-01', '2026-01-01', 0.0)",
        rusqlite::params![id, body],
    )
    .unwrap();
}

#[test]
fn prune_thin_vectors_deletes_only_thin_keeps_thick() {
    let conn = fresh_conn();
    conn.execute_batch("CREATE TABLE mem_vectors (mem_id TEXT, embedding BLOB)")
        .unwrap();

    // Thick: a real, substantive body — its vector is GOOD and must survive.
    insert_item_inline(
        &conn,
        "fact_thick",
        "A genuinely meaningful body with real content worth searching.",
    );
    // Thin: empty and title-only-ish bodies — their vectors are noise.
    insert_item_inline(&conn, "fact_empty", "");
    insert_item_inline(&conn, "fact_short", "hi");
    // Blob-backed: large body lives on disk (body_inline NULL) — never thin.
    conn.execute(
        "INSERT INTO mem_items (id, uid, kind, scope, title, body_blob_path, created_at, updated_at, accessed_at, importance)
         VALUES ('fact_blob','fact_blob','note','global','t','sha256_x.txt','2026-01-01','2026-01-01','2026-01-01',0.0)",
        [],
    ).unwrap();

    conn.execute(
        "INSERT INTO mem_vectors VALUES
            ('fact_thick', X''),
            ('fact_thick_chunk_1', X''),
            ('fact_empty', X''),
            ('fact_short', X''),
            ('fact_blob', X'')",
        [],
    )
    .unwrap();

    let dry = prune_thin_vectors(&conn, true, "dreamer").unwrap();
    assert!(dry.dry_run);
    assert_eq!(dry.deleted, 0);
    // Only the two thin items (+none of thick/blob) are flagged.
    assert_eq!(dry.orphan_ids.len(), 2, "got {:?}", dry.orphan_ids);
    assert!(dry.orphan_ids.contains(&"fact_empty".to_string()));
    assert!(dry.orphan_ids.contains(&"fact_short".to_string()));
    assert!(
        !dry.orphan_ids.contains(&"fact_thick".to_string()),
        "thick (good) vector must never be flagged"
    );
    assert!(!dry.orphan_ids.contains(&"fact_blob".to_string()));

    let report = prune_thin_vectors(&conn, false, "dreamer").unwrap();
    assert_eq!(report.deleted, 2);

    let surviving: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT mem_id FROM mem_vectors ORDER BY mem_id")
            .unwrap();
        let rows: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        rows
    };
    assert_eq!(
        surviving,
        vec![
            "fact_blob".to_string(),
            "fact_thick".to_string(),
            "fact_thick_chunk_1".to_string()
        ],
        "thick body, its chunk, and the blob-backed vector must all survive"
    );

    // Items themselves are never deleted — only the vector rows.
    let item_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM mem_items", [], |r| r.get(0))
        .unwrap();
    assert_eq!(item_count, 4, "no mem_item deleted by vector pruning");

    let (count, actor): (i64, String) = conn
        .query_row(
            "SELECT count, actor FROM graph_purge_log ORDER BY id DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(count, 2);
    assert_eq!(actor, "dreamer");
}

#[test]
fn prune_thin_vectors_noop_when_all_thick() {
    let conn = fresh_conn();
    conn.execute_batch("CREATE TABLE mem_vectors (mem_id TEXT, embedding BLOB)")
        .unwrap();
    insert_item_inline(
        &conn,
        "fact_a",
        "Plenty of real searchable content lives in this body.",
    );
    conn.execute("INSERT INTO mem_vectors VALUES ('fact_a', X'')", [])
        .unwrap();

    let report = prune_thin_vectors(&conn, false, "dreamer").unwrap();
    assert_eq!(report.deleted, 0);
    assert!(report.orphan_ids.is_empty());
    let remaining: i64 = conn
        .query_row("SELECT COUNT(*) FROM mem_vectors", [], |r| r.get(0))
        .unwrap();
    assert_eq!(remaining, 1, "good vector untouched");
}
#[test]
fn repair_orphan_blobs_finds_unreferenced_files() {
    let conn = fresh_conn();
    let tmp = tempfile::tempdir().unwrap();
    let blobs_dir = tmp.path();

    // Seed the dir with three CAS-named files and two random ones.
    std::fs::write(blobs_dir.join("sha256_referenced.txt"), "real").unwrap();
    std::fs::write(blobs_dir.join("sha256_orphan_a.txt"), "ghost1").unwrap();
    std::fs::write(blobs_dir.join("sha256_orphan_b.txt"), "ghost2").unwrap();
    std::fs::write(blobs_dir.join("other.txt"), "not a blob").unwrap();

    // Tell the DB about the referenced one.
    conn.execute(
        "INSERT INTO mem_items (id, uid, kind, scope, title, body_blob_path, created_at, updated_at, accessed_at, importance)
         VALUES ('m1','u1','note','global','t','sha256_referenced.txt','2026-01-01','2026-01-01','2026-01-01',0.0)",
        [],
    ).unwrap();

    let dry = repair_orphan_blobs(&conn, blobs_dir, true, "test").unwrap();
    assert!(dry.dry_run);
    assert_eq!(dry.deleted, 0);
    assert_eq!(dry.orphan_files.len(), 2);

    let report = repair_orphan_blobs(&conn, blobs_dir, false, "test").unwrap();
    assert_eq!(report.deleted, 2);
    assert!(
        blobs_dir.join("sha256_referenced.txt").exists(),
        "referenced blob stays"
    );
    assert!(
        !blobs_dir.join("sha256_orphan_a.txt").exists(),
        "orphan deleted"
    );
    assert!(
        !blobs_dir.join("sha256_orphan_b.txt").exists(),
        "orphan deleted"
    );
    assert!(
        blobs_dir.join("other.txt").exists(),
        "non-CAS file untouched"
    );
}
