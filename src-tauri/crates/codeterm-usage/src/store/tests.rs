use super::*;

fn make_snapshot(ms: i64) -> UsageSample {
    let mut s = UsageSample::empty("claude", "raw", ms);
    s.session_pct = Some(42.0);
    s.weekly_pct = Some(10.0);
    s
}

fn guarded_snapshot(
    ms: i64,
    session_pct: f32,
    weekly_pct: f32,
    session_reset: i64,
    weekly_reset: i64,
) -> UsageSample {
    let mut s = UsageSample::empty("codex", "OpenAI Codex\nraw usage screen", ms);
    s.session_pct = Some(session_pct);
    s.weekly_pct = Some(weekly_pct);
    s.session_resets_at_ms = Some(session_reset);
    s.weekly_resets_at_ms = Some(weekly_reset);
    s
}

#[test]
fn add_missing_columns_migrates_pre_spend_db() {
    // Pre-spend schema: original columns only, no spend_* yet.
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE usage_snapshots (
           id INTEGER PRIMARY KEY AUTOINCREMENT,
           provider TEXT NOT NULL, account_id TEXT,
           captured_at INTEGER NOT NULL, parsed_ok INTEGER NOT NULL,
           session_pct REAL, session_resets_at INTEGER,
           weekly_pct REAL, weekly_resets_at INTEGER);",
    )
    .unwrap();
    assert!(!table_columns(&conn).unwrap().contains("spend_cents"));

    add_missing_columns(&conn).unwrap();
    let cols = table_columns(&conn).unwrap();
    assert!(cols.contains("spend_cents"), "spend_cents added");
    assert!(
        cols.contains("spend_limit_cents"),
        "spend_limit_cents added"
    );
    assert!(
        cols.contains("spend_remaining_cents"),
        "spend_remaining_cents added"
    );
    assert!(cols.contains("spend_resets_at"), "spend_resets_at added");
    assert!(cols.contains("raw_excerpt"), "raw_excerpt added");
    assert!(cols.contains("debug_json"), "debug_json added");

    // Idempotent: a second pass on an already-migrated DB is a no-op.
    add_missing_columns(&conn).unwrap();
}

#[test]
fn insert_select_roundtrip() {
    let store = UsageStore::open_in_memory().unwrap();
    let snap = make_snapshot(1_000_000_000_000);
    let id = store.insert_snapshot(&snap).unwrap();
    assert!(id > 0);
    let got = store.get_latest("claude", None).unwrap().unwrap();
    assert_eq!(got.id, id);
    assert_eq!(got.session_pct, Some(42.0));
    assert_eq!(got.weekly_pct, Some(10.0));
}

#[test]
fn history_returns_newest_first() {
    let store = UsageStore::open_in_memory().unwrap();
    for ms in [1_000_000_000_000_i64, 1_000_000_500_000, 1_000_001_000_000] {
        store.insert_snapshot(&make_snapshot(ms)).unwrap();
    }
    let hist = store.get_history("claude", 0, 10, None).unwrap();
    assert_eq!(hist.len(), 3);
    assert!(hist[0].captured_at > hist[1].captured_at);
    assert!(hist[1].captured_at > hist[2].captured_at);
}

#[test]
fn daily_rollup_arithmetic_on_three_days() {
    let store = UsageStore::open_in_memory().unwrap();
    let day_a = 1_700_000_000_000_i64;
    let day_b = day_a + 86_400_000;
    let day_c = day_a + 2 * 86_400_000;
    let mut snaps = Vec::new();
    for &ms in &[day_a, day_a + 1000, day_b, day_c, day_c + 5000] {
        snaps.push(make_snapshot(ms));
    }
    snaps[0].session_pct = Some(20.0);
    snaps[1].session_pct = Some(40.0);
    snaps[2].session_pct = Some(50.0);
    snaps[3].session_pct = Some(10.0);
    snaps[4].session_pct = Some(70.0);
    for s in &snaps {
        store.insert_snapshot(s).unwrap();
    }
    let roll = store.get_daily_rollup("claude", 0, None).unwrap();
    assert_eq!(roll.len(), 3, "three distinct days");
    let day0 = &roll[0];
    assert_eq!(day0.avg_session_pct, Some(30.0), "avg of 20+40");
    assert_eq!(day0.max_session_pct, Some(40.0));
    let day2 = &roll[2];
    assert_eq!(day2.max_session_pct, Some(70.0));
}

