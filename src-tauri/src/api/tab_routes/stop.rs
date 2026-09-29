use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};

use crate::api::routes::{check_auth, resolve_tab, AppState};
use crate::service::tab_stop::StopDecision;

use super::resolve_pty_id_with_fallback;

/// Interrupts the agent's turn in progress (not a clean exit) with the
/// provider's declared stop keystroke. An agent with no turn in progress gets
/// no keys and `{stopped:false, reason:"not_working"}`; 404 when the PTY is gone.
pub async fn stop_tab(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_tab_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let tab_id = resolve_tab(&state, &raw_tab_id)?;
    let svc = state.svc.clone();
    let decided_tab = tab_id.clone();
    let decision = tokio::task::spawn_blocking(move || svc.stop_decision(&decided_tab))
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Stop decision failed"))?;
    let (aid, session_id) = match decision {
        StopDecision::NoAgent => {
            return Ok(Json(serde_json::json!({"ok": true, "stopped": false})));
        }
        StopDecision::NotWorking(status) => {
            return Ok(Json(serde_json::json!({
                "ok": true,
                "stopped": false,
                "reason": "not_working",
                "status": status.label(),
            })));
        }
        StopDecision::Interrupt {
            agent_id,
            session_id,
        } => (agent_id, session_id),
    };
    if let Some(sid) = session_id {
        let svc = state.svc.clone();
        let tab = tab_id.clone();
        let structured_abort = tokio::task::spawn_blocking(move || {
            crate::service::structured_prompt::abort_session(&svc, &tab, &sid)
        })
        .await
        .ok()
        .flatten();
        if let Some(Ok(())) = structured_abort {
            if let Ok(pty_id) = resolve_pty_id_with_fallback(&state, &raw_tab_id) {
                state.svc.mark_agent_interrupted(&pty_id);
            }
            return Ok(Json(
                serde_json::json!({"ok": true, "stopped": true, "structured_abort": true}),
            ));
        }
    }
    let key: String = state
        .svc
        .agent_registry
        .with_provider(&aid, |p| p.stop_keystroke().map(|s| s.to_string()))
        .flatten()
        .unwrap_or_else(|| "\x1b".to_string());
    if let Ok(pty_id) = resolve_pty_id_with_fallback(&state, &raw_tab_id) {
        state.svc.mark_agent_interrupted(&pty_id);
    }
    state
        .svc
        .write_raw(&tab_id, key.as_bytes())
        .map_err(|_| (StatusCode::NOT_FOUND, "Tab not found or has no PTY"))?;
    Ok(Json(serde_json::json!({"ok": true, "stopped": true})))
}
