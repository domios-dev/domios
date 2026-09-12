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

fn event(kind: KpiEventKind, plan_id: Option<&str>, tab_id: &str, bytes: u64) -> KpiEventInput {
    KpiEventInput {
        plan_id: plan_id.map(String::from),
        task_id: Some("task_1".into()),
        tab_id: tab_id.into(),
        role: Some("worker".into()),
        provider: "claude".into(),
        model: Some("claude-opus-4-8".into()),
        timestamp: 1_700_000_000,
        kind,
        bytes,
        tokens: None,
        dedup_key: None,
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
        .insert(&event(KpiEventKind::Send, Some("pl_1"), "tab-a", 128))
        .unwrap();
    ledger
        .insert(&event(KpiEventKind::Report, Some("pl_1"), "tab-b", 512))
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
    assert_eq!(all[0].tab_id, "tab-a");
    assert_eq!(all[1].kind, KpiEventKind::Report);
    assert_eq!(all[1].bytes, 512);

    cleanup(&db);
}

#[test]
fn list_for_plan_scopes_and_excludes_unattributed() {
    let db = temp_db();
    let ledger = KpiLedger::open_at(&db).unwrap();

    ledger
        .insert(&event(KpiEventKind::Send, Some("pl_1"), "tab-a", 10))
        .unwrap();
    ledger
        .insert(&event(KpiEventKind::Send, Some("pl_2"), "tab-b", 20))
        .unwrap();
    ledger
        .insert(&event(KpiEventKind::Send, None, "tab-c", 30))
        .unwrap();

    let pl1 = ledger.list_for_plan("pl_1").unwrap();
    assert_eq!(pl1.len(), 1);
    assert_eq!(pl1[0].tab_id, "tab-a");

    let pl2 = ledger.list_for_plan("pl_2").unwrap();
    assert_eq!(pl2.len(), 1);
    assert_eq!(pl2[0].tab_id, "tab-b");

    assert_eq!(ledger.list_for_plan("nobody").unwrap().len(), 0);
    assert_eq!(
        ledger.list_all().unwrap().len(),
        3,
        "unattributed row still stored"
    );

    cleanup(&db);
}

/// The exact fixture the task's own test plan calls for: an accepted lifecycle event
/// retried twice with the same dedup key, plus once with a distinct key, must land as
/// exactly two rows — a retried choke-point call is a no-op, a genuinely new event isn't.
#[test]
fn retried_events_with_the_same_dedup_key_collapse_to_one_row() {
    let db = temp_db();
    let ledger = KpiLedger::open_at(&db).unwrap();

    let retried = KpiEventInput {
        dedup_key: Some("task_1:0:accept".into()),
        ..event(KpiEventKind::Accept, Some("pl_1"), "tab-a", 10)
    };
    ledger.insert(&retried).unwrap();
    ledger.insert(&retried).unwrap(); // simulated RPC retry — same round, same verdict

    let distinct = KpiEventInput {
        dedup_key: Some("task_2:0:bounce".into()),
        ..event(KpiEventKind::Bounce, Some("pl_1"), "tab-a", 20)
    };
    ledger.insert(&distinct).unwrap();

    let rows = ledger.list_all().unwrap();
    assert_eq!(
        rows.len(),
        2,
        "two retries of the same dedup key plus one distinct key is exactly two rows"
    );
    assert_eq!(rows[0].kind, KpiEventKind::Accept);
    assert_eq!(rows[0].dedup_key.as_deref(), Some("task_1:0:accept"));
    assert_eq!(rows[1].kind, KpiEventKind::Bounce);

    cleanup(&db);
}

#[test]
fn events_without_a_dedup_key_never_collide_with_each_other() {
    let db = temp_db();
    let ledger = KpiLedger::open_at(&db).unwrap();

    for _ in 0..3 {
        ledger
            .insert(&event(KpiEventKind::Send, Some("pl_1"), "tab-a", 5))
            .unwrap();
    }

    assert_eq!(
        ledger.list_all().unwrap().len(),
        3,
        "kinds that leave dedup_key=None insert unconditionally, same as before the column existed"
    );

    cleanup(&db);
}

#[test]
fn rows_survive_drop_and_reopen() {
    let db = temp_db();
    {
        let ledger = KpiLedger::open_at(&db).unwrap();
        ledger
            .insert(&event(KpiEventKind::Spawn, Some("pl_1"), "tab-a", 64))
            .unwrap();
    } // Connection dropped — simulates daemon restart.

    let reopened = KpiLedger::open_at(&db).unwrap();
    let rows = reopened.list_all().unwrap();
    assert_eq!(rows.len(), 1, "ledger rows persist across reopen");
    assert_eq!(rows[0].kind, KpiEventKind::Spawn);

    cleanup(&db);
}

#[test]
fn usage_cursor_round_trips_through_insert_usage_batch() {
    let db = temp_db();
    let ledger = KpiLedger::open_at(&db).unwrap();

    assert_eq!(
        ledger.load_usage_cursor("/sessions/a.jsonl").unwrap(),
        None,
        "a never-ingested source has no watermark"
    );

    ledger
        .insert_usage_batch(
            &[event(KpiEventKind::TokenUsage, Some("pl_1"), "tab-a", 10)],
            "/sessions/a.jsonl",
            4_096,
        )
        .unwrap();

    assert_eq!(
        ledger.load_usage_cursor("/sessions/a.jsonl").unwrap(),
        Some(4_096)
    );
    assert_eq!(ledger.list_all().unwrap().len(), 1);

    // A later pass over the same source updates the watermark in place, not a new row.
    ledger
        .insert_usage_batch(&[], "/sessions/a.jsonl", 8_192)
        .unwrap();
    assert_eq!(
        ledger.load_usage_cursor("/sessions/a.jsonl").unwrap(),
        Some(8_192)
    );

    cleanup(&db);
}

#[test]
fn v1_ledger_migrates_to_v2_keeping_existing_rows() {
    let db = temp_db();
    {
        std::fs::create_dir_all(db.parent().unwrap()).unwrap();
        let conn = Connection::open(&db).unwrap();
        KpiLedger::create_v1_schema(&conn).unwrap();
        conn.execute(
            "INSERT INTO kpi_events (schema_version, plan_id, tab_id, provider, ts, kind, bytes)
             VALUES (1, 'pl_1', 'tab-a', 'claude', 1700000000, 'send', 42)",
            [],
        )
        .unwrap();
        assert_eq!(KpiLedger::user_version(&conn).unwrap(), 1);
    }

    let ledger = KpiLedger::open_at(&db).unwrap();
    {
        let conn = ledger.conn.lock();
        assert_eq!(
            KpiLedger::user_version(&conn).unwrap(),
            CURRENT_KPI_SCHEMA_VERSION
        );
    }
    let rows = ledger.list_all().unwrap();
    assert_eq!(rows.len(), 1, "v1 row survives the v2 migration");
    assert_eq!(rows[0].bytes, 42);
    assert_eq!(rows[0].tokens, None, "pre-v2 rows have no token breakdown");

    ledger
        .insert(&KpiEventInput {
            tokens: Some(TokenCounts {
                input: 1000,
                uncached_input: 250,
                output: 40,
            }),
            ..event(KpiEventKind::TokenUsage, Some("pl_1"), "tab-a", 1040)
        })
        .unwrap();
    let rows = ledger.list_all().unwrap();
    assert_eq!(
        rows[1].tokens,
        Some(TokenCounts {
            input: 1000,
            uncached_input: 250,
            output: 40
        })
    );

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
            .insert(&event(KpiEventKind::Send, Some("pl_1"), "tab-a", bytes))
            .unwrap();
    }
    ledger
        .insert(&event(KpiEventKind::Report, Some("pl_1"), "tab-b", 999))
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
        .insert(&event(KpiEventKind::Send, Some("pl_1"), "tab-a", 100))
        .unwrap();
    ledger
        .insert(&event(KpiEventKind::Send, Some("pl_2"), "tab-b", 200))
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
        .insert(&event(KpiEventKind::Send, Some("pl_1"), "tab-a", 10))
        .unwrap();
    ledger
        .insert(&event(KpiEventKind::Send, Some("pl_1"), "tab-b", 20))
        .unwrap();
    ledger
        .insert(&event(KpiEventKind::Send, None, "tab-c", 5))
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
fn kpi_ledger_enabled_by_default_in_config() {
    let config = crate::config::schema::AppConfig::default();
    assert!(
        kpi_ledger_enabled(&config),
        "the shipped default records; the opt-out is the explicit false in config"
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
        let tab_id = format!("tab-{}", i % 20);

        ledger
            .insert(&KpiEventInput {
                plan_id: plan_id.clone(),
                task_id: Some(format!("task_{}", i % 7)),
                tab_id,
                role: Some("worker".into()),
                provider: "claude".into(),
                model: Some("claude-opus-4-8".into()),
                timestamp: 1_700_000_000 + i as u64,
                kind,
                bytes,
                tokens: None,
                dedup_key: None,
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

/// The policy this lane settled: recording is on unless someone opts out, and the gate
/// every writer consults follows the config rather than a second switch.
#[test]
fn kpi_policy_default_or_explicit_opt_in() {
    use crate::agent::kpi::KpiRecorder;
    use crate::config::schema::{AppConfig, KpiConfig, KPI_ENABLED_KEY};

    let default_config = AppConfig::default();
    assert!(
        default_config.kpi.enabled,
        "a fresh install records: both preconditions the flag waited on now ship"
    );
    assert!(kpi_ledger_enabled(&default_config));

    let opted_out = AppConfig {
        kpi: KpiConfig { enabled: false },
        ..AppConfig::default()
    };
    assert!(
        !kpi_ledger_enabled(&opted_out),
        "the opt-out is honoured by the single gate every writer consults"
    );

    let dir = std::env::temp_dir().join(format!("kpi-policy-{}", ulid::Ulid::new()));
    let recorder = KpiRecorder::from_config(&opted_out, &dir);
    assert!(!recorder.is_enabled());
    assert!(
        !recorder.open_failed(),
        "an opt-out is not a ledger failure, so the surface never blames the ledger"
    );
    assert!(recorder.ledger().is_none());
    assert!(
        !dir.join("kpi_ledger.db").exists(),
        "opting out touches no disk at all"
    );

    let enabled = KpiRecorder::from_config(&AppConfig::default(), &dir);
    assert!(enabled.is_enabled());
    assert!(enabled.ledger().is_some());
    drop(enabled);

    // The key the not-recording surface names must be a real path into this config.
    let as_json = serde_json::to_value(&default_config).unwrap();
    let mut cursor = &as_json;
    for segment in KPI_ENABLED_KEY.split('.') {
        cursor = cursor.get(segment).unwrap_or_else(|| {
            panic!(
                "{} must address a real config field, missing {}",
                KPI_ENABLED_KEY, segment
            )
        });
    }
    assert_eq!(cursor.as_bool(), Some(true));

    std::fs::remove_dir_all(&dir).ok();
}

/// Config files written before `[kpi]` existed must keep loading, and an explicit
/// choice must survive the load/serialize round trip the config route performs.
#[test]
fn kpi_config_round_trips_without_losing_enabled() {
    use crate::config::schema::AppConfig;

    let legacy = r#"
[theme]
name = "dracula"

[font]
size = 16
"#;
    let loaded: AppConfig = toml::from_str(legacy).expect("a config with no [kpi] must still load");
    assert_eq!(loaded.theme.name, "dracula");
    assert!(
        loaded.kpi.enabled,
        "an absent section takes the default, it does not fail the load"
    );

    let opted_out: AppConfig = toml::from_str("[kpi]\nenabled = false\n").unwrap();
    assert!(!opted_out.kpi.enabled);
    let opted_in: AppConfig = toml::from_str("[kpi]\nenabled = true\n").unwrap();
    assert!(opted_in.kpi.enabled);

    for original in [opted_out, opted_in] {
        let expected = original.kpi.enabled;
        let toml_text = toml::to_string_pretty(&original).expect("config must serialize");
        let reloaded: AppConfig = toml::from_str(&toml_text).expect("config must reload");
        assert_eq!(
            reloaded.kpi.enabled, expected,
            "a write-back must not flip the stored choice"
        );

        let json = serde_json::to_value(&original).unwrap();
        assert_eq!(json["kpi"]["enabled"].as_bool(), Some(expected));
        let from_json: AppConfig = serde_json::from_value(json).unwrap();
        assert_eq!(from_json.kpi.enabled, expected);
    }
}
