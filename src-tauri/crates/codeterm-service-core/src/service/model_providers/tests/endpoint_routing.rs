use super::*;
use crate::agent::model_catalog;
use crate::agent::provider::*;
use crate::service::{spawn, test_fixture::ServiceFixture};

struct EndpointHarness {
    id: String,
    adapter: Arc<dyn ModelProviderConfig>,
}

impl AgentProvider for EndpointHarness {
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
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let mut fixture = ServiceFixture::new(mux);
    fixture.service.secrets = store();
    let id = format!("endpoint-harness-{}", uuid::Uuid::new_v4());
    let path = fixture.temp.path().join("opencode.json");
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(EndpointHarness {
            id: id.clone(),
            adapter: Arc::new(adapter(path.clone())),
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
fn model_provider_add_refuses_harness_id_before_config_or_secret_write() {
    let (fixture, harness, path) = fixture();
    let error = add(&fixture.service, &harness, &harness, "qa-collision-model").unwrap_err();
    assert_eq!(serde_json::to_value(error).unwrap(), "harness_id_collision");
    assert!(!path.exists());
    assert!(crate::async_runtime::block_on(async {
        fixture
            .service
            .secrets
            .read()
            .await
            .get(&format!("dummy/{harness}"))
            .await
    })
    .is_err());
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
    model_catalog::unregister(&harness);
}
