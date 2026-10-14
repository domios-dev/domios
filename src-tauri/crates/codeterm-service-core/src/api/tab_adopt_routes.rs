use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use super::routes::{resolve_tab, AppState};
use crate::service::tab_adopt::{
    TabAdoptError, TabAdoptErrorKind, TabAdoptResponse, TAB_ADOPT_PROVIDER_REQUIRED,
};

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TabAdoptRequest {
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    force: bool,
}

#[derive(Debug)]
struct ApiAdoptError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ApiAdoptError {
    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }

    fn response(self) -> (StatusCode, Json<serde_json::Value>) {
        (
            self.status,
            Json(serde_json::json!({
                "ok": false,
                "error": {"code": self.code, "message": self.message},
            })),
        )
    }
}

impl From<TabAdoptError> for ApiAdoptError {
    fn from(error: TabAdoptError) -> Self {
        let status = match error.kind {
            TabAdoptErrorKind::NotFound => StatusCode::NOT_FOUND,
            TabAdoptErrorKind::NotAuthorized => StatusCode::FORBIDDEN,
            TabAdoptErrorKind::LivePtyRequired
            | TabAdoptErrorKind::AgentTuiRequired
            | TabAdoptErrorKind::UnknownProvider
            | TabAdoptErrorKind::ProviderRequired => StatusCode::BAD_REQUEST,
        };
        Self::new(status, error.code(), error.message)
    }
}

type AdoptResult = Result<Json<TabAdoptResponse>, (StatusCode, Json<serde_json::Value>)>;

pub(crate) async fn adopt_agent(
    AxumState(state): AxumState<AppState>,
    Path(raw_tab_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<TabAdoptRequest>,
) -> AdoptResult {
    let result = adopt_agent_inner(state, raw_tab_id, headers, request).await;
    result.map(Json).map_err(ApiAdoptError::response)
}

async fn adopt_agent_inner(
    state: AppState,
    raw_tab_id: String,
    headers: HeaderMap,
    request: TabAdoptRequest,
) -> Result<TabAdoptResponse, ApiAdoptError> {
    let tab_id = resolve_tab(&state, &raw_tab_id).map_err(|(status, message)| {
        ApiAdoptError::new(status, "tab_adopt_target_not_found", message)
    })?;
    let provider = request
        .provider
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ApiAdoptError::new(
                StatusCode::BAD_REQUEST,
                TAB_ADOPT_PROVIDER_REQUIRED,
                "provider is required; adopt never infers agent_type",
            )
        })?;
    let caller = crate::api::tab_routes::authenticated_plan_caller(&headers);
    let svc = state.svc.clone();
    let adopted_tab = tab_id.clone();
    let adopted_provider = provider.to_string();
    let force = request.force;
    tokio::task::spawn_blocking(move || {
        svc.adopt_live_pty_as_agent(&adopted_tab, &caller, &adopted_provider, force)
    })
    .await
    .map_err(|error| {
        ApiAdoptError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "tab_adopt_join_failed",
            error.to_string(),
        )
    })?
    .map_err(ApiAdoptError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_provider_is_refused_at_the_api_boundary() {
        let request = TabAdoptRequest {
            provider: Some("  ".into()),
            force: false,
        };
        assert!(request
            .provider
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none());
    }
}
