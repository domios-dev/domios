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
    let recording = Arc::new(std::sync::Mutex::new(
        crate::sinks::fake::Recording::default(),
    ));
    let sink = || {
        Box::new(crate::sinks::fake::Fake {
            recording: recording.clone(),
        }) as Box<dyn AnalyticsSink>
    };
    let worker = Worker::start_with_sink(path.clone(), Config::default(), sink()).unwrap();
    assert_eq!(worker.status().await.unwrap().consent, Consent::On);
    assert!(worker.status().await.unwrap().has_device_id);
    assert!(worker.record(Event::PlanViewed {}).await.unwrap());
    worker.flush().await.unwrap();
    assert!(!recording.lock().unwrap().batches.is_empty());
    worker.set_consent(Consent::Off).await.unwrap();
    recording.lock().unwrap().batches.clear();
    assert!(!worker.record(Event::PlanViewed {}).await.unwrap());
    worker.flush().await.unwrap();
    let status = worker.status().await.unwrap();
    assert_eq!(status.consent, Consent::Off);
    assert_eq!(status.queued, 0);
    assert!(!status.has_device_id && !status.has_user_id);
    let restored = Worker::start_with_sink(path, Config::default(), sink()).unwrap();
    assert_eq!(restored.status().await.unwrap().consent, Consent::Off);
    assert!(!restored.status().await.unwrap().has_device_id);
    assert!(!restored.record(Event::PlanViewed {}).await.unwrap());
    restored.flush().await.unwrap();
    assert!(recording.lock().unwrap().batches.is_empty());
}

#[tokio::test]
async fn local_off_waits_for_worker_queue_capacity() {
    let dir = tempfile::tempdir().unwrap();
    let mut engine = Engine::open(dir.path().join("settings.json"), Config::default()).unwrap();
    let (sender, receiver) = mpsc::sync_channel(1);
    let worker = Worker {
        sender,
        activity: Arc::new(crate::activity::Activity::new(Consent::On, None)),
    };
    worker.record_nowait(Event::PlanViewed {}).unwrap();
    let drain = async move {
        tokio::task::spawn_blocking(move || {
            assert!(matches!(receiver.recv().unwrap(), Command::Event(..)));
            let Command::Consent(consent, reply) = receiver.recv().unwrap() else {
                panic!("expected consent command")
            };
            assert_eq!(consent, Consent::Off);
            assert!(reply.send(engine.consent(consent)).is_ok());
        })
        .await
        .unwrap();
    };
    let (status, ()) = tokio::time::timeout(Duration::from_secs(1), async {
        tokio::join!(worker.set_consent(Consent::Off), drain)
    })
    .await
    .unwrap();
    assert_eq!(status.unwrap().consent, Consent::Off);
}
