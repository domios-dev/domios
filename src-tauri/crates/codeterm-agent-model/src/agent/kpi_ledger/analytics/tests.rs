use std::sync::Arc;

use codeterm_plan::service_plan::state::analytics::{BucketBound, TabCreatedKind};
use rusqlite::{params, Connection};

use super::super::{KpiEventInput, KpiEventKind, KpiLedger, TokenCounts};

const DAY: u64 = 86_400;
const START: u64 = 1_788_220_800;

fn temp_db() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "kpi-analytics-test-{}/kpi_ledger.db",
        ulid::Ulid::new()
    ))
}

fn cleanup(db: &std::path::Path) {
    std::fs::remove_dir_all(db.parent().unwrap()).ok();
}

fn buckets(count: u64) -> Vec<BucketBound> {
    (0..count)
        .map(|index| BucketBound {
            start: START + index * DAY,
            end: START + (index + 1) * DAY,
            label: format!("day-{index}"),
        })
        .collect()
}

fn token_event(
    timestamp: u64,
    workspace: Option<&str>,
    plan: Option<&str>,
    uncached: u64,
) -> KpiEventInput {
    KpiEventInput {
        plan_id: plan.map(str::to_string),
        task_id: None,
        tab_id: "tab-a".into(),
        role: None,
        provider: "prov-a".into(),
        model: Some("m1".into()),
        timestamp,
        kind: KpiEventKind::TokenUsage,
        bytes: uncached,
        tokens: Some(TokenCounts {
            input: uncached * 2,
            uncached_input: uncached,
            output: 1,
        }),
        dedup_key: Some(ulid::Ulid::new().to_string()),
        workspace_id: workspace.map(str::to_string),
        tab_kind: None,
    }
}

fn tab_event(timestamp: u64, workspace: Option<&str>, kind: TabCreatedKind) -> KpiEventInput {
    KpiEventInput {
        kind: KpiEventKind::TabCreated,
        tokens: None,
        bytes: 0,
        tab_kind: Some(kind),
        ..token_event(timestamp, workspace, None, 0)
    }
}

#[test]
fn analytics_host_aggregates_group_rows_by_bucket_and_scope() {
    let db = temp_db();
    let ledger = KpiLedger::open_at(&db).unwrap();
    for event in [
        token_event(START + 10, Some("ws-1"), Some("pl-1"), 5),
        token_event(START + 20, Some("ws-1"), Some("pl-1"), 7),
        token_event(START + DAY + 5, Some("ws-2"), None, 3),
        token_event(START + 2 * DAY, Some("ws-1"), None, 900),
        token_event(START - 1, Some("ws-1"), None, 900),
        tab_event(START + 30, Some("ws-1"), TabCreatedKind::Agent),
        tab_event(START + DAY + 1, None, TabCreatedKind::Terminal),
        KpiEventInput {
            kind: KpiEventKind::Send,
            tokens: None,
            bytes: 64,
            ..token_event(START + 40, Some("ws-1"), None, 0)
        },
    ] {
        ledger.insert(&event).unwrap();
    }

    let aggregates = ledger.analytics_aggregates(&buckets(2)).unwrap();
    assert_eq!(
        aggregates.tokens.len(),
        2,
        "rows outside the range stay out"
    );
    let first = aggregates
        .tokens
        .iter()
        .find(|row| row.bucket == 0)
        .unwrap();
    assert_eq!(first.turns, 2);
    assert_eq!(first.uncached_input_tokens, 12);
    assert_eq!(first.input_tokens, 24);
    assert_eq!(first.output_tokens, 2);
    assert_eq!(first.workspace_id.as_deref(), Some("ws-1"));
    assert_eq!(first.plan_id.as_deref(), Some("pl-1"));
    let second = aggregates
        .tokens
        .iter()
        .find(|row| row.bucket == 1)
        .unwrap();
    assert_eq!(second.workspace_id.as_deref(), Some("ws-2"));
    assert_eq!(second.plan_id, None);
    assert_eq!(second.uncached_input_tokens, 3);

    assert_eq!(aggregates.tabs.len(), 2);
    let agent = aggregates.tabs.iter().find(|row| row.bucket == 0).unwrap();
    assert_eq!(agent.tab_kind, TabCreatedKind::Agent);
    assert_eq!(agent.workspace_id.as_deref(), Some("ws-1"));
    assert_eq!(agent.count, 1);
    let terminal = aggregates.tabs.iter().find(|row| row.bucket == 1).unwrap();
    assert_eq!(terminal.tab_kind, TabCreatedKind::Terminal);
    assert_eq!(terminal.workspace_id, None);

    assert_eq!(aggregates.coverage.first_recorded_at, Some(START - 1));
    assert_eq!(aggregates.coverage.last_recorded_at, Some(START + 2 * DAY));
    assert!(aggregates.coverage.workspace_recorded_since.is_some());
    assert!(aggregates.coverage.tabs_counted_since.is_some());

    let rows = ledger.list_all().unwrap();
    let created = rows
        .iter()
        .find(|row| row.kind == KpiEventKind::TabCreated)
        .unwrap();
    assert_eq!(created.tab_kind, Some(TabCreatedKind::Agent));
    assert_eq!(created.workspace_id.as_deref(), Some("ws-1"));
    drop(ledger);
    cleanup(&db);
}

