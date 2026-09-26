use axum::http::StatusCode;

pub(super) fn validate_agent_model(
    registry: &crate::agent::registry::AgentRegistry,
    provider_id: &str,
    model: Option<&str>,
) -> Result<(), super::SpawnRejection> {
    crate::service::spawn::validate_agent_model(registry, provider_id, model)
        .map_err(super::SpawnRejection::from)
}

impl From<crate::service::spawn::InvalidModelError> for super::SpawnRejection {
    fn from(error: crate::service::spawn::InvalidModelError) -> Self {
        let mut details = error.rpc_data();
        details["code"] = serde_json::json!("invalid_model");
        Self {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            body: serde_json::json!({"ok": false, "error": details}),
        }
    }
}
