use crate::service::ServiceState;
use axum::http::StatusCode;
pub use codeterm_recall::search::*;

fn search_error_http(error: SearchError) -> (StatusCode, String) {
    match error {
        SearchError::InvalidParams(message) => (StatusCode::BAD_REQUEST, message),
        SearchError::Internal(message) if message.starts_with("neighbor_") => {
            (StatusCode::FORBIDDEN, message)
        }
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
    if let Some(tab_id) = p.caller_tab_id.as_ref() {
        let binding_nonce = svc
            .mux
            .read()
            .get_tab(tab_id)
            .and_then(|tab| tab.session_binding_nonce.clone())
            .ok_or_else(|| {
                SearchError::Internal("neighbor_forbidden: Caller binding unavailable".into())
            })?;
        let caller = crate::service::tab_credentials::TabCaller {
            tab_id: tab_id.clone(),
            binding_nonce,
        };
        if let Some(areas) = crate::async_runtime::block_on(svc.neighbor_memory_areas(&caller))
            .map_err(SearchError::Internal)?
        {
            return restricted_search(&**svc, p, &areas).map_err(SearchError::Internal);
        }
    }
    let cfg = svc.configuration().supporter;
    let (result, degraded) = codeterm_mem::embedder::recall_priority::with_recall_status(|| {
        resolve_search(&**svc, &svc.supporter_models, &cfg, p)
    });
    result.map(|result| search_status(result, degraded))
}

fn restricted_search<B: RecallBackend + ?Sized>(
    backend: &B,
    mut p: SearchParams,
    areas: &[String],
) -> Result<serde_json::Value, String> {
    let selected: Vec<_> = areas
        .iter()
        .filter(|area| p.area.as_ref().is_none_or(|requested| requested == *area))
        .collect();
    p.project = None;
    p.supporter = Some(false);
    p.rerank = Some(false);
    let limit = p.limit.unwrap_or(20).min(500);
    p.limit = Some(limit);
    let mut hits = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for area in selected {
        p.area = (area != "(none)").then(|| area.clone());
        let result = baseline_search(backend, &p)?;
        for hit in result["hits"].as_array().into_iter().flatten() {
            let actual = hit["item"]["area"]
                .as_str()
                .filter(|area| !area.is_empty())
                .unwrap_or("(none)");
            if actual == area && seen.insert(hit["item"]["id"].clone().to_string()) {
                hits.push(hit.clone());
            }
        }
    }
    hits.sort_by(|left, right| {
        right["score"]
            .as_f64()
            .unwrap_or_default()
            .total_cmp(&left["score"].as_f64().unwrap_or_default())
    });
    hits.truncate(limit);
    Ok(serde_json::json!({"hits": hits}))
}

fn search_status(mut result: serde_json::Value, degraded: bool) -> serde_json::Value {
    if degraded {
        result["degraded"] = serde_json::json!("semantic pending");
    }
    result
}

#[cfg(test)]
mod tests;
