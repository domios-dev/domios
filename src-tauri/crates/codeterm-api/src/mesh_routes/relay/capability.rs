use crate::routes::AppState;
use axum::http::StatusCode;
use codeterm_service_core::service::tab_credentials::TabCaller;

pub(super) async fn authorize(
    state: &AppState,
    caller: &TabCaller,
    _method: &str,
    _path: &str,
) -> Result<(), (StatusCode, String)> {
    state
        .svc
        .authorize_neighbor_capability(
            caller,
            codeterm_service_core::service::neighbors::capabilities::Capability::Denied,
        )
        .await
        .map_err(|message| (StatusCode::FORBIDDEN, message))
}
