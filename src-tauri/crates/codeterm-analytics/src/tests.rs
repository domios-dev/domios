use super::*;
use crate::engine::Engine;
trait TestDestination {
    fn destination(&mut self, endpoint: String);
}
impl TestDestination for Engine {
    fn destination(&mut self, endpoint: String) {
        let mut sink = sinks::amplitude::Http::new(
            self.state.config,
            std::sync::Arc::new(|_| Ok("test-key".into())),
        )
        .unwrap();
        sink.destination(endpoint);
        self.sink = Box::new(sink);
    }
}

fn event() -> Event {
    Event::WorkspaceCreated { workspaces: 2 }
}
fn config() -> Config {
    Config {
        mode: Mode::Amplitude,
        ..Config::default()
    }
}
fn engine(dir: &tempfile::TempDir, server: &mockito::ServerGuard) -> Engine {
    let mut engine = Engine::open(dir.path().join("analytics-settings.json"), config()).unwrap();
    engine.destination(format!("{}/2/httpapi", server.url()));
    engine
}

#[test]
fn consent_off_never_queues_or_sends() {
    let mut server = mockito::Server::new();
    let destination = server.mock("POST", "/2/httpapi").expect(0).create();
    let dir = tempfile::tempdir().unwrap();
    let mut engine = engine(&dir, &server);
    assert!(!engine.enqueue(event(), 0).unwrap());
    engine.deliver(0).unwrap();
    assert_eq!(engine.status().queued, 0);
    assert!(!engine.status().has_device_id);
    destination.assert();
}

#[test]
fn opt_out_clears_persisted_queue_and_ids() {
    let mut server = mockito::Server::new();
    let destination = server.mock("POST", "/2/httpapi").expect(0).create();
    let dir = tempfile::tempdir().unwrap();
    let mut engine = engine(&dir, &server);
    engine.consent(Consent::On).unwrap();
    let first_id = engine.state.device_id;
    engine.enqueue(event(), 0).unwrap();
    engine.consent(Consent::Off).unwrap();
    engine.deliver(0).unwrap();
    let mut reopened =
        engine::Engine::open(dir.path().join("analytics-settings.json"), config()).unwrap();
    assert_eq!(reopened.status().consent, Consent::Off);
    assert_eq!(reopened.status().queued, 0);
    assert!(!reopened.status().has_device_id);
    reopened.consent(Consent::On).unwrap();
    assert_ne!(reopened.state.device_id, first_id);
    destination.assert();
}

#[test]
fn catalog_rejects_unknown_events_properties_and_strings() {
    for raw in [
        r#"{"event_type":"prompt","event_properties":{}}"#,
        r#"{"event_type":"settings_opened","event_properties":{"section":"privacy","path":"private"}}"#,
        r#"{"event_type":"settings_opened","event_properties":{"section":"hostname"}}"#,
        r#"{"event_type":"app_started","event_properties":{"version":"1.2.3-private"}}"#,
        r#"{"event_type":"settings_opened","event_properties":{"section":"privacy"},"user_id":"private"}"#,
        r#"{"event_type":"session_completed","event_properties":{"outcome":"completed","duration_ms":-1,"message_count":0}}"#,
    ] {
        assert!(
            serde_json::from_str::<Event>(raw).is_err(),
            "accepted {raw}"
        );
    }
    let accepted = Event::AppStarted {
        version: Version::new(1, 2, 3),
        platform: Platform::Macos,
        mesh_machines: 1,
        workspaces: 2,
    };
    assert_eq!(
        serde_json::from_value::<Event>(serde_json::to_value(&accepted).unwrap()).unwrap(),
        accepted
    );
}

