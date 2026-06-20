use axum::{
    extract::{Path, Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, resolve_pane, AppState};

pub(crate) fn list_agent_provider_values(
    registry: &crate::provider::ProviderRegistry,
) -> Vec<serde_json::Value> {
    registry
        .describe_all()
        .into_iter()
        .filter_map(|descriptor| serde_json::to_value(descriptor).ok())
        .collect()
}

#[derive(Deserialize)]
pub(crate) struct SpawnAgentBody {
    /// Agent command to run (e.g. "claude", "aider", "goose"). Default: "claude"
    agent: Option<String>,
    /// Claude agent name from .claude/agents/ (e.g. "architect" → --agent architect)
    agent_name: Option<String>,
    /// Extra args for the agent
    args: Option<Vec<String>>,
    cwd: Option<String>,
    title: Option<String>,
    description: Option<String>,
    role: Option<String>,
    parent_pane_id: Option<String>,
    can_spawn: Option<bool>,
    task: Option<String>,
    workspace_id: Option<String>,
    force_tab: Option<bool>,
    report_target_pane_id: Option<String>,
    skip_permissions: Option<bool>,
    /// Pane id of the caller that ran `agent spawn` (CODETERM_PANE_ID).
    /// Recorded on the spawned pane for report routing fallback.
    spawned_by_pane_id: Option<String>,
    /// Model selector — provider decides flag syntax via
    /// AgentProvider::model_launch_args.
    model: Option<String>,
    #[serde(alias = "system")]
    system_prompt: Option<String>,
    preset: Option<String>,
    activate: Option<bool>,
}

pub async fn spawn_agent(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<SpawnAgentBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    if let Some(ref role) = body.role {
        crate::service::ServiceContext::parse_role(role)
            .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    }
    let params = crate::service::SpawnAgentParams {
        agent: body.agent,
        agent_name: body.agent_name,
        args: body.args,
        cwd: body.cwd,
        title: body.title,
        description: body.description,
        role: body.role,
        parent_pane_id: body.parent_pane_id,
        can_spawn: body.can_spawn,
        task: body.task,
        workspace_id: body.workspace_id,
        force_tab: body.force_tab,
        report_target_pane_id: body.report_target_pane_id,
        skip_permissions: body.skip_permissions,
        spawned_by_pane_id: body.spawned_by_pane_id,
        model: body.model,
        system_prompt: body.system_prompt,
        preset: body.preset,
        resume_session_id: None,
        activate: body.activate,
    };
    let result = state
        .svc
        .spawn_agent(params, None)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    let mut resp = serde_json::json!({
        "id": result.pane_id,
        "pane_id": result.pane_id,
        "tab_id": result.tab_id,
        "pty_id": result.pty_id,
        "agent": result.agent,
        "title": result.title,
        "description": result.description,
        "role": result.role,
    });
    if let Some(ref warning) = result.warning {
        resp["warning"] = serde_json::json!(warning);
    }
    Ok(Json(resp))
}

#[derive(Deserialize)]
pub(crate) struct BatchBody {
    commands: Vec<BatchOp>,
}

#[derive(Deserialize)]
pub(crate) struct BatchOp {
    method: String,
    #[serde(default)]
    params: serde_json::Value,
    /// Optional ID — other commands can reference this result via "$id"
    id: Option<String>,
}

pub async fn batch_commands(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<BatchBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let ops: Vec<crate::service::BatchOp> = body
        .commands
        .into_iter()
        .map(|c| crate::service::BatchOp {
            method: c.method,
            params: c.params,
            id: c.id,
        })
        .collect();
    let results = state.svc.batch(ops).await;
    Ok(Json(serde_json::json!({"results": results})))
}

#[derive(Deserialize)]
pub(crate) struct SetRoleBody {
    role: String,
    parent_pane_id: Option<String>,
    can_spawn: Option<bool>,
}

pub async fn set_pane_role(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<SetRoleBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    state
        .svc
        .set_pane_role(
            &id,
            &body.role,
            body.parent_pane_id.as_deref(),
            body.can_spawn,
        )
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn clear_pane_role(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    state
        .svc
        .clear_pane_role(&id)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn get_pane_status(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let info = state
        .svc
        .get_pane_status(&id)
        .map_err(|e| (StatusCode::NOT_FOUND, e))?;
    Ok(Json(serde_json::to_value(info).unwrap()))
}

#[derive(Deserialize)]
pub(crate) struct ReportBody {
    message: String,
    #[serde(flatten)]
    details: crate::agent::reports::ReportDetails,
}

pub async fn submit_report(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<ReportBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let result = state
        .svc
        .submit_report(&id, &body.message, body.details)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(
        serde_json::json!({"ok": true, "report_id": result.report_id}),
    ))
}

#[derive(Deserialize)]
pub(crate) struct ReportsQuery {
    clear: Option<bool>,
}

pub async fn get_reports(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<ReportsQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let clear = query.clear.unwrap_or(false);
    let reports = state.svc.report_store.lock().get(&id, clear);
    Ok(Json(serde_json::to_value(&reports).unwrap_or_default()))
}

#[derive(Deserialize)]
pub(crate) struct CanSpawnBody {
    can_spawn: bool,
}

pub async fn set_can_spawn(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<CanSpawnBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    state
        .svc
        .set_can_spawn(&id, body.can_spawn)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
pub(crate) struct AgentRestartBody {
    #[serde(default)]
    force: bool,
    #[serde(default = "default_grace_secs")]
    grace_secs: u64,
}

fn default_grace_secs() -> u64 {
    crate::service::agent_restart::DEFAULT_GRACE_SECS
}

pub async fn agent_restart_handler(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
    body: Option<Json<AgentRestartBody>>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    if let Err((status, msg)) = check_auth(&state, &headers) {
        return (status, msg.to_string()).into_response();
    }
    let pane_id = match resolve_pane(&state, &raw_pane_id) {
        Ok(p) => p,
        Err((status, msg)) => return (status, msg.to_string()).into_response(),
    };
    let body = body.map(|Json(b)| b).unwrap_or(AgentRestartBody {
        force: false,
        grace_secs: default_grace_secs(),
    });

    match state
        .svc
        .agent_restart(&pane_id, body.force, body.grace_secs)
    {
        Ok(outcome) => (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({
                "restarting": true,
                "pane_id": outcome.pane_id,
                "provider": outcome.provider,
                "session_id": outcome.session_id,
                "session_was_stale": outcome.session_was_stale,
                "force": outcome.force,
            })),
        )
            .into_response(),
        Err(crate::service::agent_restart::AgentRestartError::NotFound(m)) => {
            (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": m}))).into_response()
        }
        Err(crate::service::agent_restart::AgentRestartError::BadRequest(m)) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": m})),
        )
            .into_response(),
        Err(crate::service::agent_restart::AgentRestartError::RateLimited { retry_after_secs }) => {
            let mut h = HeaderMap::new();
            h.insert("Retry-After", retry_after_secs.to_string().parse().unwrap());
            (
                StatusCode::TOO_MANY_REQUESTS,
                h,
                Json(serde_json::json!({
                    "error": "rate limited",
                    "retry_after_secs": retry_after_secs,
                })),
            )
                .into_response()
        }
        Err(crate::service::agent_restart::AgentRestartError::Internal(m)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": m})),
        )
            .into_response(),
    }
}

pub async fn respond_prompt(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let pane_id = resolve_pane(&state, &raw_pane_id).map_err(|(s, m)| (s, m.to_string()))?;
    let value = body
        .get("value")
        .and_then(|v| v.as_str())
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "Missing 'value' field".to_string()))?
        .to_string();
    state
        .svc
        .respond_prompt(&pane_id, &value)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

/// Clear pending_prompt state without sending to PTY (safe for cleanup/reset).
pub async fn clear_prompt(
    AxumState(state): AxumState<AppState>,
    Path(raw_pane_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let pane_id = resolve_pane(&state, &raw_pane_id).map_err(|(s, m)| (s, m.to_string()))?;
    state
        .svc
        .clear_prompt(&pane_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

/// Set agent type on a pane (enables chat view for that agent's provider).
pub async fn set_agent_type(
    AxumState(state): AxumState<AppState>,
    Path(pane_id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let agent_type = body
        .get("agent_type")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    state.svc.set_pane_agent_type(&pane_id, agent_type);
    state.svc.emit_state_changed();
    Ok(Json(serde_json::json!({"ok": true})))
}

/// List all registered agent providers as `ProviderDescriptor`s.
pub async fn list_agent_providers(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    Ok(Json(serde_json::Value::Array(list_agent_provider_values(
        &state.svc.provider_registry,
    ))))
}

/// Public provider list for local plugin verification/UI smoke checks.
/// This exposes provider descriptors only; mutating agent routes remain
/// authenticated.
pub async fn list_agent_providers_public(
    AxumState(state): AxumState<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    Ok(Json(serde_json::Value::Array(list_agent_provider_values(
        &state.svc.provider_registry,
    ))))
}

#[cfg(test)]
mod tests {
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

        fn send_message(
            &self,
            _: &str,
            _: &str,
        ) -> Result<(), crate::chatbackend::ChatBackendError> {
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
        ) -> Result<Vec<crate::agent::provider::ModelInfo>, crate::chatbackend::ChatBackendError>
        {
            Ok(vec![crate::agent::provider::ModelInfo {
                id: "qwen".into(),
                display_name: "Qwen".into(),
                description: None,
                available: None,
            }])
        }

        fn display_name(&self) -> &str {
            "Local Shell"
        }

        fn spawnable(&self) -> bool {
            true
        }
    }

    /// A registered chatBackend that is NOT user-spawnable (bare
    /// `chatBackend: true`) — e.g. the NPC backend. Must be hidden from the
    /// new-session picker.
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

        fn send_message(
            &self,
            _: &str,
            _: &str,
        ) -> Result<(), crate::chatbackend::ChatBackendError> {
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
        ) -> Result<Vec<crate::agent::provider::ModelInfo>, crate::chatbackend::ChatBackendError>
        {
            Ok(Vec::new())
        }
        // spawnable() defaults to false.
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
            backends.register(*id, shell.clone());
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
        // `kind` is gone — transport is the discriminant now.
        assert!(
            shell.get("kind").is_none(),
            "no `kind` key in serialized output"
        );
        assert_eq!(shell["transport"], "chat_backend");
        assert_eq!(shell["display_name"], "Local Shell");
        assert_eq!(shell["available_models"][0]["id"], "qwen");
    }

    #[test]
    fn provider_listing_hides_non_spawnable_chatbackends() {
        // Regression: the NPC backend (bare `chatBackend: true`) wrongly showed
        // in the new-session picker. Only spawnable shells are listed.
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
}
