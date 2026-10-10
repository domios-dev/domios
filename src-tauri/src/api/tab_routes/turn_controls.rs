use super::*;

#[derive(Deserialize)]
pub struct TurnControlsQuery {
    pub session_id: String,
}

#[derive(Clone, serde::Serialize)]
pub struct TurnControls {
    id: String,
    agent_session_id: Option<String>,
    session_binding_nonce: Option<String>,
    reported_session_binding_nonce: Option<String>,
    model: Option<String>,
    effort: Option<String>,
}

fn validate_controls(
    controls: TurnControls,
    session: &str,
    nonce: &str,
) -> Result<TurnControls, (StatusCode, &'static str)> {
    if nonce.is_empty()
        || controls.agent_session_id.as_deref() != Some(session)
        || controls.session_binding_nonce.as_deref() != Some(nonce)
        || controls.reported_session_binding_nonce.as_deref() != Some(nonce)
    {
        Err((StatusCode::CONFLICT, "Turn controls binding is not current"))
    } else {
        Ok(controls)
    }
}

fn selection(
    staged: Option<&crate::multiplexer::types::PendingModelSelection>,
    model: Option<&str>,
    effort: Option<&str>,
) -> (Option<String>, Option<String>) {
    (
        staged
            .map(|selection| selection.model.as_str())
            .or(model)
            .map(str::to_owned),
        staged
            .and_then(|selection| selection.effort.as_deref())
            .or(effort)
            .map(str::to_owned),
    )
}

pub async fn turn_controls(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(tab_id): Path<String>,
    Query(query): Query<TurnControlsQuery>,
) -> Result<Json<TurnControls>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let nonce = headers
        .get("x-codeterm-binding-nonce")
        .and_then(|header| header.to_str().ok())
        .filter(|nonce| !nonce.is_empty())
        .ok_or((StatusCode::UNAUTHORIZED, "Binding nonce is required"))?
        .to_owned();
    let controls = tokio::task::spawn_blocking(move || {
        let mux = state.svc.mux.read();
        let tab = mux
            .get_tab(&tab_id)
            .ok_or((StatusCode::NOT_FOUND, "Tab was not found"))?;
        let (model, effort) = selection(
            tab.pending_model_selection.as_ref(),
            tab.agent_provider_resolved_model.as_deref(),
            tab.reasoning_effort_identity.resolved.as_deref(),
        );
        validate_controls(
            TurnControls {
                id: tab.id.clone(),
                agent_session_id: tab.agent_session_id.clone(),
                session_binding_nonce: tab.session_binding_nonce.clone(),
                reported_session_binding_nonce: tab.reported_session_binding_nonce.clone(),
                model,
                effort,
            },
            &query.session_id,
            &nonce,
        )
    })
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Turn controls read failed",
        )
    })??;
    Ok(Json(controls))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staged_selection_is_visible_before_completion_without_changing_actual_identity() {
        let staged = crate::multiplexer::types::PendingModelSelection {
            model: "target/model".into(),
            effort: Some("high".into()),
        };
        assert_eq!(
            selection(Some(&staged), Some("old/model"), Some("low")),
            (Some("target/model".into()), Some("high".into()))
        );
        assert_eq!(
            selection(None, Some("old/model"), Some("low")),
            (Some("old/model".into()), Some("low".into()))
        );
    }

    #[test]
    fn controls_require_exact_session_and_current_acknowledged_nonce() {
        let controls = TurnControls {
            id: "own".into(),
            agent_session_id: Some("session".into()),
            session_binding_nonce: Some("nonce".into()),
            reported_session_binding_nonce: Some("nonce".into()),
            model: None,
            effort: None,
        };
        assert!(validate_controls(controls.clone(), "session", "nonce").is_ok());
        assert!(validate_controls(controls.clone(), "foreign", "nonce").is_err());
        assert!(validate_controls(controls.clone(), "session", "retired").is_err());
        assert!(validate_controls(controls.clone(), "session", "").is_err());
        assert!(validate_controls(
            TurnControls {
                reported_session_binding_nonce: None,
                ..controls
            },
            "session",
            "nonce"
        )
        .is_err());
    }
}