#[test]
fn batches_carry_catalog_properties_and_account_linkage() {
    let mut server = mockito::Server::new();
    let dir = tempfile::tempdir().unwrap();
    let mut engine = engine(&dir, &server);
    engine.consent(Consent::On).unwrap();
    engine.enqueue(event(), 100).unwrap();
    engine
        .enqueue(Event::MeshPeerPaired { mesh_machines: 3 }, 100)
        .unwrap();
    let destination = server.mock("POST", "/2/httpapi")
        .match_header("content-type", "application/json")
        .match_body(mockito::Matcher::PartialJson(serde_json::json!({"api_key":"test-key","events":[
            {"event_type":"workspace_created","event_properties":{"workspaces":2},"insert_id":engine.state.queue[0].insert_id,"device_id":engine.state.device_id,"time":100},
            {"event_type":"mesh_peer_paired","event_properties":{"mesh_machines":3},"insert_id":engine.state.queue[1].insert_id,"device_id":engine.state.device_id,"time":100}
        ]}))).with_status(200).expect(1).create();
    engine.deliver(100).unwrap();
    engine.deliver(100).unwrap();
    assert_eq!(engine.status().queued, 0);
    assert_eq!(engine.status().counters.delivered, 2);
    destination.assert();
}

#[test]
fn retries_429_and_5xx_then_delivers_once() {
    for status in [429, 500, 503] {
        let mut server = mockito::Server::new();
        let dir = tempfile::tempdir().unwrap();
        let mut engine = engine(&dir, &server);
        engine.consent(Consent::On).unwrap();
        engine.enqueue(event(), 0).unwrap();
        let insert_id = engine.state.queue[0].insert_id;
        let body =
            mockito::Matcher::PartialJson(serde_json::json!({"events":[{"insert_id":insert_id}]}));
        let failed = server
            .mock("POST", "/2/httpapi")
            .match_body(body.clone())
            .with_status(status)
            .expect(1)
            .create();
        engine.deliver(0).unwrap();
        engine.deliver(29_999).unwrap();
        failed.assert();
        failed.remove();
        let delivered = server
            .mock("POST", "/2/httpapi")
            .match_body(body)
            .with_status(200)
            .expect(1)
            .create();
        engine.deliver(30_000).unwrap();
        engine.deliver(30_000).unwrap();
        assert_eq!(engine.status().counters.delivered, 1);
        delivered.assert();
    }
}

#[test]
fn retry_schedule_is_bounded_and_persisted() {
    let mut server = mockito::Server::new();
    let destination = server
        .mock("POST", "/2/httpapi")
        .with_status(503)
        .expect(6)
        .create();
    let dir = tempfile::tempdir().unwrap();
    let mut engine = engine(&dir, &server);
    engine.consent(Consent::On).unwrap();
    engine.enqueue(event(), 0).unwrap();
    let mut now = 0;
    for delay in RETRY_DELAYS_MS {
        engine.deliver(now).unwrap();
        assert_eq!(engine.deadline(), Some(now + delay));
        now += delay;
    }
    drop(engine);
    let mut engine =
        engine::Engine::open(dir.path().join("analytics-settings.json"), config()).unwrap();
    engine.destination(format!("{}/2/httpapi", server.url()));
    engine.deliver(now).unwrap();
    assert_eq!(engine.status().queued, 0);
    assert_eq!(engine.status().counters.retry_exhausted, 1);
    destination.assert();
}

#[test]
fn payload_400_drops_and_counts() {
    let mut server = mockito::Server::new();
    let destination = server
        .mock("POST", "/2/httpapi")
        .with_status(400)
        .expect(1)
        .create();
    let dir = tempfile::tempdir().unwrap();
    let mut engine = engine(&dir, &server);
    engine.consent(Consent::On).unwrap();
    engine.enqueue(event(), 0).unwrap();
    engine.deliver(0).unwrap();
    engine.deliver(30_000).unwrap();
    assert_eq!(engine.status().queued, 0);
    assert_eq!(engine.status().counters.dropped_payload, 1);
    destination.assert();
}

#[test]
fn queue_cap_drops_oldest_and_counts() {
    let server = mockito::Server::new();
    let dir = tempfile::tempdir().unwrap();
    let mut engine = engine(&dir, &server);
    engine.consent(Consent::On).unwrap();
    for index in 0..QUEUE_CAP {
        engine.state.queue.push_back(crate::state::Queued {
            insert_id: uuid::Uuid::new_v4(),
            time: index as u64,
            event: event(),
            failures: 0,
            retry_at: 0,
        });
    }
    engine.enqueue(event(), QUEUE_CAP as u64).unwrap();
    assert_eq!(engine.state.queue.len(), QUEUE_CAP);
    assert_eq!(engine.state.queue.front().unwrap().time, 1);
    assert_eq!(engine.status().counters.dropped_oldest, 1);
}

