use super::*;

pub async fn archive(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    authorize_plan_mutation_http(&state.svc, &headers, &id).await?;
    tokio::task::spawn_blocking(move || {
        state
            .svc
            .plan_store
            .archive(&id, epoch_ms())
            .map_err(plan_err)?;
        emit_plan_changed(&state, &id);
        let mut resp = serde_json::json!({ "ok": true });
        if let Some(plan) = state.svc.plan_store.get(&id) {
            let cleanup =
                codeterm_service_core::service::plan::cleanup_debt::detect_plan_cleanup_debt(
                    &plan,
                    |tab| state.svc.tab_probe(tab).exists,
                );
            push_hints(
                &mut resp,
                codeterm_service_core::service::plan::cleanup_debt::cleanup_debt_hints(&cleanup),
            );
        }
        Ok(Json(resp))
    })
    .await
    .map_err(|_| {
        text_err(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "blocking task failed",
        )
    })?
}

pub async fn unarchive(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    authorize_plan_mutation_http(&state.svc, &headers, &id).await?;
    tokio::task::spawn_blocking(move || {
        codeterm_service_core::service::unarchive_team_plan(&state.svc, &id, epoch_ms())
            .map_err(team_plan_err)?;
        Ok(Json(serde_json::json!({ "ok": true })))
    })
    .await
    .map_err(|_| {
        text_err(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "blocking task failed",
        )
    })?
}

#[derive(Deserialize)]
pub struct PauseBody {
    reason: String,
    #[serde(default)]
    from: OrchestratorRef,
}

pub async fn pause(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<PauseBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    authorize_plan_mutation_http(&state.svc, &headers, &id).await?;
    tokio::task::spawn_blocking(move || {
        let outcome = state
            .svc
            .pause_plan_operation(&id, &body.reason, body.from, epoch_ms())
            .map_err(team_plan_err)?;
        let mut response = serde_json::to_value(&outcome).unwrap_or_default();
        response["ok"] = serde_json::json!(true);
        response["stride_id"] = serde_json::json!(outcome.stride_id.unwrap_or_default());
        Ok(Json(response))
    })
    .await
    .map_err(|_| {
        text_err(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "blocking task failed",
        )
    })?
}

pub async fn resume(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    authorize_plan_mutation_http(&state.svc, &headers, &id).await?;
    tokio::task::spawn_blocking(move || {
        let outcome = state
            .svc
            .resume_plan_operation(&id, epoch_ms())
            .map_err(team_plan_err)?;
        let mut response = serde_json::to_value(&outcome).unwrap_or_default();
        response["ok"] = serde_json::json!(true);
        Ok(Json(response))
    })
    .await
    .map_err(|_| {
        text_err(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "blocking task failed",
        )
    })?
}

pub(super) async fn authorize_plan_mutation_http(
    svc: &codeterm_service_core::service::ServiceState,
    headers: &HeaderMap,
    plan_id: &str,
) -> Result<(), (StatusCode, Json<Value>)> {
    let caller = crate::tab_routes::authenticated_plan_caller(headers);
    let confirmed_human = headers
        .get("X-CodeTerm-Plan-Confirmation")
        .and_then(|value| value.to_str().ok())
        == Some(plan_id);
    if caller.tab_id.as_deref().is_none_or(str::is_empty) && !confirmed_human {
        return Err(text_err(
            StatusCode::FORBIDDEN,
            "plan mutation requires a Plan role or explicit human confirmation",
        ));
    }
    if !confirmed_human
        && !matches!(
            svc.classify_planning_caller_async(plan_id, &caller).await,
            Ok(codeterm_service_core::plan::PlanningAuthor::User
                | codeterm_service_core::plan::PlanningAuthor::Planner { .. })
        )
    {
        return Err(text_err(
            StatusCode::FORBIDDEN,
            "the caller is not the Plan Orchestrator or Planner",
        ));
    }
    Ok(())
}

pub async fn pause_preview(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> PlanRouteResult {
    tokio::task::spawn_blocking(move || {
        check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
        let preview = state.svc.plan_pause_preview(&id).map_err(team_plan_err)?;
        Ok(Json(serde_json::to_value(preview).unwrap_or_default()))
    })
    .await
    .map_err(|_| {
        text_err(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "blocking task failed",
        )
    })?
}

pub async fn resume_preview(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> PlanRouteResult {
    tokio::task::spawn_blocking(move || {
        check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
        let preview = state.svc.plan_resume_preview(&id).map_err(team_plan_err)?;
        Ok(Json(serde_json::to_value(preview).unwrap_or_default()))
    })
    .await
    .map_err(|_| {
        text_err(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "blocking task failed",
        )
    })?
}

#[derive(Deserialize)]
pub struct TransferBody {
    to: OrchestratorRef,
    #[serde(default)]
    reason: String,
}

fn transfer_caller(headers: &HeaderMap) -> Result<String, (StatusCode, Json<Value>)> {
    crate::tab_routes::caller_tab_header(headers)
        .filter(|tab_id| !tab_id.trim().is_empty())
        .ok_or_else(|| {
            text_err(
                StatusCode::FORBIDDEN,
                "X-CodeTerm-Tab-Id is required for plan transfer",
            )
        })
}

pub async fn transfer(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<TransferBody>,
) -> PlanRouteResult {
    tokio::task::spawn_blocking(move || {
        check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
        let caller = transfer_caller(&headers)?;
        codeterm_service_core::service::transfer_team_plan(
            &state.svc,
            &id,
            body.to,
            &body.reason,
            caller,
            epoch_ms(),
        )
        .map_err(team_plan_err)?;
        Ok(Json(serde_json::json!({ "ok": true })))
    })
    .await
    .map_err(|_| {
        text_err(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "blocking task failed",
        )
    })?
}

fn team_plan_err(error: TeamPlanError) -> (StatusCode, Json<Value>) {
    match error {
        TeamPlanError::Plan(error) => plan_err(error),
        TeamPlanError::Team(message)
            if message.contains("not authorized")
                || message.contains("not a member")
                || message.contains("authority") =>
        {
            text_err(StatusCode::FORBIDDEN, message)
        }
        TeamPlanError::Team(message) => text_err(StatusCode::BAD_REQUEST, message),
    }
}
