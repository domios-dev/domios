use super::*;

#[test]
fn activity_full_queue_releases_claim_for_next_activity() {
    let (sender, _receiver) = mpsc::sync_channel(1);
    let worker = Worker {
        sender,
        activity: Arc::new(crate::activity::Activity::new(Consent::On, None)),
    };
    worker.record_nowait(Event::AnalyticsOptedIn {}).unwrap();
    let day = chrono::NaiveDate::from_ymd_opt(2026, 10, 13).unwrap();
    assert!(worker.record_activity_on_day(day, || 1).is_err());
    assert!(worker.activity.claim(day));
}