#[test]
fn persisted_queue_survives_restart() {
    let mut server = mockito::Server::new();
    let dir = tempfile::tempdir().unwrap();
    let mut original = engine(&dir, &server);
    original.consent(Consent::On).unwrap();
    original.enqueue(event(), 0).unwrap();
    let id = original.state.queue[0].insert_id;
    let device = original.state.device_id;
    drop(original);
    let mut restarted = engine(&dir, &server);
    assert_eq!(restarted.state.queue[0].insert_id, id);
    assert_eq!(restarted.state.device_id, device);
    let delivered = server
        .mock("POST", "/2/httpapi")
        .with_status(200)
        .expect(1)
        .create();
    restarted.deliver(0).unwrap();
    assert_eq!(restarted.status().queued, 0);
    delivered.assert();
}

#[test]
fn config_is_closed_and_projects_never_mix() {
    assert_eq!(Config::parse(None, None, None).unwrap().mode, Mode::Off);
    assert!(Config::parse(Some("custom"), None, None).is_err());
    assert!(Config::parse(None, Some("private-host"), None).is_err());
    assert!(Config::parse(None, None, Some("private-project")).is_err());
    assert_eq!(
        Zone::Eu.endpoint(),
        "https://api.eu.amplitude.com/2/httpapi"
    );
    let server = mockito::Server::new();
    let dir = tempfile::tempdir().unwrap();
    let mut engine = engine(&dir, &server);
    engine.consent(Consent::On).unwrap();
    engine.enqueue(event(), 0).unwrap();
    drop(engine);
    let switched = Engine::open(
        dir.path().join("analytics-settings.json"),
        Config {
            project: Project::Prod,
            ..config()
        },
    )
    .unwrap();
    assert_eq!(switched.status().queued, 0);
    assert_eq!(switched.status().counters.dropped_config, 1);
}

#[test]
fn fake_sink_shares_consent_and_retry_policy() {
    use std::sync::{Arc, Mutex};
    let dir = tempfile::tempdir().unwrap();
    let recording = Arc::new(Mutex::new(sinks::fake::Recording::default()));
    recording
        .lock()
        .unwrap()
        .results
        .extend([DeliveryResult::Retry, DeliveryResult::Delivered]);
    let mut engine = Engine::open(dir.path().join("analytics-settings.json"), config()).unwrap();
    engine.sink = Box::new(sinks::fake::Fake {
        recording: recording.clone(),
    });
    assert!(!engine.enqueue(event(), 0).unwrap());
    engine.deliver(0).unwrap();
    assert!(recording.lock().unwrap().batches.is_empty());
    engine.consent(Consent::On).unwrap();
    engine.enqueue(event(), 0).unwrap();
    engine.deliver(0).unwrap();
    engine.deliver(29_999).unwrap();
    assert_eq!(recording.lock().unwrap().batches.len(), 1);
    engine.deliver(30_000).unwrap();
    let recording = recording.lock().unwrap();
    assert_eq!(recording.batches.len(), 2);
    assert_eq!(
        recording.batches[0][0].insert_id,
        recording.batches[1][0].insert_id
    );
    assert_eq!(engine.status().queued, 0);
}

#[test]
fn off_sink_and_missing_credentials_send_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut engine = Engine::open(
        dir.path().join("analytics-settings.json"),
        Config::default(),
    )
    .unwrap();
    engine.consent(Consent::On).unwrap();
    assert!(!engine.enqueue(event(), 0).unwrap());
    let mut engine = Engine::open(dir.path().join("analytics-settings.json"), config()).unwrap();
    engine.enqueue(event(), 0).unwrap();
    assert!(engine.deliver(0).is_err());
    assert_eq!(engine.status().queued, 1);
    assert_eq!(engine.state.queue[0].failures, 0);
}

