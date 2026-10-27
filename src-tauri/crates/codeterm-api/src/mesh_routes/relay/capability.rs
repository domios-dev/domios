use crate::routes::AppState;
use axum::http::StatusCode;
use codeterm_service_core::service::tab_credentials::TabCaller;

pub(super) async fn authorize(
    state: &AppState,
    caller: &TabCaller,
    method: &str,
    path: &str,
) -> Result<(), (StatusCode, String)> {
    if let Some(capability) =
        codeterm_service_core::service::neighbors::capabilities::http_capability(
            &method.to_ascii_uppercase(),
            path.split('?').next().unwrap_or(path),
        )
    {
        state
            .svc
            .authorize_neighbor_capability(caller, capability)
            .await
            .map_err(|message| (StatusCode::FORBIDDEN, message))?;
    }
    Ok(())
}
