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

/// Evidence snapshot of the desktop `mux-state-changed` emit path. The
/// event-freeze probe reads this to classify a stall: `emit_requests` climbing
/// while `emit_fires` stays flat and `pending` is stuck true (with a large
/// `pending_since_ms`) is the emit-side latch; `emit_fires` climbing while the
/// client `__ctEvents.received` stays flat is Tauri channel death.
pub async fn event_stats(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    use std::sync::atomic::Ordering;
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let st = &state.svc.mux_emit_stats;
    let pending = state.svc.mux_emit_pending.load(Ordering::Acquire);
    let since = st.pending_since_ms.load(Ordering::Relaxed);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    Ok(Json(serde_json::json!({
        "emit_requests": st.requests.load(Ordering::Relaxed),
        "emit_fires": st.fires.load(Ordering::Relaxed),
        "emit_suppressed": st.suppressed.load(Ordering::Relaxed),
        "pending": pending,
        "frozen": st.frozen.load(Ordering::Acquire),
        "pending_since_ms": if pending && since > 0 { now.saturating_sub(since) } else { 0 },
    })))
}

/// Fault injection (debug builds only): latch `mux_emit_pending` true WITHOUT a
/// reset thread, reproducing the real failure where a coalesce-thread spawn dies
/// and every future `emit_state_changed` early-returns. Proves the mechanism
/// deterministically instead of waiting for the soak to exhaust OS threads.
pub async fn force_latch(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    use std::sync::atomic::Ordering;
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    if !cfg!(debug_assertions) {
        return Err((StatusCode::CONFLICT, "debug builds only".to_string()));
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    state
        .svc
        .mux_emit_stats
        .frozen
        .store(true, Ordering::Release);
    state.svc.mux_emit_pending.store(true, Ordering::Release);
    state
        .svc
        .mux_emit_stats
        .pending_since_ms
        .store(now, Ordering::Relaxed);
    Ok(Json(serde_json::json!({"latched": true})))
}

/// Clear a forced (or real) latch so the emit path recovers — used to demonstrate
/// that de-latching restores desktop event delivery.
pub async fn reset_latch(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    use std::sync::atomic::Ordering;
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    state
        .svc
        .mux_emit_stats
        .frozen
        .store(false, Ordering::Release);
    state.svc.mux_emit_pending.store(false, Ordering::Release);
    state
        .svc
        .mux_emit_stats
        .pending_since_ms
        .store(0, Ordering::Relaxed);
    Ok(Json(serde_json::json!({"latched": false})))
}

pub async fn clean_restart(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    if !cfg!(debug_assertions) {
        return Err((
            StatusCode::CONFLICT,
            "clean restart is only available in debug builds".to_string(),
        ));
    }
    let app = state.svc.app_handle.clone();
    log::info!("[shutdown] clean-restart request accepted");
    tokio::task::spawn_blocking(move || {
        crate::crash::shutdown::mark_shutting_down_for(
            crate::crash::lifecycle::ShutdownReason::Restart,
        );
        log::info!("[shutdown] requesting Tauri app exit");
        app.exit(0);
        log::info!("[shutdown] Tauri app exit request returned");
    });
    Ok(Json(serde_json::json!({"accepted": true})))
}
