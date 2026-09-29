use super::{
    bump_on_get, compose_appended_body, pick_restore_body, wake_up_options_from_params,
    WakeUpParams,
};
use crate::mem::store::HistoryEntry;
use rusqlite::Connection;

#[test]
fn save_same_body_skips_extraction_schedule() {
    assert!(!super::save_schedules_extraction(
        "unchanged",
        false,
        false,
        true
    ));
    assert!(super::save_schedules_extraction(
        "updated", false, false, true
    ));
}

#[test]
fn unknown_mem_scope_is_refused() {
    assert!(crate::mem::store::MemScope::from_str("misspelled").is_err());
    assert!(crate::mem::store::MemScope::from_str("global").is_ok());
}

fn hist(id: i64, old: Option<&str>, new: Option<&str>) -> HistoryEntry {
    HistoryEntry {
        id,
        item_id: "x".into(),
        changed_at: "t".into(),
        changed_by: "user".into(),
        change_type: "update".into(),
        changed_fields: Vec::new(),
        old_title: None,
        new_title: None,
        old_body: old.map(String::from),
        new_body: new.map(String::from),
        old_importance: None,
        new_importance: None,
        old_kind: None,
        new_kind: None,
        reason: None,
    }
}

#[test]
fn append_to_empty_replaces_outright() {
    assert_eq!(compose_appended_body("", "section"), "section");
    assert_eq!(compose_appended_body("   \n", "section"), "section");
}

#[test]
fn append_separates_with_blank_line_and_keeps_old() {
    let out = compose_appended_body("old body", "new section");
    assert_eq!(out, "old body\n\nnew section");
    assert!(out.starts_with("old body"));
}

#[test]
fn append_collapses_trailing_newlines_before_separator() {
    assert_eq!(compose_appended_body("old\n\n", "new"), "old\n\nnew");
}

#[test]
fn restore_default_picks_latest_old_body() {
    let h = vec![
        hist(1, None, Some("v1")),
        hist(2, Some("v1"), Some("v2")),
        hist(3, Some("v2"), Some("v3")),
    ];
    assert_eq!(pick_restore_body(&h, None).unwrap(), "v2");
}

#[test]
fn restore_version_picks_that_rows_new_body() {
    let h = vec![hist(1, None, Some("v1")), hist(2, Some("v1"), Some("v2"))];
    assert_eq!(pick_restore_body(&h, Some(1)).unwrap(), "v1");
    assert_eq!(pick_restore_body(&h, Some(2)).unwrap(), "v2");
}

#[test]
fn restore_unknown_version_errors() {
    let h = vec![hist(1, None, Some("v1"))];
    assert!(pick_restore_body(&h, Some(99)).is_err());
}

#[test]
fn restore_with_no_body_history_errors() {
    let h = vec![hist(1, None, None)];
    assert!(pick_restore_body(&h, None).is_err());
}

fn open_test_conn() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    crate::mem::store::init_schema(&conn).unwrap();
    conn
}

fn insert(conn: &Connection, id: &str, kind: &str) {
    conn.execute(
        "INSERT INTO mem_items (id, uid, kind, scope, title, tags_json, created_at, updated_at)
         VALUES (?1, ?1, ?2, 'global', 't', '[]', '2026-01-01', '2026-01-01')",
        rusqlite::params![id, kind],
    )
    .unwrap();
}