#[test]
fn failed_opt_out_persistence_still_gates_delivery() {
    let mut server = mockito::Server::new();
    let destination = server.mock("POST", "/2/httpapi").expect(0).create();
    let dir = tempfile::tempdir().unwrap();
    let mut engine = engine(&dir, &server);
    engine.consent(Consent::On).unwrap();
    engine.enqueue(event(), 0).unwrap();
    std::fs::remove_file(dir.path().join("analytics-settings.json")).unwrap();
    std::fs::create_dir(dir.path().join("analytics-settings.json")).unwrap();
    assert!(engine.consent(Consent::Off).is_err());
    engine.deliver(0).unwrap();
    assert_eq!(engine.status().consent, Consent::Off);
    destination.assert();
}

#[test]
fn catalog_every_event_rejects_extra_properties_and_unknown_enum_values() {
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../catalog.schema.json")).unwrap();
    let enums = schema["enums"].as_object().unwrap();
    for (name, fields) in schema["events"].as_object().unwrap() {
        let mut properties = serde_json::Map::new();
        for (field, kind) in fields.as_object().unwrap() {
            let kind = kind.as_str().unwrap();
            let value = match kind {
                "Version" => serde_json::json!("1.2.3"),
                "u32" => serde_json::json!(1),
                "ProviderId" => serde_json::json!("codex"),
                _ => enums[kind][0].clone(),
            };
            properties.insert(field.clone(), value);
        }
        let payload = serde_json::json!({"event_type":name,"event_properties":properties});
        let event = serde_json::from_value::<Event>(payload.clone()).unwrap();
        assert_eq!(serde_json::to_value(event).unwrap(), payload);
        let mut extra = payload.clone();
        extra["event_properties"]["private"] = serde_json::json!("private");
        assert!(
            serde_json::from_value::<Event>(extra).is_err(),
            "extra field accepted by {name}"
        );
        for (field, _) in fields.as_object().unwrap() {
            let mut invalid = payload.clone();
            invalid["event_properties"][field] = serde_json::json!("private");
            assert!(
                serde_json::from_value::<Event>(invalid).is_err(),
                "untyped field accepted by {name}.{field}"
            );
        }
    }
}

