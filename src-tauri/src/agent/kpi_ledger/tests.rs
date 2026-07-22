use super::*;
use std::collections::HashMap;

fn temp_db() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "kpi-ledger-test-{}/kpi_ledger.db",
        ulid::Ulid::new()
    ))
}

fn cleanup(db: &std::path::Path) {
    std::fs::remove_dir_all(db.parent().unwrap()).ok();
}

fn event(kind: KpiEventKind, plan_id: Option<&str>, pane_id: &str, bytes: u64) -> KpiEventInput {
    KpiEventInput {
        plan_id: plan_id.map(String::from),
        task_id: Some("task_1".into()),
        pane_id: pane_id.into(),
        role: Some("worker".into()),
        provider: "claude".into(),
        model: Some("claude-opus-4-8".into()),
        timestamp: 1_700_000_000,
        kind,
        bytes,
    }
}

#[test]
fn fresh_db_migrates_to_current_schema_version() {
    let db = temp_db();
    let ledger = KpiLedger::open_at(&db).unwrap();
    let conn = ledger.conn.lock();
    assert_eq!(
        KpiLedger::user_version(&conn).unwrap(),
        CURRENT_KPI_SCHEMA_VERSION
    );
    drop(conn);
    cleanup(&db);
}

#[test]
fn insert_and_list_round_trip_carries_schema_version() {
    let db = temp_db();
    let ledger = KpiLedger::open_at(&db).unwrap();

    ledger
        .insert(&event(KpiEventKind::Send, Some("pl_1"), "pane-a", 128))
        .unwrap();
    ledger
        .insert(&event(KpiEventKind::Report, Some("pl_1"), "pane-b", 512))
        .unwrap();

    let all = ledger.list_all().unwrap();
    assert_eq!(all.len(), 2, "append-only: both rows land, no upsert");
    for row in &all {
        assert_eq!(
            row.schema_version, CURRENT_KPI_SCHEMA_VERSION,
            "every row carries the current schema version"
        );
    }
    assert_eq!(all[0].kind, KpiEventKind::Send);
    assert_eq!(all[0].bytes, 128);
    assert_eq!(all[0].pane_id, "pane-a");
    assert_eq!(all[1].kind, KpiEventKind::Report);
    assert_eq!(all[1].bytes, 512);

    cleanup(&db);
}

#[test]
fn list_for_plan_scopes_and_excludes_unattributed() {
    let db = temp_db();
    let ledger = KpiLedger::open_at(&db).unwrap();

    ledger
        .insert(&event(KpiEventKind::Send, Some("pl_1"), "pane-a", 10))
        .unwrap();
    ledger
        .insert(&event(KpiEventKind::Send, Some("pl_2"), "pane-b", 20))
        .unwrap();
    ledger
        .insert(&event(KpiEventKind::Send, None, "pane-c", 30))
        .unwrap();

    let pl1 = ledger.list_for_plan("pl_1").unwrap();
    assert_eq!(pl1.len(), 1);
    assert_eq!(pl1[0].pane_id, "pane-a");

    let pl2 = ledger.list_for_plan("pl_2").unwrap();
    assert_eq!(pl2.len(), 1);
    assert_eq!(pl2[0].pane_id, "pane-b");

    assert_eq!(ledger.list_for_plan("nobody").unwrap().len(), 0);
    assert_eq!(
        ledger.list_all().unwrap().len(),
        3,
        "unattributed row still stored"
    );

    cleanup(&db);
}

#[test]
fn rows_survive_drop_and_reopen() {
    let db = temp_db();
    {
        let ledger = KpiLedger::open_at(&db).unwrap();
        ledger
            .insert(&event(KpiEventKind::Spawn, Some("pl_1"), "pane-a", 64))
            .unwrap();
    } // Connection dropped — simulates daemon restart.

    let reopened = KpiLedger::open_at(&db).unwrap();
    let rows = reopened.list_all().unwrap();
    assert_eq!(rows.len(), 1, "ledger rows persist across reopen");
    assert_eq!(rows[0].kind, KpiEventKind::Spawn);

    cleanup(&db);
}