#[test]
fn analytics_host_cache_rereads_only_after_a_new_row_commits() {
    let db = temp_db();
    let ledger = KpiLedger::open_at(&db).unwrap();
    ledger
        .insert(&token_event(START + 10, Some("ws-1"), None, 5))
        .unwrap();

    let first = ledger.analytics_aggregates(&buckets(2)).unwrap();
    let again = ledger.analytics_aggregates(&buckets(2)).unwrap();
    assert!(
        Arc::ptr_eq(&first, &again),
        "an unchanged ledger answers from the cache"
    );
    assert_eq!(ledger.analytics_reads(), 1);

    ledger.analytics_aggregates(&buckets(1)).unwrap();
    assert_eq!(
        ledger.analytics_reads(),
        2,
        "a different bucket set is its own read"
    );
    ledger.analytics_aggregates(&buckets(2)).unwrap();
    assert_eq!(ledger.analytics_reads(), 2, "both bucket sets stay cached");

    ledger
        .insert(&token_event(START + 20, Some("ws-1"), None, 7))
        .unwrap();
    let fresh = ledger.analytics_aggregates(&buckets(2)).unwrap();
    assert_eq!(
        ledger.analytics_reads(),
        3,
        "a committed row invalidates the cache"
    );
    assert_eq!(fresh.tokens[0].turns, 2);
    drop(ledger);
    cleanup(&db);
}

#[test]
fn analytics_host_refuses_a_range_past_its_row_bound() {
    let db = temp_db();
    let ledger = KpiLedger::open_at(&db).unwrap();
    ledger
        .insert(&token_event(START + 1, Some("ws-1"), None, 1))
        .unwrap();
    ledger
        .insert(&token_event(START + 2, Some("ws-2"), None, 1))
        .unwrap();

    let refused = ledger.analytics_aggregates_within(&buckets(1), 1);
    assert!(refused.is_err(), "two grouped rows exceed a bound of one");
    let within = ledger.analytics_aggregates_within(&buckets(1), 2).unwrap();
    assert_eq!(within.tokens.len(), 2);
    drop(ledger);
    cleanup(&db);
}

#[test]
fn analytics_host_v4_migration_leaves_earlier_rows_unrecorded_without_backfill() {
    const OLD_TS: u64 = 1_700_000_000;
    let db = temp_db();
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    {
        let conn = Connection::open(&db).unwrap();
        KpiLedger::create_v1_schema(&conn).unwrap();
        KpiLedger::add_v2_token_columns(&conn).unwrap();
        KpiLedger::add_v3_dedup_and_cursors(&conn).unwrap();
        conn.execute(
            "INSERT INTO kpi_events (schema_version, tab_id, provider, ts, kind, bytes,
                                     input_tokens, uncached_input_tokens, output_tokens)
             VALUES (3, 'tab-old', 'prov-a', ?1, 'token_usage', 11, 10, 4, 1)",
            params![OLD_TS as i64],
        )
        .unwrap();
    }

    let ledger = KpiLedger::open_at(&db).unwrap();
    let rows = ledger.list_all().unwrap();
    assert_eq!(rows.len(), 1, "the v3 row survives the v4 migration");
    assert_eq!(rows[0].workspace_id, None);
    assert_eq!(rows[0].tab_kind, None);

    let window = [BucketBound {
        start: OLD_TS - 10,
        end: OLD_TS + DAY,
        label: "old".into(),
    }];
    let aggregates = ledger.analytics_aggregates(&window).unwrap();
    assert_eq!(aggregates.tokens.len(), 1);
    assert_eq!(aggregates.tokens[0].workspace_id, None);
    let since = aggregates
        .coverage
        .workspace_recorded_since
        .expect("the migration stamps the workspace mark");
    assert!(
        since > OLD_TS,
        "the mark dates the upgrade, after the row it leaves alone"
    );
    assert!(aggregates.coverage.tabs_counted_since.is_some());
    assert!(
        aggregates.tabs.is_empty(),
        "no tab-created row is invented for history"
    );
    drop(ledger);
    cleanup(&db);
}

#[test]
fn analytics_host_tab_created_rows_stay_out_of_board_plan_aggregates() {
    let db = temp_db();
    let ledger = KpiLedger::open_at(&db).unwrap();
    ledger
        .insert(&KpiEventInput {
            plan_id: Some("pl-1".into()),
            ..tab_event(START, Some("ws-1"), TabCreatedKind::Agent)
        })
        .unwrap();
    ledger
        .insert(&tab_event(START, None, TabCreatedKind::Terminal))
        .unwrap();
    ledger
        .insert(&KpiEventInput {
            kind: KpiEventKind::Send,
            tokens: None,
            bytes: 40,
            ..token_event(START, Some("ws-1"), Some("pl-1"), 0)
        })
        .unwrap();

    let plans = ledger.aggregate_by_plan().unwrap();
    assert_eq!(
        plans.len(),
        1,
        "a tab-created row never opens a plan bucket"
    );
    assert_eq!(plans[0].plan_id.as_deref(), Some("pl-1"));
    assert_eq!(plans[0].count, 1);
    assert_eq!(plans[0].total_bytes, 40);
    drop(ledger);
    cleanup(&db);
}
