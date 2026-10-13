use super::*;

#[test]
fn history_query_preserves_account_and_default_limit() {
    let query = Query::<UsageHistoryQuery>::try_from_uri(
        &"/api/usage/history?provider=fixture&since=1000&account_id=account-a"
            .parse()
            .unwrap(),
    )
    .unwrap()
    .0;
    assert_eq!(query.provider, "fixture");
    assert_eq!(query.since, 1000);
    assert_eq!(query.limit, 200);
    assert_eq!(query.account_id.as_deref(), Some("account-a"));
    assert!(Query::<UsageHistoryQuery>::try_from_uri(
        &"/api/usage/history?since=1000".parse().unwrap()
    )
    .is_err());
}

#[test]
fn persisted_usage_row_keeps_the_api_wire_contract() {
    let data_dir = tempfile::tempdir().unwrap();
    let store = crate::usage::UsageStore::open(data_dir.path()).unwrap();
    let sample = codeterm_usage::UsageSample {
        provider: "fixture".into(),
        billing_mode: codeterm_types::usage::BillingMode::Subscription,
        limits_reported: false,
        account_id: Some("account-a".into()),
        captured_at_ms: 1000,
        session_pct: Some(12.5),
        session_resets_at_ms: Some(2000),
        weekly_pct: None,
        weekly_resets_at_ms: None,
        spend_cents: None,
        spend_limit_cents: None,
        spend_remaining_cents: None,
        spend_resets_at_ms: None,
        raw_text: "fixture".into(),
    };
    store.insert_snapshot(&sample).unwrap();
    drop(store);
    let store = crate::usage::UsageStore::open(data_dir.path()).unwrap();
    let query = Query::<UsageQuery>::try_from_uri(
        &"/api/usage/latest?provider=fixture&account_id=account-a"
            .parse()
            .unwrap(),
    )
    .unwrap()
    .0;
    let row: Option<codeterm_types::usage::UsageSnapshotRow> = store
        .get_latest_with_stale_reason(&query.provider, query.account_id.as_deref())
        .unwrap();
    let value = serde_json::to_value(row).unwrap();
    assert_eq!(value["provider"], "fixture");
    assert_eq!(value["account_id"], "account-a");
    assert_eq!(value["captured_at"], 1000);
    assert_eq!(value["parsed_ok"], true);
    assert_eq!(value["session_pct"], 12.5);
    assert_eq!(value["session_resets_at"], 2000);
    assert_eq!(value["billing_mode"], "subscription");
    assert_eq!(value["limits_reported"], false);
    assert!(value["weekly_pct"].is_null());
    assert!(value["spend_cents"].is_null());
    assert!(store
        .get_latest_with_stale_reason("fixture", Some("account-b"))
        .unwrap()
        .is_none());
}
