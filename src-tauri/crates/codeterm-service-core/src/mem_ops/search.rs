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
    let (result, degraded) = codeterm_mem::embedder::recall_priority::with_recall_status(|| {
        resolve_search(&**svc, &svc.supporter_models, &cfg, p)
    });
    result.map(|result| search_status(result, degraded))
}

fn search_status(mut result: serde_json::Value, degraded: bool) -> serde_json::Value {
    if degraded {
        result["degraded"] = serde_json::json!("semantic pending");
    }
    result
}

#[cfg(test)]
mod tests;
