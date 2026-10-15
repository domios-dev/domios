use super::*;

/// Clear pending_prompt state without sending to PTY (safe for cleanup/reset).
pub async fn clear_prompt(
    AxumState(state): AxumState<AppState>,
    Path(raw_tab_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let tab_id = crate::routes::resolve_tab_async(&state, &raw_tab_id)
        .await
        .map_err(|(s, m)| (s, m.to_string()))?;
    tokio::task::spawn_blocking(move || {
        state
            .svc
            .clear_prompt(&tab_id)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
        Ok(Json(serde_json::json!({"ok": true})))
    })
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "blocking task failed".to_string(),
        )
    })?
}

/// Set agent type on a tab (enables chat view for that agent's provider).
pub async fn set_agent_type(
    AxumState(state): AxumState<AppState>,
    Path(tab_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || {
        let agent_type = body
            .get("agent_type")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        match agent_type {
            Some(agent_type) => {
                state.svc.write_tab_agent_type_with_origin(
                    &tab_id,
                    agent_type,
                    false,
                    codeterm_service_core::session::lifecycle::AgentTypeWriteOrigin::Ui,
                    crate::tab_routes::caller_tab_header(&headers).as_deref(),
                );
            }
            None => {
                state.svc.set_tab_agent_type(&tab_id, None);
            }
        }
        state.svc.emit_state_changed();
        Ok(Json(serde_json::json!({"ok": true})))
    })
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "blocking task failed".to_string(),
        )
    })?
}