#[test]
fn daily_active_day_survives_restart_and_emits_again_next_day() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("analytics-settings.json");
    let recording = std::sync::Arc::new(std::sync::Mutex::new(sinks::fake::Recording::default()));
    let mut engine = Engine::open(path.clone(), config()).unwrap();
    engine.sink = Box::new(sinks::fake::Fake {
        recording: recording.clone(),
    });
    engine.consent(Consent::On).unwrap();
    let day = chrono::NaiveDate::from_ymd_opt(2026, 10, 13).unwrap();
    let activity = Event::ActiveDay {
        mesh_machines: 2,
        workspaces: 3,
    };
    assert!(engine.enqueue_on_day(activity.clone(), 0, day).unwrap());
    assert!(!engine.enqueue_on_day(activity.clone(), 0, day).unwrap());
    engine.deliver(0).unwrap();
    drop(engine);
    let mut engine = Engine::open(path, config()).unwrap();
    engine.sink = Box::new(sinks::fake::Fake {
        recording: recording.clone(),
    });
    assert!(!engine.enqueue_on_day(activity.clone(), 0, day).unwrap());
    assert!(engine
        .enqueue_on_day(activity, 0, day.succ_opt().unwrap())
        .unwrap());
    engine.deliver(0).unwrap();
    assert_eq!(
        recording.lock().unwrap().batches.iter().flatten().count(),
        2
    );
}
#[test]
fn daily_feature_used_deduplicates_each_feature_and_persists() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("analytics-settings.json");
    let recording = std::sync::Arc::new(std::sync::Mutex::new(sinks::fake::Recording::default()));
    let mut engine = Engine::open(path.clone(), config()).unwrap();
    engine.sink = Box::new(sinks::fake::Fake {
        recording: recording.clone(),
    });
    engine.consent(Consent::On).unwrap();
    let day = chrono::NaiveDate::from_ymd_opt(2026, 10, 13).unwrap();
    let feature = Event::FeatureUsed {
        feature: Feature::MemorySaved,
    };
    assert!(engine.enqueue_on_day(feature.clone(), 0, day).unwrap());
    assert!(!engine.enqueue_on_day(feature.clone(), 0, day).unwrap());
    assert!(engine
        .enqueue_on_day(
            Event::FeatureUsed {
                feature: Feature::ThemeChanged
            },
            0,
            day
        )
        .unwrap());
    engine.deliver(0).unwrap();
    drop(engine);
    let mut engine = Engine::open(path, config()).unwrap();
    engine.sink = Box::new(sinks::fake::Fake {
        recording: recording.clone(),
    });
    assert!(!engine.enqueue_on_day(feature.clone(), 0, day).unwrap());
    assert!(engine
        .enqueue_on_day(feature, 0, day.succ_opt().unwrap())
        .unwrap());
    engine.deliver(0).unwrap();
    assert_eq!(
        recording.lock().unwrap().batches.iter().flatten().count(),
        3
    );
}
#[test]
fn daily_events_with_consent_off_never_emit_or_save_days() {
    let dir = tempfile::tempdir().unwrap();
    let recording = std::sync::Arc::new(std::sync::Mutex::new(sinks::fake::Recording::default()));
    let mut engine = Engine::open(dir.path().join("analytics-settings.json"), config()).unwrap();
    engine.sink = Box::new(sinks::fake::Fake {
        recording: recording.clone(),
    });
    let day = chrono::NaiveDate::from_ymd_opt(2026, 10, 13).unwrap();
    for event in [
        Event::ActiveDay {
            mesh_machines: 1,
            workspaces: 1,
        },
        Event::FeatureUsed {
            feature: Feature::MemorySaved,
        },
    ] {
        assert!(!engine.enqueue_on_day(event, 0, day).unwrap());
    }
    engine.deliver(0).unwrap();
    assert!(recording.lock().unwrap().batches.is_empty());
    assert!(engine.state.last_active_day.is_none());
    assert!(engine.state.feature_days.is_empty());
}
#[test]
fn account_linkage_is_exact_then_cleared_on_sign_out() {
    let dir = tempfile::tempdir().unwrap();
    let recording = std::sync::Arc::new(std::sync::Mutex::new(sinks::fake::Recording::default()));
    let mut engine = Engine::open(dir.path().join("analytics-settings.json"), config()).unwrap();
    engine.sink = Box::new(sinks::fake::Fake {
        recording: recording.clone(),
    });
    engine.consent(Consent::On).unwrap();
    engine.identity(Some("firebase-opaque-uid".into())).unwrap();
    engine.enqueue(event(), 0).unwrap();
    engine.identity(None).unwrap();
    engine.enqueue(event(), 0).unwrap();
    engine.deliver(0).unwrap();
    let recording = recording.lock().unwrap();
    let events: Vec<_> = recording.batches.iter().flatten().collect();
    assert_eq!(events[0].user_id.as_deref(), Some("firebase-opaque-uid"));
    assert!(events[1].user_id.is_none());
    assert_eq!(events[0].device_id, events[1].device_id);
}

#[tokio::test]
async fn catalog_worker_identity_follows_sign_in_opt_in_and_sign_out() {
    use std::sync::{Arc, Mutex};
    let dir = tempfile::tempdir().unwrap();
    let recording = Arc::new(Mutex::new(sinks::fake::Recording::default()));
    let worker = Worker::start_with_sink(
        dir.path().join("analytics-settings.json"),
        config(),
        Box::new(sinks::fake::Fake {
            recording: recording.clone(),
        }),
    )
    .unwrap();
    worker
        .set_user_id(Some("account-uid".into()))
        .await
        .unwrap();
    assert!(!worker.status().await.unwrap().has_user_id);
    assert!(!worker.record(event()).await.unwrap());
    worker.flush().await.unwrap();
    assert!(recording.lock().unwrap().batches.is_empty());
    worker.set_consent(Consent::On).await.unwrap();
    worker.record(event()).await.unwrap();
    worker.flush().await.unwrap();
    worker.set_user_id(None).await.unwrap();
    worker.record(Event::PlanCreated {}).await.unwrap();
    worker.flush().await.unwrap();
    let recording = recording.lock().unwrap();
    let events: Vec<_> = recording.batches.iter().flatten().collect();
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].event, Event::AnalyticsOptedIn {});
    assert_eq!(events[0].user_id.as_deref(), Some("account-uid"));
    assert_eq!(events[1].user_id.as_deref(), Some("account-uid"));
    assert_eq!(events[2].user_id, None);
}
