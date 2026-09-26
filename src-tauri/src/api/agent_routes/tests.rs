use super::*;

#[test]
fn spawn_body_minimal() {
    let body: SpawnAgentBody = serde_json::from_str(r#"{"agent":"claude"}"#).unwrap();
    assert_eq!(body.agent.as_deref(), Some("claude"));
}

#[test]
fn spawn_body_empty_object() {
    let body: SpawnAgentBody = serde_json::from_str("{}").unwrap();
    assert_eq!(body.agent, None);
    assert!(!body.require_delivery_ack);
}

#[test]
fn configure_spawn_can_require_delivery_ack_without_naming_a_provider() {
    let body: SpawnAgentBody =
        serde_json::from_str(r#"{"task":"opaque-configure-token","require_delivery_ack":true}"#)
            .unwrap();
    assert!(body.require_delivery_ack);
    assert_eq!(body.agent, None);
    assert_eq!(body.task.as_deref(), Some("opaque-configure-token"));
}

#[test]
fn configure_spawn_delivery_ack_round_trips_exact_failure() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel();
    tx.send(Err("opaque delivery failure 91c2".into())).unwrap();
    assert_eq!(
        runtime.block_on(await_spawn_delivery(rx)),
        Err("opaque delivery failure 91c2".into())
    );
}

#[test]
fn configure_spawn_refuses_a_pending_delivery_after_launch_ack() {
    assert_eq!(require_no_pending_delivery(None), Ok(()));
    assert_eq!(
        require_no_pending_delivery(Some("Unconfirmed".into())),
        Err(
            "agent launch ended without confirmed task delivery; pending state: Unconfirmed".into()
        )
    );
}

#[test]
fn spawn_role_validation_treats_empty_strings_as_absent() {
    let role_probe = validate_spawn_role(Some("  ".into()), Some("".into())).unwrap();
    assert!(role_probe.role.is_none());
    assert!(role_probe.role_profile.is_none());
}

#[test]
fn spawn_role_validation_rejects_unknown_role_with_body() {
    let (status, body) = validate_spawn_role(Some("Lead".into()), None).unwrap_err();
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("Invalid role: Lead"));
    assert!(!body.is_empty());
}

#[test]
fn spawn_rejection_serializes_a_structured_error_not_bare_text() {
    let rejection = SpawnRejection::from("checkout guard rejected: dirty worktree".to_string());
    assert_eq!(rejection.status, StatusCode::INTERNAL_SERVER_ERROR);
    let error = rejection.body.get("error").expect("error object present");
    assert_eq!(
        error.get("code").and_then(|v| v.as_str()),
        Some("spawn_failed")
    );
    assert_eq!(
        error.get("message").and_then(|v| v.as_str()),
        Some("checkout guard rejected: dirty worktree")
    );
    assert!(
        error
            .get("retry_hint")
            .and_then(|v| v.as_str())
            .is_some_and(|hint| !hint.is_empty()),
        "a caller must get a next-step, not just a message"
    );
    assert_eq!(
        rejection.body.get("ok").and_then(|v| v.as_bool()),
        Some(false)
    );
}

#[tokio::test]
async fn unknown_authoritative_model_returns_structured_422_without_retry_hint() {
    let registry = facade_with_agent(
        BinaryProvider::new("rest-catalogue", "rest-catalogue", None)
            .with_authoritative_models(&["model-a", "model-b"]),
    );
    let rejection =
        super::model_error::validate_agent_model(&registry, "rest-catalogue", Some("model-z"))
            .expect_err("unknown authoritative ids are a REST refusal");
    let response = rejection.into_response();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("REST error body is readable");
    let body: serde_json::Value = serde_json::from_slice(&body).expect("REST error body is JSON");
    let error = &body["error"];
    assert_eq!(body["ok"], false);
    assert_eq!(error["code"], "invalid_model");
    assert_eq!(error["type"], "invalid_model");
    assert_eq!(error["provider"], "rest-catalogue");
    assert_eq!(error["requested_model"], "model-z");
    assert_eq!(
        error["refresh_command"],
        "codeterm agent models rest-catalogue"
    );
    assert!(error["closest_model_ids"]
        .as_array()
        .is_some_and(|ids| !ids.is_empty()));
    assert!(error.get("retry_hint").is_none());
}

struct ListingShell;

impl crate::chatbackend::ChatBackendProvider for ListingShell {
    fn id(&self) -> &str {
        "local-shell"
    }

    fn open_session(
        &self,
        _: serde_json::Value,
    ) -> Result<String, crate::chatbackend::ChatBackendError> {
        Ok("session".into())
    }

    fn send_message(&self, _: &str, _: &str) -> Result<(), crate::chatbackend::ChatBackendError> {
        Ok(())
    }

    fn poll(
        &self,
        _: &str,
        _: Option<&str>,
    ) -> Result<crate::agent::chat_types::StructuredChat, crate::chatbackend::ChatBackendError>
    {
        Ok(crate::agent::chat_types::StructuredChat::default())
    }