#[test]
fn get_latest_skips_parse_failures() {
    let store = UsageStore::open_in_memory().unwrap();
    let good = make_snapshot(1_000_000_000_000);
    store.insert_snapshot(&good).unwrap();
    store
        .insert_failure("claude", None, Some("raw"), "regex panic", Some("/usage"))
        .unwrap();
    let latest = store.get_latest("claude", None).unwrap().unwrap();
    assert!(latest.parsed_ok, "get_latest must skip failed snapshots");
    assert_eq!(latest.session_pct, Some(42.0));
    let hist = store.get_history("claude", 0, 10, None).unwrap();
    assert_eq!(hist.len(), 2, "history still contains the failure row");
    assert_eq!(
        hist[0].failure_reason.as_deref(),
        Some("PARSE_ERROR: regex panic")
    );
    assert_eq!(hist[0].probe_command.as_deref(), Some("/usage"));
    assert_eq!(hist[0].raw_excerpt.as_deref(), Some("raw"));
    assert!(hist[0]
        .debug_json
        .as_deref()
        .unwrap_or("")
        .contains("regex panic"));
}

#[test]
fn codex_zero_zero_with_backward_resets_is_rejected() {
    let store = UsageStore::open_in_memory().unwrap();
    let prev = guarded_snapshot(
        1_783_601_000_000,
        6.0,
        9.0,
        1_783_609_680_000,
        1_784_139_960_000,
    );
    store.insert_snapshot(&prev).unwrap();

    let bad = guarded_snapshot(
        1_783_601_095_000,
        0.0,
        0.0,
        1_783_608_240_000,
        1_784_132_760_000,
    );
    store.insert_snapshot(&bad).unwrap();

    let latest = store.get_latest("codex", None).unwrap().unwrap();
    assert_eq!(latest.session_pct, Some(6.0));
    let hist = store.get_history("codex", 0, 10, None).unwrap();
    assert!(!hist[0].parsed_ok);
    assert_eq!(hist[0].session_pct, Some(0.0));
    assert_eq!(hist[0].weekly_pct, Some(0.0));
    assert!(hist[0]
        .failure_reason
        .as_deref()
        .unwrap_or("")
        .contains("0%/0%"));
    assert!(hist[0]
        .raw_excerpt
        .as_deref()
        .unwrap_or("")
        .contains("OpenAI Codex"));
}

#[test]
fn codex_hundred_hundred_with_identical_resets_is_rejected() {
    let store = UsageStore::open_in_memory().unwrap();
    let prev = guarded_snapshot(
        1_783_601_000_000,
        6.0,
        9.0,
        1_783_609_680_000,
        1_784_139_960_000,
    );
    store.insert_snapshot(&prev).unwrap();

    let bad = guarded_snapshot(
        1_783_601_095_000,
        100.0,
        100.0,
        1_783_700_000_000,
        1_783_700_000_000,
    );
    store.insert_snapshot(&bad).unwrap();

    let hist = store.get_history("codex", 0, 10, None).unwrap();
    assert!(!hist[0].parsed_ok);
    assert!(hist[0]
        .failure_reason
        .as_deref()
        .unwrap_or("")
        .contains("100%/100%"));
}

/// The failure mode that cost a day: the gauge kept rendering its last good
/// reading and nothing anywhere said why it had stopped moving. The newest
/// failure now rides on the row the panel already draws.
#[test]
fn a_frozen_reading_carries_the_failure_that_froze_it() {
    let store = UsageStore::open_in_memory().unwrap();
    let now = chrono::Utc::now().timestamp_millis();
    store
        .insert_snapshot(&guarded_snapshot(now - 86_400_000, 12.0, 7.0, 0, 0))
        .unwrap();
    store
        .insert_failure_kind(
            "codex",
            None,
            None,
            UsageFailureKind::CapabilityDenied,
            "plugin denied readFile",
            None,
        )
        .unwrap();

    let row = store
        .get_latest_with_stale_reason("codex", None)
        .unwrap()
        .expect("the last good reading is still what the panel renders");
    assert!(row.parsed_ok, "the rendered row is still the good reading");
    assert_eq!(row.weekly_pct, Some(7.0));
    let reason = row
        .failure_reason
        .as_deref()
        .expect("staleness must name its cause");
    assert!(
        reason.starts_with(UsageFailureKind::CapabilityDenied.as_str()),
        "the cause is carried as the kind constant, got {reason}"
    );
}