#[test]
fn percentile_matches_textbook_nearest_rank_values() {
    let sorted: Vec<u64> = (1..=10).collect(); // [1..10]
    assert_eq!(percentile(&sorted, 50.0), 5, "ceil(0.50*10)=5th value");
    assert_eq!(percentile(&sorted, 95.0), 10, "ceil(0.95*10)=10th value");
    assert_eq!(percentile(&[], 50.0), 0, "empty input has no percentile");
    assert_eq!(
        percentile(&[42], 95.0),
        42,
        "single element is its own percentile"
    );
}

#[test]
fn aggregate_by_kind_counts_totals_and_percentiles() {
    let db = temp_db();
    let ledger = KpiLedger::open_at(&db).unwrap();

    for bytes in [10, 20, 30, 40, 50] {
        ledger
            .insert(&event(KpiEventKind::Send, Some("pl_1"), "pane-a", bytes))
            .unwrap();
    }
    ledger
        .insert(&event(KpiEventKind::Report, Some("pl_1"), "pane-b", 999))
        .unwrap();

    let agg = ledger.aggregate_by_kind(None).unwrap();
    let send = agg.iter().find(|a| a.kind == KpiEventKind::Send).unwrap();
    assert_eq!(send.count, 5);
    assert_eq!(send.total_bytes, 150);
    assert_eq!(
        send.p50_bytes, 30,
        "ceil(0.5*5)=3rd smallest of [10,20,30,40,50]"
    );
    assert_eq!(send.p95_bytes, 50, "ceil(0.95*5)=5th smallest");

    let report = agg.iter().find(|a| a.kind == KpiEventKind::Report).unwrap();
    assert_eq!(report.count, 1);
    assert_eq!(report.total_bytes, 999);

    assert!(
        agg.iter().all(|a| a.kind != KpiEventKind::Bounce),
        "kinds with zero events are omitted, not zero-filled"
    );

    cleanup(&db);
}

#[test]
fn aggregate_by_kind_scopes_to_plan() {
    let db = temp_db();
    let ledger = KpiLedger::open_at(&db).unwrap();

    ledger
        .insert(&event(KpiEventKind::Send, Some("pl_1"), "pane-a", 100))
        .unwrap();
    ledger
        .insert(&event(KpiEventKind::Send, Some("pl_2"), "pane-b", 200))
        .unwrap();

    let pl1_agg = ledger.aggregate_by_kind(Some("pl_1")).unwrap();
    assert_eq!(pl1_agg.len(), 1);
    assert_eq!(pl1_agg[0].total_bytes, 100);

    cleanup(&db);
}

#[test]
fn aggregate_by_plan_rolls_up_unattributed_to_none() {
    let db = temp_db();
    let ledger = KpiLedger::open_at(&db).unwrap();

    ledger
        .insert(&event(KpiEventKind::Send, Some("pl_1"), "pane-a", 10))
        .unwrap();
    ledger
        .insert(&event(KpiEventKind::Send, Some("pl_1"), "pane-b", 20))
        .unwrap();
    ledger
        .insert(&event(KpiEventKind::Send, None, "pane-c", 5))
        .unwrap();

    let agg = ledger.aggregate_by_plan().unwrap();
    let pl1 = agg
        .iter()
        .find(|a| a.plan_id.as_deref() == Some("pl_1"))
        .unwrap();
    assert_eq!(pl1.count, 2);
    assert_eq!(pl1.total_bytes, 30);

    let unattributed = agg.iter().find(|a| a.plan_id.is_none()).unwrap();
    assert_eq!(unattributed.count, 1);
    assert_eq!(unattributed.total_bytes, 5);

    cleanup(&db);
}

