use crate::service::ServiceState;
use axum::http::StatusCode;
pub use codeterm_recall::search::*;

fn search_error_http(error: SearchError) -> (StatusCode, String) {
    match error {
        SearchError::InvalidParams(message) => (StatusCode::BAD_REQUEST, message),
        SearchError::Internal(message) => (StatusCode::INTERNAL_SERVER_ERROR, message),
    }
}

/// Recall and stage 2 block on SQLite and adapter IO, so they never run on the async runtime.
pub async fn run_blocking_search<F>(search: F) -> Result<serde_json::Value, (StatusCode, String)>
where
    F: FnOnce() -> Result<serde_json::Value, SearchError> + Send + 'static,
{
    tokio::task::spawn_blocking(search)
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("mem search task failed: {error}"),
            )
        })?
        .map_err(search_error_http)
}

pub fn execute_search(
    svc: &ServiceState,
    p: SearchParams,
) -> Result<serde_json::Value, SearchError> {
    let cfg = svc.configuration().supporter;
    resolve_search(&**svc, &svc.supporter_models, &cfg, p)
}

#[cfg(test)]
mod tests;