#[test]
fn an_account_with_only_failed_reads_surfaces_its_failure() {
    let store = UsageStore::open_in_memory().unwrap();
    store
        .insert_failure_kind(
            "fixture",
            Some("profile:only-failed"),
            None,
            UsageFailureKind::FetchError,
            "opaque cause",
            None,
        )
        .unwrap();

    assert!(store
        .get_latest_with_stale_reason("fixture", Some("profile:only-failed"))
        .unwrap()
        .is_none());
    let row = store
        .get_latest_or_failure("fixture", Some("profile:only-failed"))
        .unwrap()
        .expect("a failed read is what the account has to show");
    assert!(!row.parsed_ok);
    assert!(row.failure_reason.unwrap().ends_with("opaque cause"));
    assert!(store
        .get_latest_or_failure("fixture", Some("profile:unread"))
        .unwrap()
        .is_none());
}

#[test]
fn a_kept_reading_outranks_a_newer_failure_and_carries_its_cause() {
    let store = UsageStore::open_in_memory().unwrap();
    let now = chrono::Utc::now().timestamp_millis();
    let kept = guarded_snapshot(now - 86_400_000, 12.0, 7.0, 0, 0);
    store.insert_snapshot(&kept).unwrap();
    store
        .insert_failure_kind(
            &kept.provider,
            None,
            None,
            UsageFailureKind::FetchError,
            "opaque cause",
            None,
        )
        .unwrap();

    let row = store
        .get_latest_or_failure(&kept.provider, None)
        .unwrap()
        .unwrap();
    assert!(row.parsed_ok);
    assert!(row.failure_reason.unwrap().ends_with("opaque cause"));
}

#[test]
fn a_reading_newer_than_the_last_failure_is_not_marked_stale() {
    let store = UsageStore::open_in_memory().unwrap();
    let now = chrono::Utc::now().timestamp_millis();
    store
        .insert_failure_kind(
            "codex",
            None,
            None,
            UsageFailureKind::CapabilityDenied,
            "plugin denied readFile",
            None,
        )
        .unwrap();
    store
        .insert_snapshot(&guarded_snapshot(now + 3_600_000, 12.0, 7.0, 0, 0))
        .unwrap();

    let row = store
        .get_latest_with_stale_reason("codex", None)
        .unwrap()
        .unwrap();
    assert_eq!(
        row.failure_reason, None,
        "a recovered probe must not keep flagging a cause that has passed"
    );
}

#[test]
fn codex_zero_after_supported_reset_is_accepted() {
    let store = UsageStore::open_in_memory().unwrap();
    let prev = guarded_snapshot(
        1_783_601_000_000,
        98.0,
        9.0,
        1_783_609_680_000,
        1_784_139_960_000,
    );
    store.insert_snapshot(&prev).unwrap();

    let reset = guarded_snapshot(
        1_783_610_000_000,
        0.0,
        9.0,
        1_783_628_000_000,
        1_784_139_960_000,
    );
    store.insert_snapshot(&reset).unwrap();

    let latest = store.get_latest("codex", None).unwrap().unwrap();
    assert!(latest.parsed_ok);
    assert_eq!(latest.session_pct, Some(0.0));
    assert_eq!(latest.weekly_pct, Some(9.0));
}

#[test]
fn codex_exhaustion_with_distinct_resets_is_accepted() {
    let store = UsageStore::open_in_memory().unwrap();
    let prev = guarded_snapshot(
        1_783_601_000_000,
        98.0,
        99.0,
        1_783_609_680_000,
        1_784_139_960_000,
    );
    store.insert_snapshot(&prev).unwrap();

    let full = guarded_snapshot(
        1_783_601_095_000,
        100.0,
        100.0,
        1_783_609_680_000,
        1_784_139_960_000,
    );
    store.insert_snapshot(&full).unwrap();

    let latest = store.get_latest("codex", None).unwrap().unwrap();
    assert!(latest.parsed_ok);
    assert_eq!(latest.session_pct, Some(100.0));
    assert_eq!(latest.weekly_pct, Some(100.0));
}

