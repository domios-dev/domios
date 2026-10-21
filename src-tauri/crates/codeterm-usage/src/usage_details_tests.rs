use crate::{UsageSample, UsageStore};

#[test]
fn usage_details_roundtrip() {
    let store = UsageStore::open_in_memory().unwrap();
    let details = serde_json::json!({
        "limits": [{"label":"Weekly · Fable","percent":47,"critical":true,"is_active":true}],
        "dollar_credits": [{"remaining_dollars":250,"resets_at":1793836800000_i64}],
        "extra_usage": {"enabled":false,"settings_url":"https://claude.ai/settings/usage"}
    });
    let mut sample = UsageSample::empty("claude", "opaque", 1_000_000_000_000);
    sample.session_pct = Some(17.0);
    sample.usage_details = Some(details.clone());
    store.insert_snapshot(&sample).unwrap();
    let latest = store.get_latest("claude", None).unwrap().unwrap();
    assert_eq!(latest.usage_details, Some(details));
    let mut absent = UsageSample::empty("claude", "opaque", 1_000_000_000_001);
    absent.session_pct = Some(17.0);
    store.insert_snapshot(&absent).unwrap();
    assert!(store
        .get_latest("claude", None)
        .unwrap()
        .unwrap()
        .usage_details
        .is_none());
}
