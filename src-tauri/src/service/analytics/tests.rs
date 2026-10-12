use super::*;

#[test]
fn product_agent_spawned_contains_only_catalog_categories() {
    let events = tab_events(Some("codex"), Some("Worker"), Some("gpt-6.1-sol"));
    assert_eq!(events.len(), 2);
    assert_eq!(
        events[0],
        Event::TabOpened {
            kind: TabKind::Agent,
            provider_id: Some(ProviderId::Codex)
        }
    );
    assert_eq!(
        events[1],
        Event::AgentSpawned {
            provider_id: ProviderId::Codex,
            role: Role::Worker,
            model_family: ModelFamily::Gpt61Sol
        }
    );
    let unknown = tab_events(
        Some("private-provider"),
        Some("private-role"),
        Some("private-model"),
    );
    assert_eq!(
        unknown[1],
        Event::AgentSpawned {
            provider_id: ProviderId::Other,
            role: Role::Other,
            model_family: ModelFamily::Unknown
        }
    );
    let payload = serde_json::to_string(&unknown).unwrap();
    assert!(!payload.contains("private"));
}

#[test]
fn product_shell_opened_has_no_provider_or_spawn_event() {
    assert_eq!(
        tab_events(None, None, None),
        vec![Event::TabOpened {
            kind: TabKind::Shell,
            provider_id: None
        }]
    );
}

#[tokio::test]
async fn product_agent_spawned_reaches_fake_sink_only_after_consent() {
    use codeterm_analytics::{
        sinks::fake::{Fake, Recording},
        Consent,
    };
    use std::sync::{Arc, Mutex};
    let dir = tempfile::tempdir().unwrap();
    let recording = Arc::new(Mutex::new(Recording::default()));
    let worker = Worker::start_with_sink(
        dir.path().join("analytics-settings.json"),
        Config::default(),
        Box::new(Fake {
            recording: recording.clone(),
        }),
    )
    .unwrap();
    let events = tab_events(Some("codex"), Some("Worker"), Some("gpt-6.1-sol"));
    for event in &events {
        assert!(!worker.record(event.clone()).await.unwrap());
    }
    worker.flush().await.unwrap();
    assert!(recording.lock().unwrap().batches.is_empty());
    worker.set_consent(Consent::On).await.unwrap();
    for event in &events {
        assert!(worker.record(event.clone()).await.unwrap());
    }
    worker.flush().await.unwrap();
    let actual: Vec<_> = recording
        .lock()
        .unwrap()
        .batches
        .iter()
        .flatten()
        .map(|value| value.event.clone())
        .collect();
    assert_eq!(actual, [vec![Event::AnalyticsOptedIn {}], events].concat());
}

#[test]
fn product_app_started_strips_release_labels_and_rejects_arbitrary_values() {
    assert_eq!(
        app_started("1.2.3-rc.1+private", "macos", "rc"),
        Some(Event::AppStarted {
            version: Version::new(1, 2, 3),
            platform: Platform::Macos,
            update_channel: UpdateChannel::Rc
        })
    );
    assert!(app_started("private", "macos", "stable").is_none());
    assert!(app_started("1.2.3", "private", "stable").is_none());
    assert!(app_started("1.2.3", "linux", "private").is_none());
}

mod outcomes;