    fn close_session(&self, _: &str) -> Result<(), crate::chatbackend::ChatBackendError> {
        Ok(())
    }

    fn list_models(
        &self,
    ) -> Result<Vec<crate::agent::provider::ModelInfo>, crate::chatbackend::ChatBackendError> {
        Ok(vec![crate::agent::provider::ModelInfo {
            id: "qwen".into(),
            display_name: "Qwen".into(),
            description: None,
            available: None,
            availability: None,
            featured: None,
            badge: None,
            group: None,
            reasoning_efforts: None,
        }])
    }

    fn display_name(&self) -> &str {
        "Local Shell"
    }

    fn spawnable(&self) -> bool {
        true
    }
}

/// A registered chatBackend that must stay hidden from the new-session picker.
struct BackendOnlyShell;

impl crate::chatbackend::ChatBackendProvider for BackendOnlyShell {
    fn id(&self) -> &str {
        "npc"
    }

    fn open_session(
        &self,
        _: serde_json::Value,
    ) -> Result<String, crate::chatbackend::ChatBackendError> {
        Ok("session".into())
    }

    fn send_message(&self, _: &str, _: &str) -> Result<(), crate::chatbackend::ChatBackendError> {
        Ok(())
    }

    fn poll(
        &self,
        _: &str,
        _: Option<&str>,
    ) -> Result<crate::agent::chat_types::StructuredChat, crate::chatbackend::ChatBackendError>
    {
        Ok(crate::agent::chat_types::StructuredChat::default())
    }

    fn close_session(&self, _: &str) -> Result<(), crate::chatbackend::ChatBackendError> {
        Ok(())
    }

    fn list_models(
        &self,
    ) -> Result<Vec<crate::agent::provider::ModelInfo>, crate::chatbackend::ChatBackendError> {
        Ok(Vec::new())
    }
}

fn facade_with(
    shells: &[(
        &str,
        std::sync::Arc<dyn crate::chatbackend::ChatBackendProvider>,
    )],
) -> crate::provider::ProviderRegistry {
    let agents = std::sync::Arc::new(crate::agent::registry::AgentRegistry::new());
    let backends = std::sync::Arc::new(crate::chatbackend::ChatBackendRegistry::new());
    for (id, shell) in shells {
        backends.register_and_wait(*id, shell.clone());
    }
    crate::provider::ProviderRegistry::new(agents, backends)
}

#[test]
fn provider_listing_includes_spawnable_chatbackend_tagged_transport() {
    let registry = facade_with(&[("local-shell", std::sync::Arc::new(ListingShell))]);
    let listing = super::list_agent_provider_values(&registry);
    let shell = listing
        .iter()
        .find(|p| p.get("id").and_then(|v| v.as_str()) == Some("local-shell"))
        .expect("registered spawnable chatBackend shell is listed");

    assert!(shell.get("kind").is_none());
    assert_eq!(shell["transport"], "chat_backend");
    assert_eq!(shell["display_name"], "Local Shell");
    assert_eq!(shell["available_models"][0]["id"], "qwen");
}

#[test]
fn provider_listing_hides_non_spawnable_chatbackends() {
    let registry = facade_with(&[
        ("local-shell", std::sync::Arc::new(ListingShell)),
        ("npc", std::sync::Arc::new(BackendOnlyShell)),
    ]);
    let listing = super::list_agent_provider_values(&registry);

    assert!(
        listing
            .iter()
            .any(|p| p.get("id").and_then(|v| v.as_str()) == Some("local-shell")),
        "spawnable shell is listed"
    );
    assert!(
        !listing
            .iter()
            .any(|p| p.get("id").and_then(|v| v.as_str()) == Some("npc")),
        "non-spawnable backend must NOT be listed"
    );
}

/// A PTY agent whose declared launch binary is whatever the fixture says, so
/// enumeration can be asked about a machine that has it and one that does not.
struct BinaryProvider {
    id: &'static str,
    binary_name: &'static str,
    cli_binary: Option<&'static str>,
    models: Vec<crate::agent::provider::ModelInfo>,
    model_catalogue: crate::agent::provider::ModelCatalogueCapability,
}

impl BinaryProvider {
    fn new(id: &'static str, binary_name: &'static str, cli_binary: Option<&'static str>) -> Self {
        Self {
            id,
            binary_name,
            cli_binary,
            models: Vec::new(),
            model_catalogue: Default::default(),
        }
    }

    fn with_authoritative_models(mut self, ids: &[&str]) -> Self {
        self.model_catalogue = crate::agent::provider::ModelCatalogueCapability::authoritative();
        self.models = ids
            .iter()
            .map(|id| crate::agent::provider::ModelInfo {
                id: (*id).into(),
                display_name: (*id).into(),
                description: None,
                available: None,
                availability: None,
                featured: None,
                badge: None,
                group: None,
                reasoning_efforts: None,
            })
            .collect();
        self
    }
}

impl crate::agent::provider::AgentProvider for BinaryProvider {
    fn id(&self) -> &str {
        self.id
    }

    fn display_name(&self) -> &str {
        self.id
    }

