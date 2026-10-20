use axum::{
    extract::{Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};

use crate::routes::{check_auth, AppState};
use codeterm_service_core::mem_ops::search::{execute_search, run_blocking_search, SearchParams};

pub async fn mem_search(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(p): Query<SearchParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    if p.q.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "search query must not be empty".into(),
        ));
    }
    let svc = state.svc.clone();
    let p = SearchParams {
        caller_tab_id: crate::tab_routes::caller_tab_header(&headers),
        ..p
    };
    run_blocking_search(move || execute_search(&svc, p))
        .await
        .map(Json)
}
