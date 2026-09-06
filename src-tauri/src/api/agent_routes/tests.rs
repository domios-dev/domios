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
