use super::*;
use std::sync::{Arc, Mutex};

fn recording_engine(
    path: std::path::PathBuf,
    recording: Arc<Mutex<sinks::fake::Recording>>,
) -> Engine {
    let mut engine = Engine::open(path, config()).unwrap();
    engine.sink = Box::new(sinks::fake::Fake { recording });
    engine
}

fn startup() -> Event {
    Event::AppStarted {
        version: Version::new(1, 15, 2),
        platform: Platform::Windows,
        mesh_machines: 1,
        workspaces: 1,
    }
}

#[test]
fn consent_gate_covers_lifecycle_identity_and_pending_delivery() {
    let dir = tempfile::tempdir().unwrap();
    let recording = Arc::new(Mutex::new(sinks::fake::Recording::default()));
    let mut engine = recording_engine(dir.path().join("settings.json"), recording.clone());
    assert!(!engine.enqueue(startup(), 0).unwrap());
    engine.identity(Some("firebase-uid".into())).unwrap();
    engine.deliver(0).unwrap();
    assert!(!engine.state.first_run_recorded);
    assert!(engine.state.device_id.is_none());
    engine.consent(Consent::On).unwrap();
    engine.enqueue(startup(), 0).unwrap();
    engine.consent(Consent::Off).unwrap();
    engine.deliver(0).unwrap();
    assert!(recording.lock().unwrap().batches.is_empty());
    assert!(engine.state.queue.is_empty());
}

#[test]
fn anonymous_events_link_to_exact_uid_with_stable_device_across_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let recording = Arc::new(Mutex::new(sinks::fake::Recording::default()));
    let mut engine = recording_engine(path.clone(), recording.clone());
    engine.consent(Consent::On).unwrap();
    engine.enqueue(Event::PlanViewed {}, 0).unwrap();
    engine.deliver(0).unwrap();
    let device = engine.state.device_id.unwrap();
    drop(engine);
    let mut engine = recording_engine(path, recording.clone());
    engine.identity(Some("exact-Firebase_uid".into())).unwrap();
    engine
        .enqueue(
            Event::SignedIn {
                auth_provider: AuthProvider::Firebase,
            },
            1,
        )
        .unwrap();
    engine.deliver(1).unwrap();
    let recording = recording.lock().unwrap();
    let events: Vec<_> = recording.batches.iter().flatten().collect();
    assert_eq!(events.len(), 2);
    assert!(events[0].user_id.is_none());
    assert_eq!(events[1].user_id.as_deref(), Some("exact-Firebase_uid"));
    assert!(events.iter().all(|event| event.device_id == device));
}

#[test]
fn lifecycle_first_run_is_persisted_and_contains_only_version_and_platform() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let recording = Arc::new(Mutex::new(sinks::fake::Recording::default()));
    let mut engine = recording_engine(path.clone(), recording.clone());
    engine.consent(Consent::On).unwrap();
    engine.enqueue(startup(), 0).unwrap();
    engine.deliver(0).unwrap();
    drop(engine);
    let mut engine = recording_engine(path, recording.clone());
    engine.enqueue(startup(), 1).unwrap();
    engine.deliver(1).unwrap();
    let recording = recording.lock().unwrap();
    let events: Vec<_> = recording.batches.iter().flatten().collect();
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e.event, Event::AppFirstRun { .. }))
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e.event, Event::AppOpened {}))
            .count(),
        2
    );
    assert_eq!(
        serde_json::to_value(&events[0].event).unwrap(),
        serde_json::json!({
            "event_type": "app_first_run", "event_properties": {"version": "1.15.2", "platform": "windows"}
        })
    );
}

#[test]
fn queue_and_delivery_batches_remain_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let recording = Arc::new(Mutex::new(sinks::fake::Recording::default()));
    let mut engine = recording_engine(dir.path().join("settings.json"), recording.clone());
    engine.consent(Consent::On).unwrap();
    engine.enqueue(Event::PlanViewed {}, 0).unwrap();
    let queued = engine.state.queue[0].clone();
    engine.state.queue.resize(QUEUE_CAP, queued);
    for _ in 0..3 {
        engine.enqueue(Event::PlanViewed {}, 0).unwrap();
    }
    assert_eq!(engine.state.queue.len(), QUEUE_CAP);
    assert_eq!(engine.state.counters.dropped_oldest, 3);
    engine.deliver(0).unwrap();
    let recording = recording.lock().unwrap();
    assert!(recording
        .batches
        .iter()
        .all(|batch| !batch.is_empty() && batch.len() <= BATCH_CAP));
    assert_eq!(recording.batches.len(), 1);
    assert_eq!(recording.batches[0].len(), BATCH_CAP);
    assert_eq!(engine.state.queue.len(), QUEUE_CAP - BATCH_CAP);
}

#[test]
fn catalog_rejects_forbidden_fields_on_product_events() {
    for event_type in [
        "app_first_run",
        "app_opened",
        "plan_viewed",
        "signed_in",
        "checkout_started",
    ] {
        let fields = match event_type {
            "app_first_run" => serde_json::json!({"version":"1.15.2","platform":"windows"}),
            "signed_in" => serde_json::json!({"auth_provider":"firebase"}),
            "checkout_started" => serde_json::json!({"plan":"pro"}),
            _ => serde_json::json!({}),
        };
        for field in [
            "path",
            "prompt",
            "tab_content",
            "email",
            "agent_duration",
            "tokens",
            "model",
        ] {
            let mut properties = fields.clone();
            properties[field] = serde_json::json!("private");
            assert!(serde_json::from_value::<Event>(serde_json::json!({
                "event_type":event_type, "event_properties":properties
            }))
            .is_err());
        }
    }
}

#[tokio::test]
async fn enabling_after_startup_records_lifecycle_under_signed_in_uid() {
    let dir = tempfile::tempdir().unwrap();
    let recording = Arc::new(Mutex::new(sinks::fake::Recording::default()));
    let worker = Worker::start_with_sink(
        dir.path().join("settings.json"),
        config(),
        Box::new(sinks::fake::Fake {
            recording: recording.clone(),
        }),
    )
    .unwrap();
    assert!(!worker.record(startup()).await.unwrap());
    worker
        .set_user_id(Some("firebase-uid".into()))
        .await
        .unwrap();
    worker.set_consent(Consent::On).await.unwrap();
    worker.flush().await.unwrap();
    let recording = recording.lock().unwrap();
    let events: Vec<_> = recording.batches.iter().flatten().collect();
    assert_eq!(events.len(), 4);
    assert!(events
        .iter()
        .all(|e| e.user_id.as_deref() == Some("firebase-uid")));
    assert!(events
        .iter()
        .any(|e| matches!(e.event, Event::AppFirstRun { .. })));
}
