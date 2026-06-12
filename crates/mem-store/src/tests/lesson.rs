use rusqlite::Connection;

use super::init_v1_22_schema;
use crate::lesson::{promote_high_confidence_lessons, DEFAULT_PROMOTION_THRESHOLD};

fn fresh_conn() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    init_v1_22_schema(&conn);
    crate::migrations::run(&conn).unwrap();
    conn
}

fn insert_item(
    conn: &Connection,
    id: &str,
    kind: &str,
    tags_json: &str,
    evidence: i64,
    false_fires: i64,
    promotion_threshold: Option<i64>,
) {
    conn.execute(
        "INSERT INTO mem_items (id, uid, kind, scope, title, tags_json, created_at, updated_at)
         VALUES (?1, ?1, ?2, 'global', 't', ?3, '2026-01-01', '2026-01-01')",
        rusqlite::params![id, kind, tags_json],
    )
    .unwrap();
    conn.execute(
        "UPDATE mem_items SET evidence_count = ?1, false_fire_count = ?2, promotion_threshold = ?3 WHERE id = ?4",
        rusqlite::params![evidence, false_fires, promotion_threshold, id],
    ).unwrap();
}

fn tags_for(conn: &Connection, id: &str) -> String {
    conn.query_row(
        "SELECT tags_json FROM mem_items WHERE id = ?1",
        rusqlite::params![id],
        |r| r.get::<_, String>(0),
    )
    .unwrap()
}

fn audit_count_for(conn: &Connection, lesson_uid: &str) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM lesson_audit WHERE lesson_uid = ?1",
        rusqlite::params![lesson_uid],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn migration_adds_promotion_threshold_and_lesson_audit() {
    let conn = fresh_conn();
    let cols: Vec<String> = conn
        .prepare("PRAGMA table_info(mem_items)")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(1))
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    assert!(
        cols.contains(&"promotion_threshold".to_string()),
        "expected `promotion_threshold` column; got {:?}",
        cols,
    );
    let table_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='lesson_audit'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(table_count, 1, "lesson_audit table must exist");
    let idx_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_lesson_audit_uid'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(idx_count, 1, "idx_lesson_audit_uid index must exist");
}

#[test]
fn promote_high_confidence_lessons_marks_eligible() {
    let conn = fresh_conn();
    insert_item(&conn, "lesson_eligible", "lesson", "[]", 12, 0, None);

    let report =
        promote_high_confidence_lessons(&conn, DEFAULT_PROMOTION_THRESHOLD, "machine-test")
            .unwrap();
    assert_eq!(report.promoted, 1);
    assert_eq!(report.already_high_confidence, 0);

    let tags = tags_for(&conn, "lesson_eligible");
    assert!(
        tags.contains("high-confidence"),
        "expected high-confidence tag; got {}",
        tags
    );

    assert_eq!(
        audit_count_for(&conn, "lesson_eligible"),
        1,
        "exactly one audit row"
    );
    let (action, reason, machine): (String, Option<String>, String) = conn.query_row(
        "SELECT action, reason, machine_id FROM lesson_audit WHERE lesson_uid = 'lesson_eligible'",
        [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    ).unwrap();
    assert_eq!(action, "promoted");
    assert!(reason.unwrap_or_default().contains("evidence_count"));
    assert_eq!(machine, "machine-test");
}

#[test]
fn promote_high_confidence_lessons_respects_per_item_override() {
    let conn = fresh_conn();
    insert_item(&conn, "lesson_strict", "lesson", "[]", 15, 0, Some(20));

    let report =
        promote_high_confidence_lessons(&conn, DEFAULT_PROMOTION_THRESHOLD, "machine-test")
            .unwrap();
    assert_eq!(
        report.promoted, 0,
        "15 < per-item override 20 → not promoted"
    );
    let tags = tags_for(&conn, "lesson_strict");
    assert!(!tags.contains("high-confidence"));
    assert_eq!(audit_count_for(&conn, "lesson_strict"), 0);
}

#[test]
fn promote_high_confidence_lessons_skips_already_high_confidence() {
    let conn = fresh_conn();
    insert_item(&conn, "lesson_first", "lesson", "[]", 12, 0, None);

    let r1 = promote_high_confidence_lessons(&conn, DEFAULT_PROMOTION_THRESHOLD, "machine-test")
        .unwrap();
    assert_eq!(r1.promoted, 1);
    assert_eq!(r1.already_high_confidence, 0);

    let r2 = promote_high_confidence_lessons(&conn, DEFAULT_PROMOTION_THRESHOLD, "machine-test")
        .unwrap();
    assert_eq!(r2.promoted, 0, "second pass: already tagged");
    assert_eq!(r2.already_high_confidence, 1);

    assert_eq!(
        audit_count_for(&conn, "lesson_first"),
        1,
        "no second audit row"
    );
}

#[test]
fn promote_high_confidence_lessons_ignores_false_fires() {
    let conn = fresh_conn();
    insert_item(&conn, "lesson_burned", "lesson", "[]", 20, 1, None);

    let report =
        promote_high_confidence_lessons(&conn, DEFAULT_PROMOTION_THRESHOLD, "machine-test")
            .unwrap();
    assert_eq!(report.promoted, 0, "false_fire_count > 0 disqualifies");
    let tags = tags_for(&conn, "lesson_burned");
    assert!(!tags.contains("high-confidence"));
}

#[test]
fn promote_high_confidence_lessons_ignores_non_lessons() {
    let conn = fresh_conn();
    insert_item(&conn, "skill_x", "skill", "[]", 50, 0, None);
    insert_item(&conn, "note_x", "note", "[]", 50, 0, None);

    let report =
        promote_high_confidence_lessons(&conn, DEFAULT_PROMOTION_THRESHOLD, "machine-test")
            .unwrap();
    assert_eq!(report.promoted, 0, "only kind='lesson' is in scope");
    let tags_skill = tags_for(&conn, "skill_x");
    let tags_note = tags_for(&conn, "note_x");
    assert!(!tags_skill.contains("high-confidence"));
    assert!(!tags_note.contains("high-confidence"));
}
