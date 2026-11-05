use super::*;
use crate::agent::model_catalog;
use crate::agent::provider::*;
use crate::service::{spawn, test_fixture::ServiceFixture};

struct EndpointHarness {
    id: String,
    adapter: Arc<dyn ModelProviderConfig>,
    discovery: Option<HeldDiscovery>,
}

/// A live probe that reports its start and answers only when the test releases it.
struct HeldDiscovery {
    started: std::sync::Mutex<std::sync::mpsc::Sender<()>>,
    release: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
}

#[async_trait::async_trait]
impl AgentProvider for EndpointHarness {
    async fn discover_models(&self) -> Option<Vec<ModelInfo>> {
        let held = self.discovery.as_ref()?;
        let _ = held.started.lock().unwrap().send(());
        let _ = held.release.lock().unwrap().recv();
        None
    }
    fn id(&self) -> &str {
        &self.id
    }
    fn display_name(&self) -> &str {
        &self.id
    }
    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        String::new()
    }
    fn readiness_strategy(&self) -> ReadinessStrategy {
        ReadinessStrategy::FixedDelay {
            duration: Duration::ZERO,
        }
    }
    fn task_delivery(&self) -> TaskDelivery {
        TaskDelivery::PtyInput
    }
    fn detect_from_title(&self, _: &str) -> bool {
        false
    }
    fn detect_from_output(&self, _: &str) -> bool {
        false
    }
    fn session_file_path(&self, _: &str, _: &str) -> Option<PathBuf> {
        None
    }
    fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> {
        None
    }
    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }
    fn available_models(&self) -> Vec<ModelInfo> {
        model_catalog::resolve_models(&self.id)
    }
    fn model_catalogue_capability(&self) -> ModelCatalogueCapability {
        ModelCatalogueCapability::authoritative_live()
    }
    fn model_provider_config(&self) -> Option<Arc<dyn ModelProviderConfig>> {
        Some(self.adapter.clone())
    }
}

fn fixture() -> (ServiceFixture, String, PathBuf) {
    fixture_with(None)
}

fn fixture_with(discovery: Option<HeldDiscovery>) -> (ServiceFixture, String, PathBuf) {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let mut fixture = ServiceFixture::new(mux);
    fixture.service.secrets = store();
    let id = format!("endpoint-harness-{}", uuid::Uuid::new_v4());
    let path = fixture.temp.path().join("config.json");
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(EndpointHarness {
            id: id.clone(),
            adapter: Arc::new(adapter(path.clone())),
            discovery,
        }));
    model_catalog::put_live(
        &id,
        vec![ModelInfo {
            id: "mimo/mimo-v2.6-pro".into(),
            ..Default::default()
        }],
        None,
    );
    (fixture, id, path)
}

fn add(
    service: &ServiceContext,
    harness: &str,
    endpoint: &str,
    model: &str,
) -> Result<Value, ModelProviderError> {
    service.model_provider_command(Some(harness), serde_json::from_value(json!({
        "action": "add", "provider": { "id": endpoint, "kind": "openai-compatible",
        "baseUrl": "http://127.0.0.1:9/v1", "models": [model], "secretName": format!("dummy/{endpoint}") },
        "value": "dummy-endpoint-key"
    })).unwrap())
}

#[test]
fn agent_models_lists_endpoint_added_after_stale_harness_snapshot() {
    let (fixture, harness, _) = fixture();
    add(
        &fixture.service,
        &harness,
        "qa-review-endpoint",
        "qa-route-model",
    )
    .unwrap();
    let descriptors = fixture.service.agent_registry.list_descriptors();
    let descriptor = descriptors
        .iter()
        .find(|descriptor| descriptor.id == harness)
        .unwrap();
    let value = serde_json::to_value(descriptor).unwrap();
    assert!(value["available_models"]
        .as_array()
        .unwrap()
        .iter()
        .any(|model| model["id"] == "qa-review-endpoint/qa-route-model"));
    model_catalog::unregister(&harness);
}

