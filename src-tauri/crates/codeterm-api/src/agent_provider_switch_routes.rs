use super::routes::{check_auth, AppState};
use axum::{
    extract::{Path, State},
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
    match tokio::task::spawn_blocking(move || {
        state.svc.switch_provider_in_place(
            &tab,
            &body.provider,
            body.model,
            body.reasoning_effort,
            body.force,
        )
    })
    .await
    {
        Ok(Ok(outcome)) => Json(outcome).into_response(),
        Ok(Err(error)) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"ok":false,"error":error})),
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"ok":false,"error":error.to_string()})),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
