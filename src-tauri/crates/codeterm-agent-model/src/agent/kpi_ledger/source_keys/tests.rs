use super::*;
use crate::agent::kpi_ledger::{CumulativeUsage, KpiLedger, UsageCursorState};
use rusqlite::params;

#[test]
fn usage_source_keys_normalize_windows_separators_without_changing_scope() {
    let slash = "C:/usage/session.jsonl\u{1}thread\\scope";
    let backslash = "C:\\usage\\session.jsonl\u{1}thread\\scope";
    assert_eq!(normalize_usage_source_key(backslash), slash);
    assert_eq!(normalize_usage_source_key(slash), slash);
    if !cfg!(windows) {
        assert_eq!(
            normalize_usage_source_key("/usage/part\\name"),
            "/usage/part\\name"
        );
    }
}

#[test]
fn usage_cursor_and_owner_aliases_share_one_canonical_write_and_lookup() {
    let root = tempfile::tempdir().unwrap();
    let ledger = KpiLedger::open(root.path()).unwrap();
    let slash = "C:/usage/session.jsonl\u{1}thread\\scope";
    let backslash = "C:\\usage\\session.jsonl\u{1}thread\\scope";
    {
        let conn = ledger.conn.lock();
        conn.execute(
            "INSERT INTO kpi_usage_cursors (source_path, cursor_offset) VALUES (?1, 5), (?2, 10)",
            params![slash, backslash],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO kpi_usage_source_owners (source_key, provider) VALUES (?1, 'fixture')",
            [backslash],
        )
        .unwrap();
    }
    assert_eq!(ledger.load_usage_cursor(slash).unwrap().unwrap().offset, 10);
    assert_eq!(
        ledger.load_usage_cursor(backslash).unwrap().unwrap().offset,
        10
    );
    assert_eq!(ledger.list_usage_cursors().unwrap().len(), 1);
    assert_eq!(
        ledger.usage_provider(slash).unwrap().as_deref(),
        Some("fixture")
    );
    assert_eq!(
        ledger.usage_provider(backslash).unwrap().as_deref(),
        Some("fixture")
    );
    assert!(ledger.remember_usage_provider(slash, "foreign").is_err());
    ledger.remember_usage_provider(slash, "fixture").unwrap();
    let state = UsageCursorState {
        offset: 10,
        cumulative: Some(CumulativeUsage {
            input: 20,
            cached_input: 5,
            output: 2,
            total: 22,
        }),
    };
    ledger.insert_usage_batch(&[], backslash, state).unwrap();
    assert_eq!(ledger.load_usage_cursor(slash).unwrap(), Some(state));
    assert_eq!(ledger.load_usage_cursor(backslash).unwrap(), Some(state));
    assert_eq!(
        ledger.list_usage_cursors().unwrap(),
        vec![(slash.to_owned(), state)]
    );
    let conn = ledger.conn.lock();
    for table in ["kpi_usage_cursors", "kpi_usage_source_owners"] {
        assert_eq!(
            conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
}
