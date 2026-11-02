use axum::{
    extract::{Query, State as AxumState},
    http::StatusCode,
    Json,
};

use crate::routes::AppState;
use codeterm_service_core::mem_ops::search::{execute_search, run_blocking_search, SearchParams};

pub async fn mem_search(
    AxumState(state): AxumState<AppState>,
    headers: crate::auth::AgentHeaders,
    Query(p): Query<SearchParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    if p.q.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "search query must not be empty".into(),
        ));
    }
    let svc = state.svc.clone();
    let p = SearchParams {
        caller_tab_id: headers.local_tab_id(),
        ..p
    };
    run_blocking_search(move || execute_search(&svc, p))
        .await
        .map(Json)
}
