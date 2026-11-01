use super::*;
use rusqlite::Connection;

fn open_test_conn() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    crate::store::init_schema(&conn).unwrap();
    conn
}

fn insert_skill(conn: &Connection, id: &str, body: &str) {
    conn.execute(
            "INSERT INTO mem_items (id, uid, kind, scope, title, body_inline, tags_json, created_at, updated_at)
             VALUES (?1, ?1, 'skill', 'global', 't', ?2, '[]', '2026-01-01', '2026-01-01')",
            rusqlite::params![id, body],
        ).unwrap();
}

fn evidence_count(conn: &Connection, id: &str) -> i64 {
    conn.query_row(
        "SELECT evidence_count FROM mem_items WHERE id = ?1",
        rusqlite::params![id],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn observer_bumps_on_success_shaped_observation() {
    let conn = open_test_conn();
    insert_skill(&conn, "skill_a", "follow these steps to refresh tokens");
    let obs = EvolutionObserver::new();
    obs.record_injection(
        "tab1",
        vec![InjectedItem::new(
            "skill_a".into(),
            MemKind::Skill,
            "follow these steps to refresh tokens",
        )],
    );
    let bumped = obs.observe_fixture(
        &conn,
        "tab1",
        "Done. Token refreshed and request returned 200 OK.",
    );
    assert_eq!(bumped, vec!["skill_a".to_string()]);
    assert_eq!(evidence_count(&conn, "skill_a"), 2);
    assert!(
        obs.pending("tab1").is_empty(),
        "scope cleared after observe"
    );
}

#[test]
fn observer_skips_when_observation_carries_error_marker() {
    let conn = open_test_conn();
    insert_skill(&conn, "skill_b", "follow these steps");
    let obs = EvolutionObserver::new();
    obs.record_injection(
        "tab1",
        vec![InjectedItem::new(
            "skill_b".into(),
            MemKind::Skill,
            "follow these steps",
        )],
    );
    let bumped = obs.observe_fixture(&conn, "tab1", "Error: command failed with exit code 1");
    assert!(bumped.is_empty());
    assert_eq!(
        evidence_count(&conn, "skill_b"),
        1,
        "no bump on error-shaped observation"
    );
}

#[test]
fn observer_skips_when_observation_echoes_skill_body() {
    let conn = open_test_conn();
    let body = "When refreshing JWT tokens always include the kid header read from the JWKS endpoint, otherwise Auth0 returns 401 with realm=api.example.com";
    insert_skill(&conn, "skill_c", body);
    let obs = EvolutionObserver::new();
    obs.record_injection(
        "tab1",
        vec![InjectedItem::new("skill_c".into(), MemKind::Skill, body)],
    );
    let echo = format!(
        "Working on it. Following the rule: {}. Should succeed shortly.",
        body
    );
    let bumped = obs.observe_fixture(&conn, "tab1", &echo);
    assert!(
        bumped.is_empty(),
        "echo of body must not count as independent evidence"
    );
    assert_eq!(evidence_count(&conn, "skill_c"), 1);
}

#[test]
fn observer_no_op_when_nothing_tracked() {
    let conn = open_test_conn();
    let obs = EvolutionObserver::new();
    let bumped = obs.observe_fixture(&conn, "tab1", "ok");
    assert!(bumped.is_empty());
}

#[test]
fn observer_replaces_prior_batch_for_same_scope() {
    let conn = open_test_conn();
    insert_skill(&conn, "skill_x", "x body");
    insert_skill(&conn, "skill_y", "y body");
    let obs = EvolutionObserver::new();
    obs.record_injection(
        "tab1",
        vec![InjectedItem::new(
            "skill_x".into(),
            MemKind::Skill,
            "x body",
        )],
    );
    obs.record_injection(
        "tab1",
        vec![InjectedItem::new(
            "skill_y".into(),
            MemKind::Skill,
            "y body",
        )],
    );
    let bumped = obs.observe_fixture(&conn, "tab1", "ok");
    assert_eq!(bumped, vec!["skill_y".to_string()]);
    assert_eq!(
        evidence_count(&conn, "skill_x"),
        1,
        "x not in tracked batch when observe ran"
    );
    assert_eq!(evidence_count(&conn, "skill_y"), 2);
}

#[test]
fn observer_handles_missing_row_gracefully() {
    let conn = open_test_conn();
    let obs = EvolutionObserver::new();
    obs.record_injection(
        "tab1",
        vec![InjectedItem::new(
            "ghost".into(),
            MemKind::Skill,
            "vanished",
        )],
    );
    let bumped = obs.observe_fixture(&conn, "tab1", "ok");
    assert!(bumped.is_empty(), "missing row → no bump reported");
}

#[test]
fn is_success_shaped_recognizes_common_failure_words() {
    assert!(is_success_shaped("Done. exit code 0."));
    assert!(!is_success_shaped("ERROR: something blew up"));
    assert!(!is_success_shaped("the build failed"));
    assert!(!is_success_shaped("thread 'main' panicked"));
    assert!(!is_success_shaped("Traceback (most recent call last):"));
}