#[test]
fn failure_taxonomy_is_persisted_in_reason_prefix() {
    let store = UsageStore::open_in_memory().unwrap();
    for (kind, detail) in [
        (UsageFailureKind::Timeout, "agent not ready"),
        (UsageFailureKind::FetchError, "request timed out"),
        (UsageFailureKind::ParseError, "bad frame"),
        (UsageFailureKind::Skip, "empty usage frame"),
    ] {
        store
            .insert_failure_kind("cursor", None, Some("raw"), kind, detail, Some("/usage"))
            .unwrap();
    }

    let hist = store.get_history("cursor", 0, 10, None).unwrap();
    let mut reasons: Vec<_> = hist
        .iter()
        .filter_map(|row| row.failure_reason.as_deref())
        .collect();
    reasons.sort_unstable();
    assert_eq!(
        reasons,
        vec![
            "FETCH_ERROR: request timed out",
            "PARSE_ERROR: bad frame",
            "SKIP: empty usage frame",
            "TIMEOUT: agent not ready"
        ]
    );
}

#[test]
fn subscription_billing_without_limits_roundtrips() {
    let store = UsageStore::open_in_memory().unwrap();
    let mut snapshot = UsageSample::empty("fixture", "{}", 1000);
    snapshot.billing_mode = crate::contract::BillingMode::Subscription;
    store.insert_snapshot(&snapshot).unwrap();
    let row = store.get_latest("fixture", None).unwrap().unwrap();
    assert_eq!(row.billing_mode, crate::contract::BillingMode::Subscription);
    assert!(!row.limits_reported);
    assert_eq!(row.spend_cents, None);
    snapshot.limits_reported = true;
    snapshot.weekly_pct = Some(0.0);
    snapshot.captured_at_ms = 2000;
    store.insert_snapshot(&snapshot).unwrap();
    let row = store.get_latest("fixture", None).unwrap().unwrap();
    assert!(row.limits_reported);
    assert_eq!(row.weekly_pct, Some(0.0));
}

#[test]
fn spend_fields_roundtrip() {
    let store = UsageStore::open_in_memory().unwrap();
    let mut s = UsageSample::empty("cursor", "raw", 1_000_000_000_000);
    s.spend_cents = Some(1743.5);
    s.spend_limit_cents = Some(2000.0);
    s.spend_remaining_cents = Some(256.5);
    s.spend_resets_at_ms = Some(1_785_542_400_000);
    let id = store.insert_snapshot(&s).unwrap();
    assert!(id > 0);
    let got = store.get_latest("cursor", None).unwrap().unwrap();
    assert_eq!(got.spend_cents, Some(1743.5));
    assert_eq!(got.spend_limit_cents, Some(2000.0));
    assert_eq!(got.spend_remaining_cents, Some(256.5));
    assert_eq!(got.spend_resets_at, Some(1_785_542_400_000));
    assert_eq!(got.session_pct, None);
}

#[test]
fn account_id_filter_isolation() {
    let store = UsageStore::open_in_memory().unwrap();
    let mut s1 = make_snapshot(1_000_000_000_000);
    s1.account_id = Some("acct-a".into());
    let mut s2 = make_snapshot(1_000_000_001_000);
    s2.account_id = Some("acct-b".into());
    store.insert_snapshot(&s1).unwrap();
    store.insert_snapshot(&s2).unwrap();
    let a = store.get_latest("claude", Some("acct-a")).unwrap().unwrap();
    assert_eq!(a.account_id.as_deref(), Some("acct-a"));
    assert!(store.get_latest(&s1.provider, None).unwrap().is_none());
    store
        .insert_snapshot(&make_snapshot(1_000_000_002_000))
        .unwrap();
    store
        .insert_failure_kind(
            &s1.provider,
            Some("acct-b"),
            None,
            UsageFailureKind::FetchError,
            "foreign failure",
            None,
        )
        .unwrap();
    let usual = store
        .get_latest_with_stale_reason(&s1.provider, None)
        .unwrap()
        .unwrap();
    assert!(usual.account_id.is_none());
    assert!(usual.failure_reason.is_none());
    let usual = store.get_history(&s1.provider, 0, 10, None).unwrap();
    assert_eq!(usual.len(), 1);
    assert!(usual.iter().all(|row| row.account_id.is_none()));
    let named = store
        .get_history(&s1.provider, 0, 10, Some("acct-a"))
        .unwrap();
    assert_eq!(named.len(), 1);
    assert!(named
        .iter()
        .all(|row| row.account_id.as_deref() == Some("acct-a")));
    assert_eq!(
        store
            .get_provider_history(&s1.provider, 0, 10)
            .unwrap()
            .len(),
        4
    );
    let absent = store
        .get_daily_rollup(&s1.provider, 0, Some("absent"))
        .unwrap();
    assert!(absent.is_empty());
}

