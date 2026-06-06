//! Test-only debug surface. Routes exist in all builds for honest 409s,
//! but the capabilities behind them only activate in debug builds started
//! with the matching env gate (see startup/servers.rs).

use axum::{
    extract::State as AxumState,
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, AppState};

pub async fn clock_status(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let now_ms = state.cron.lock().now_ms();
    Ok(Json(serde_json::json!({
        "mocked": state.test_clock.is_some(),
        "now_ms": now_ms,
    })))
}

#[derive(Deserialize)]
pub(crate) struct AdvanceBody {
    ms: u64,
}

/// Move the mock clock forward and wake the cron scheduler so overdue
/// jobs fire immediately instead of after its real-time sleep.
pub async fn clock_advance(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<AdvanceBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let clock = state.test_clock.as_ref().ok_or((
        StatusCode::CONFLICT,
        "clock is not mocked — start a debug build with CODETERM_TEST_CLOCK=1".to_string(),
    ))?;
    let now_ms = clock.advance_ms(body.ms);
    state.cron_wake.notify_all();
    Ok(Json(serde_json::json!({"now_ms": now_ms})))
}