#[test]
fn kpi_ledger_disabled_by_default_in_config() {
    let config = crate::config::schema::AppConfig::default();
    assert!(
        !kpi_ledger_enabled(&config),
        "KPI ledger must be off by default until phase 3 wires writers"
    );
}

/// 1,000 mixed synthetic events, deterministically generated (no RNG — index-derived
/// fields keep this reproducible). Exercises write, read, per-kind aggregation,
/// per-plan aggregation, and p50/p95 size percentiles against independently
/// computed fixture values in one pass.
#[test]
fn thousand_mixed_synthetic_events_round_trip_and_aggregate() {
    let db = temp_db();
    let ledger = KpiLedger::open_at(&db).unwrap();

    const N: usize = 1000;
    let mut expected_kind_counts: HashMap<KpiEventKind, u64> = HashMap::new();
    let mut expected_kind_bytes: HashMap<KpiEventKind, Vec<u64>> = HashMap::new();
    let mut expected_plan_counts: HashMap<Option<String>, u64> = HashMap::new();
    let mut expected_plan_bytes: HashMap<Option<String>, u64> = HashMap::new();

    for i in 0..N {
        let kind = KpiEventKind::ALL[i % KpiEventKind::ALL.len()];
        let bytes = ((i * 37) % 5000 + 1) as u64;
        let plan_id = if i % 13 == 0 {
            None
        } else {
            Some(format!("pl_{}", i % 5))
        };
        let pane_id = format!("pane-{}", i % 20);

        ledger
            .insert(&KpiEventInput {
                plan_id: plan_id.clone(),
                task_id: Some(format!("task_{}", i % 7)),
                pane_id,
                role: Some("worker".into()),
                provider: "claude".into(),
                model: Some("claude-opus-4-8".into()),
                timestamp: 1_700_000_000 + i as u64,
                kind,
                bytes,
            })
            .unwrap();

        *expected_kind_counts.entry(kind).or_insert(0) += 1;
        expected_kind_bytes.entry(kind).or_default().push(bytes);
        *expected_plan_counts.entry(plan_id.clone()).or_insert(0) += 1;
        *expected_plan_bytes.entry(plan_id).or_insert(0) += bytes;
    }

    let all = ledger.list_all().unwrap();
    assert_eq!(all.len(), N);
    assert!(
        all.iter()
            .all(|r| r.schema_version == CURRENT_KPI_SCHEMA_VERSION),
        "schema_version asserted present on every one of the 1000 events"
    );

    let kind_agg = ledger.aggregate_by_kind(None).unwrap();
    assert_eq!(kind_agg.len(), expected_kind_counts.len());
    for agg in &kind_agg {
        assert_eq!(
            agg.count, expected_kind_counts[&agg.kind],
            "count for {:?}",
            agg.kind
        );
        let mut sizes = expected_kind_bytes[&agg.kind].clone();
        sizes.sort_unstable();
        let expected_total: u64 = sizes.iter().sum();
        assert_eq!(
            agg.total_bytes, expected_total,
            "total_bytes for {:?}",
            agg.kind
        );
        assert_eq!(
            agg.p50_bytes,
            percentile(&sizes, 50.0),
            "p50 for {:?}",
            agg.kind
        );
        assert_eq!(
            agg.p95_bytes,
            percentile(&sizes, 95.0),
            "p95 for {:?}",
            agg.kind
        );
    }

    let plan_agg = ledger.aggregate_by_plan().unwrap();
    assert_eq!(plan_agg.len(), expected_plan_counts.len());
    for agg in &plan_agg {
        assert_eq!(
            agg.count, expected_plan_counts[&agg.plan_id],
            "count for plan {:?}",
            agg.plan_id
        );
        assert_eq!(
            agg.total_bytes, expected_plan_bytes[&agg.plan_id],
            "total_bytes for plan {:?}",
            agg.plan_id
        );
    }

    cleanup(&db);
}