#[test]
fn trim_to_cap_keeps_only_last_n() {
    // The trim logic is cap-agnostic, so a 50-row cap proves the same behavior.
    const CAP: i64 = 50;
    let store = UsageStore::open_in_memory_with_cap(CAP).unwrap();
    for i in 0..(CAP + 25) {
        store
            .insert_snapshot(&make_snapshot(1_000_000_000_000 + i * 1000))
            .unwrap();
    }
    let count: i64 = store.with_conn(|c| {
        c.query_row(
            "SELECT COUNT(*) FROM usage_snapshots WHERE provider='claude'",
            [],
            |r| r.get(0),
        )
        .unwrap()
    });
    assert_eq!(count, CAP, "rows beyond cap are pruned");
    let latest = store.get_latest("claude", None).unwrap().unwrap();
    let expected_latest = 1_000_000_000_000 + (CAP + 24) * 1000;
    assert_eq!(latest.captured_at, expected_latest, "newest row survives");
}

#[test]
fn prune_older_than_removes_old_rows() {
    let store = UsageStore::open_in_memory().unwrap();
    let s1 = make_snapshot(1_000_000_000_000);
    let s2 = make_snapshot(2_000_000_000_000);
    store.insert_snapshot(&s1).unwrap();
    store.insert_snapshot(&s2).unwrap();
    let n = store.prune_older_than(1_500_000_000_000).unwrap();
    assert_eq!(n, 1);
    let hist = store.get_history("claude", 0, 10, None).unwrap();
    assert_eq!(hist.len(), 1);
}
#[test]
fn validated_runtime_snapshots_accept_extremes_without_text_probe_heuristics() {
    let store = UsageStore::open_in_memory().unwrap();
    let previous = guarded_snapshot(1000, 20.0, 40.0, 5000, 9000);
    store.insert_snapshot(&previous).unwrap();
    let full = guarded_snapshot(2000, 100.0, 100.0, 9000, 9000);
    store.insert_runtime_snapshot(&full).unwrap();
    let zero = guarded_snapshot(3000, 0.0, 0.0, 4000, 8000);
    store.insert_runtime_snapshot(&zero).unwrap();
    let rows = store.get_history(&previous.provider, 0, 10, None).unwrap();
    assert_eq!(rows.len(), 3);
    assert!(rows
        .iter()
        .all(|row| row.parsed_ok && row.failure_reason.is_none()));
    assert_eq!(rows[0].session_pct, Some(0.0));
    assert_eq!(rows[1].session_pct, Some(100.0));
    assert_eq!(
        store
            .get_latest(&previous.provider, None)
            .unwrap()
            .unwrap()
            .captured_at,
        3000
    );
}

#[test]
fn runtime_snapshots_share_retention_with_probe_snapshots() {
    let store = UsageStore::open_in_memory_with_cap(2).unwrap();
    let sample = make_snapshot(1000);
    store.insert_snapshot(&sample).unwrap();
    store.insert_runtime_snapshot(&make_snapshot(2000)).unwrap();
    store.insert_runtime_snapshot(&make_snapshot(3000)).unwrap();
    let rows = store.get_history(&sample.provider, 0, 10, None).unwrap();
    assert_eq!(
        rows.iter().map(|row| row.captured_at).collect::<Vec<_>>(),
        vec![3000, 2000]
    );
}
