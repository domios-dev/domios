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

#[tokio::test]
async fn new_install_defaults_on_and_off_remains_off_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let worker = Worker::start_with_sink(
        path.clone(),
        Config::default(),
        Box::new(crate::sinks::off::Off),
    )
    .unwrap();
    assert_eq!(worker.status().await.unwrap().consent, Consent::On);
    assert!(worker.device_id().await.unwrap().is_some());
    let ticket = worker.begin_consent(Some(Consent::Off)).await.unwrap();
    assert_eq!(ticket.consent, Consent::Off);
    let restored =
        Worker::start_with_sink(path, Config::default(), Box::new(crate::sinks::off::Off)).unwrap();
    assert_eq!(restored.status().await.unwrap().consent, Consent::Off);
    assert!(restored.device_id().await.unwrap().is_none());
}