    fn binary_name(&self) -> &str {
        self.binary_name
    }

    fn build_launch_command(&self, _: &crate::agent::provider::AgentLaunchParams) -> String {
        self.id.into()
    }

    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }

    fn readiness_strategy(&self) -> crate::agent::provider::ReadinessStrategy {
        crate::agent::provider::ReadinessStrategy::FixedDelay {
            duration: std::time::Duration::ZERO,
        }
    }

    fn task_delivery(&self) -> crate::agent::provider::TaskDelivery {
        crate::agent::provider::TaskDelivery::PtyInput
    }

    fn detect_from_title(&self, _: &str) -> bool {
        false
    }

    fn detect_from_output(&self, _: &str) -> bool {
        false
    }

    fn session_file_path(&self, _: &str, _: &str) -> Option<std::path::PathBuf> {
        None
    }

    fn detect_session_id(
        &self,
        _: &str,
        _: &[String],
        _: Option<std::time::Duration>,
    ) -> Option<String> {
        None
    }

    fn cli_binary(&self) -> Option<&str> {
        self.cli_binary
    }

    fn available_models(&self) -> Vec<crate::agent::provider::ModelInfo> {
        self.models.clone()
    }

    fn model_catalogue_capability(&self) -> crate::agent::provider::ModelCatalogueCapability {
        self.model_catalogue.clone()
    }
}

fn facade_with_agent(provider: BinaryProvider) -> crate::provider::ProviderRegistry {
    if !provider.models.is_empty() {
        crate::agent::model_catalog::register(
            provider.id,
            crate::agent::model_catalog::RegisteredDefaults {
                models: provider.models.clone(),
                default: provider.models.first().map(|model| model.id.clone()),
            },
        );
    }
    let agents = std::sync::Arc::new(crate::agent::registry::AgentRegistry::new());
    agents.register_provider(Box::new(provider));
    let backends = std::sync::Arc::new(crate::chatbackend::ChatBackendRegistry::new());
    crate::provider::ProviderRegistry::new(agents, backends)
}

fn listed(listing: &[serde_json::Value], id: &str) -> serde_json::Value {
    listing
        .iter()
        .find(|p| p.get("id").and_then(|v| v.as_str()) == Some(id))
        .unwrap_or_else(|| panic!("provider {id} is listed"))
        .clone()
}

/// Registration says a provider exists; only a probe says it can run here. The
/// row must carry what was observed, not what was registered.
#[test]
fn provider_listing_states_what_it_observed_about_the_launch_binary() {
    let registry = facade_with_agent(BinaryProvider::new(
        "listing-missing-cli",
        "codeterm-definitely-missing-provider-cli",
        Some("codeterm-definitely-missing-provider-cli"),
    ));
    let listing = super::list_agent_provider_values(&registry);

    assert_eq!(
        listed(&listing, "listing-missing-cli")["availability"],
        serde_json::json!("unavailable")
    );
}

/// Enumeration observes the launch hook even when install metadata is absent.
#[test]
fn provider_listing_uses_binary_name_not_optional_install_metadata() {
    let registry = facade_with_agent(BinaryProvider::new(
        "listing-without-install-metadata",
        "codeterm-definitely-missing-provider-hook",
        None,
    ));
    let listing = super::list_agent_provider_values(&registry);

    assert_eq!(
        listed(&listing, "listing-without-install-metadata")["availability"],
        serde_json::json!("unavailable")
    );
}

/// The wire carries all three states. A reader that only ever sees two would
/// have to invent one, which is the collapse this slice removes.
#[test]
fn unknown_is_a_wire_state_of_its_own_not_a_synonym_for_unavailable() {
    let mut descriptor =
        super::list_agent_provider_values(&facade_with_agent(BinaryProvider::new(
            "listing-unknown",
            "codeterm-definitely-missing-provider-cli",
            Some("codeterm-definitely-missing-provider-cli"),
        )))
        .remove(0);
    descriptor["availability"] = serde_json::json!("unknown");

    let parsed: crate::agent::provider::ProviderDescriptor =
        serde_json::from_value(descriptor).unwrap();
    assert_eq!(
        parsed.availability,
        crate::agent::provider::ProviderAvailability::Unknown
    );
    assert_ne!(
        parsed.availability,
        crate::agent::provider::ProviderAvailability::Unavailable
    );
}

/// Enumeration must answer with the SHARED resolver. A private ladder would
/// drift from it exactly on the machines where the two disagree.
#[test]
fn enumeration_agrees_with_the_shared_resolver() {
    const BINARY: &str = "codeterm-definitely-missing-provider-cli";
    let registry = facade_with_agent(BinaryProvider::new(
        "listing-shared-resolver",
        BINARY,
        Some(BINARY),
    ));

    let listed = registry.describe_all().remove(0).availability;
    let shared = crate::agent::provider::ProviderAvailability::from_resolution(
        &codeterm_plugin::js_runtime::probe_bin(BINARY).resolution,
    );
    assert_eq!(
        listed, shared,
        "the row must be the shared resolver's verdict, not a second ladder's"
    );
}