#[test]
fn model_provider_change_returns_before_a_held_live_discovery_finishes() {
    // A failure boundary for a wedged expectation, never evidence of success.
    const WEDGED: Duration = Duration::from_secs(3);
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let (fixture, harness, _) = fixture_with(Some(HeldDiscovery {
        started: std::sync::Mutex::new(started_tx),
        release: std::sync::Mutex::new(release_rx),
    }));
    let (added_tx, added_rx) = std::sync::mpsc::channel();
    let (added, discovery_started) = std::thread::scope(|scope| {
        let service = &fixture.service;
        let harness = harness.as_str();
        scope.spawn(move || {
            let _ = added_tx.send(add(
                service,
                harness,
                "qa-review-endpoint",
                "qa-route-model",
            ));
        });
        let added = added_rx.recv_timeout(WEDGED);
        let discovery_started = started_rx.recv_timeout(WEDGED).is_ok();
        // Dropping the release ends every held probe, so the scope can always join.
        drop(release_tx);
        (added, discovery_started)
    });
    added
        .expect("the config change waited for the live discovery")
        .unwrap();
    assert!(discovery_started, "the live discovery never started");
    let descriptor = fixture
        .service
        .agent_registry
        .list_descriptors()
        .into_iter()
        .find(|descriptor| descriptor.id == harness)
        .unwrap();
    assert!(
        serde_json::to_value(descriptor).unwrap()["available_models"]
            .as_array()
            .unwrap()
            .iter()
            .any(|model| model["id"] == "qa-review-endpoint/qa-route-model")
    );
    model_catalog::unregister(&harness);
}

#[test]
fn model_provider_add_refuses_harness_id_before_config_or_secret_write() {
    let (fixture, harness, path) = fixture();
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(EndpointHarness {
            id: "fixture-other-harness".into(),
            adapter: Arc::new(adapter(path.clone())),
            discovery: None,
        }));
    let error = add(
        &fixture.service,
        &harness,
        "fixture-other-harness",
        "qa-collision-model",
    )
    .unwrap_err();
    assert_eq!(serde_json::to_value(error).unwrap(), "harness_id_collision");
    assert!(!path.exists());
    assert!(crate::async_runtime::block_on(async {
        fixture
            .service
            .secrets
            .read()
            .await
            .get("dummy/fixture-other-harness")
            .await
    })
    .is_err());
    let request = serde_json::from_value(json!({"action":"add_from_secret", "provider": {
        "id": harness, "kind":"openai-compatible", "baseUrl":"http://127.0.0.1:9/v1", "models":["qa-collision-model"]
    }, "source_secret":"absent-secret"})).unwrap();
    assert_eq!(
        fixture
            .service
            .model_provider_command(Some(&harness), request)
            .unwrap_err(),
        ModelProviderError::HarnessIdCollision
    );
    model_catalog::unregister(&harness);
}

#[test]
fn agent_spawn_model_resolves_endpoint_model_outside_stale_harness_snapshot() {
    let (fixture, harness, _) = fixture();
    add(
        &fixture.service,
        &harness,
        "qa-review-endpoint",
        "qa-route-model",
    )
    .unwrap();
    let selected = spawn::resolve_agent_model_selection(
        &fixture.service.agent_registry,
        Some(&harness),
        Some("qa-review-endpoint/qa-route-model"),
        &harness,
    )
    .unwrap();
    assert_eq!(selected.provider_id, harness);
    assert_eq!(
        selected.model.as_deref(),
        Some("qa-review-endpoint/qa-route-model")
    );
    let resolved = spawn::resolve_fresh_agent_model_with_notice(
        &fixture.service.configuration(),
        &fixture.service.agent_registry,
        &harness,
        selected.model,
        None,
    );
    assert_eq!(
        resolved.model.as_deref(),
        Some("qa-review-endpoint/qa-route-model")
    );
    assert_eq!(
        resolved.provider_resolved_model.as_deref(),
        Some("qa-review-endpoint/qa-route-model")
    );
    let error = spawn::resolve_agent_model_selection(
        &fixture.service.agent_registry,
        Some(&harness),
        Some("qa-review-endpoint/absent-model"),
        &harness,
    )
    .unwrap_err();
    assert_eq!(
        error.valid_model_ids,
        vec!["qa-review-endpoint/qa-route-model"]
    );
    assert!(error.ambiguous_providers.is_empty());
    model_catalog::unregister(&harness);
}
