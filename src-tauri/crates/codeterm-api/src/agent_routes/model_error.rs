use axum::http::StatusCode;

pub(super) fn validate_agent_model(
    registry: &codeterm_service_core::agent::registry::AgentRegistry,
    provider_id: &str,
    model: Option<&str>,
) -> Result<(), super::SpawnRejection> {
    codeterm_service_core::service::spawn::validate_agent_model(registry, provider_id, model)
        .map_err(super::SpawnRejection::from)
}

impl From<codeterm_service_core::service::spawn::InvalidModelError> for super::SpawnRejection {
    fn from(error: codeterm_service_core::service::spawn::InvalidModelError) -> Self {
        let mut details = error.rpc_data();
        details["code"] = details["type"].clone();
        Self {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            body: serde_json::json!({"ok": false, "error": details}),
        }
    }
}
