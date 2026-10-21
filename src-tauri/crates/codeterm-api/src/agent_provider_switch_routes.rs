use super::routes::{check_auth, AppState};
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};

#[derive(serde::Deserialize)]
pub(crate) struct SwitchProviderBody {
    provider: String,
    model: Option<String>,
    reasoning_effort: Option<String>,
    #[serde(default)]
    force: bool,
}

#[derive(serde::Deserialize)]
pub(crate) struct HistoryQuery {
    file: String,
}

pub(crate) async fn provider_history(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(tab): Path<String>,
    Query(query): Query<HistoryQuery>,
) -> Response {
    if let Err((status, message)) = check_auth(&state, &headers) {
        return (status, message).into_response();
    }
    match tokio::task::spawn_blocking(move || state.svc.provider_switch_history(&tab, &query.file))
        .await
    {
        Ok(Ok(messages)) => Json(messages).into_response(),
        Ok(Err(error)) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error":error})),
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error":error.to_string()})),
        )
            .into_response(),
    }
}

pub(crate) async fn switch_provider(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(tab): Path<String>,
    Json(body): Json<SwitchProviderBody>,
) -> Response {
    if let Err((status, message)) = check_auth(&state, &headers) {
        return (status, message).into_response();
    }
    let tab = match crate::routes::resolve_tab_async(&state, &tab).await {
        Ok(tab) => tab,
        Err((status, message)) => return (status, message).into_response(),
    };
    let caller = crate::tab_routes::caller_tab_header(&headers);
    let nonce = headers
        .get("x-codeterm-binding-nonce")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let result = tokio::task::spawn_blocking(move || {
        state.svc.accept_provider_switch(
            &tab,
            codeterm_service_core::service::agent_restart::ProviderSwitchRequest {
                provider: body.provider,
                model: body.model,
                reasoning_effort: body.reasoning_effort,
                force: body.force,
            },
            caller.as_deref(),
            nonce.as_deref(),
        )
    })
    .await;
    match result {
        Ok(Ok(accepted)) => {
            let bytes = axum::body::Bytes::from(serde_json::to_vec(&accepted.status).unwrap());
            (
                StatusCode::ACCEPTED,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                axum::body::Body::from_stream(accepted_response(bytes, accepted.response_complete)),
            )
                .into_response()
        }
        Ok(Err(error)) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error":error})),
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error":error.to_string()})),
        )
            .into_response(),
    }
}

fn accepted_response(
    bytes: axum::body::Bytes,
    complete: tokio::sync::oneshot::Sender<()>,
) -> impl futures_core::Stream<Item = Result<axum::body::Bytes, std::convert::Infallible>> {
    async_stream::stream! { yield Ok(bytes); let _ = complete.send(()); }
}

#[derive(serde::Deserialize)]
pub(crate) struct StatusQuery {
    #[serde(default)]
    wait: bool,
}

pub(crate) async fn switch_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((tab, operation)): Path<(String, String)>,
    Query(query): Query<StatusQuery>,
) -> Response {
    if let Err((status, message)) = check_auth(&state, &headers) {
        return (status, message).into_response();
    }
    match tokio::task::spawn_blocking(move || {
        state
            .svc
            .provider_switch_status(&tab, &operation, query.wait)
    })
    .await
    {
        Ok(Ok(status)) => Json(status).into_response(),
        Ok(Err(error)) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error":error})),
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error":error.to_string()})),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn acceptance_is_emitted_before_daemon_retirement_is_released() {
        use futures_util::StreamExt;
        let (complete, mut completed) = tokio::sync::oneshot::channel();
        let response =
            accepted_response(axum::body::Bytes::from_static(b"opaque-accepted"), complete);
        tokio::pin!(response);
        assert_eq!(response.next().await.unwrap().unwrap(), "opaque-accepted");
        assert!(completed.try_recv().is_err());
        assert!(response.next().await.is_none());
        assert!(completed.await.is_ok());
    }
    #[test]
    fn provider_switch_request_preserves_target_selection_and_stop_choice() {
        let body: SwitchProviderBody = serde_json::from_value(serde_json::json!({"provider":"target-token","model":"model-token","reasoning_effort":"effort-token","force":true})).unwrap();
        assert_eq!(body.provider, "target-token");
        assert_eq!(body.model.as_deref(), Some("model-token"));
        assert_eq!(body.reasoning_effort.as_deref(), Some("effort-token"));
        assert!(body.force);
        assert!(serde_json::from_value::<SwitchProviderBody>(
            serde_json::json!({"model":"model-token"})
        )
        .is_err());
    }
}