fn evidence(conn: &Connection, id: &str) -> i64 {
    conn.query_row(
        "SELECT evidence_count FROM mem_items WHERE id = ?1",
        rusqlite::params![id],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn bump_on_get_increments_for_lesson() {
    let conn = open_test_conn();
    insert(&conn, "lesson_a", "lesson");
    bump_on_get(&conn, "lesson", "lesson_a");
    assert_eq!(evidence(&conn, "lesson_a"), 2, "default 1 + 1 bump = 2");
}

#[test]
fn bump_on_get_increments_for_skill() {
    let conn = open_test_conn();
    insert(&conn, "skill_a", "skill");
    bump_on_get(&conn, "skill", "skill_a");
    assert_eq!(evidence(&conn, "skill_a"), 2);
}

#[test]
fn bump_on_get_skips_other_kinds() {
    let conn = open_test_conn();
    insert(&conn, "note_a", "note");
    bump_on_get(&conn, "note", "note_a");
    assert_eq!(evidence(&conn, "note_a"), 1, "kind=note must not bump");
}

#[test]
fn bump_on_get_fires_for_fact() {
    let conn = open_test_conn();
    insert(&conn, "fact_a", "fact");
    bump_on_get(&conn, "fact", "fact_a");
    assert_eq!(evidence(&conn, "fact_a"), 2);
}

#[test]
fn bump_on_get_fires_for_rule() {
    let conn = open_test_conn();
    insert(&conn, "rule_a", "rule");
    bump_on_get(&conn, "rule", "rule_a");
    assert_eq!(evidence(&conn, "rule_a"), 2);
}

#[test]
fn bump_on_get_fires_for_preference() {
    let conn = open_test_conn();
    insert(&conn, "pref_a", "preference");
    bump_on_get(&conn, "preference", "pref_a");
    assert_eq!(evidence(&conn, "pref_a"), 2);
}

#[test]
fn bump_on_get_is_safe_when_row_missing() {
    let conn = open_test_conn();
    // No insert — must not panic / error.
    bump_on_get(&conn, "skill", "ghost_id");
}

#[test]
fn wake_up_budget_below_l0_floor_does_not_underflow() {
    let opts = wake_up_options_from_params(
        &WakeUpParams {
            budget: Some(10),
            l0_only: None,
            no_l0: None,
            explain: None,
            workspace: None,
        },
        None,
    );

    assert_eq!(opts.l0_budget_tokens, 10);
    assert_eq!(opts.l1_budget_tokens, 0);
}

#[test]
fn wake_up_no_l0_uses_full_budget_for_l1() {
    let opts = wake_up_options_from_params(
        &WakeUpParams {
            budget: Some(10),
            l0_only: None,
            no_l0: Some(true),
            explain: None,
            workspace: None,
        },
        None,
    );

    assert_eq!(opts.l0_budget_tokens, 0);
    assert_eq!(opts.l1_budget_tokens, 10);
    assert!(!opts.include_l0);
    assert!(opts.include_l1);
}

#[test]
fn wake_up_large_budget_caps_l0_share() {
    let opts = wake_up_options_from_params(
        &WakeUpParams {
            budget: Some(2_000),
            l0_only: None,
            no_l0: None,
            explain: None,
            workspace: None,
        },
        None,
    );

    assert_eq!(opts.l0_budget_tokens, 100);
    assert_eq!(opts.l1_budget_tokens, 1_900);
}

#[test]
fn wake_up_no_budget_uses_config_default() {
    let opts = wake_up_options_from_params(
        &WakeUpParams {
            budget: None,
            l0_only: None,
            no_l0: None,
            explain: None,
            workspace: None,
        },
        None,
    );
    assert_eq!(
        opts.l1_budget_tokens,
        crate::config::schema::MemoryConfig::default().l1_budget_tokens
    );
}

#[test]
fn wake_up_workspace_param_propagates_to_options() {
    let opts = wake_up_options_from_params(
        &WakeUpParams {
            budget: None,
            l0_only: None,
            no_l0: None,
            explain: None,
            workspace: Some("W1".into()),
        },
        Some("caller-workspace".into()),
    );
    assert_eq!(opts.current_workspace.as_deref(), Some("W1"));
}

#[test]
fn wake_up_inherits_calling_tab_workspace_when_not_overridden() {
    let opts = wake_up_options_from_params(
        &WakeUpParams {
            budget: None,
            l0_only: None,
            no_l0: None,
            explain: None,
            workspace: None,
        },
        Some("caller-workspace".into()),
    );
    assert_eq!(opts.current_workspace.as_deref(), Some("caller-workspace"));
}
