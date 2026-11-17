use super::*;
use crate::{UsageSample, UsageStore};

fn sample(
    at: i64,
    session: Option<i64>,
    weekly: Option<i64>,
    account: Option<&str>,
) -> UsageSample {
    let mut sample = UsageSample::empty("fixture", "", at);
    sample.account_id = account.map(str::to_owned);
    sample.session_pct = Some(20.0);
    sample.weekly_pct = Some(40.0);
    sample.session_resets_at_ms = session;
    sample.weekly_resets_at_ms = weekly;
    sample
}

fn projected(store: &UsageStore, account: Option<&str>, now: i64) -> UsageSnapshotRow {
    store.latest_at("fixture", account, now).unwrap().unwrap()
}

#[test]
fn partial_samples_preserve_resets_in_the_account_projection() {
    let store = UsageStore::open_in_memory().unwrap();
    store
        .insert_runtime_snapshot(&sample(100, Some(500), Some(900), Some("own")))
        .unwrap();
    store
        .insert_runtime_snapshot(&sample(200, None, None, Some("own")))
        .unwrap();
    let row = projected(&store, Some("own"), 200);
    assert_eq!(
        (row.session_resets_at, row.weekly_resets_at),
        (Some(500), Some(900))
    );
    store
        .insert_runtime_snapshot(&sample(300, Some(600), None, Some("own")))
        .unwrap();
    let row = projected(&store, Some("own"), 300);
    assert_eq!(
        (row.session_resets_at, row.weekly_resets_at),
        (Some(600), Some(900))
    );
    store
        .insert_runtime_snapshot(&sample(250, Some(700), None, Some("own")))
        .unwrap();
    assert_eq!(
        projected(&store, Some("own"), 300).session_resets_at,
        Some(600)
    );
    assert_eq!(projected(&store, Some("own"), 600).session_resets_at, None);
}

#[test]
fn expired_newer_reset_does_not_resurrect_an_older_reset() {
    let store = UsageStore::open_in_memory().unwrap();
    store
        .insert_runtime_snapshot(&sample(100, Some(900), None, None))
        .unwrap();
    store
        .insert_runtime_snapshot(&sample(200, Some(300), None, None))
        .unwrap();
    store
        .insert_runtime_snapshot(&sample(250, None, None, None))
        .unwrap();
    assert_eq!(projected(&store, None, 300).session_resets_at, None);
}

#[test]
fn resets_never_cross_provider_or_account_boundaries() {
    let store = UsageStore::open_in_memory().unwrap();
    store
        .insert_runtime_snapshot(&sample(100, Some(900), Some(1000), Some("foreign")))
        .unwrap();
    let mut foreign_provider = sample(150, Some(800), Some(900), None);
    foreign_provider.provider = "other".into();
    store.insert_runtime_snapshot(&foreign_provider).unwrap();
    store
        .insert_runtime_snapshot(&sample(200, None, None, None))
        .unwrap();
    store
        .insert_runtime_snapshot(&sample(250, None, None, Some("own")))
        .unwrap();
    for account in [None, Some("own")] {
        let row = projected(&store, account, 300);
        assert_eq!(row.account_id.as_deref(), account);
        assert_eq!((row.session_resets_at, row.weekly_resets_at), (None, None));
    }
}
