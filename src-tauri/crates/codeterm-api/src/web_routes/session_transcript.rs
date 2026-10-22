use crate::routes::{check_auth, AppState};
use axum::{
    extract::{Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct SessionTranscriptQuery {
    pub provider: String,
    pub cwd: String,
    pub session_id: String,
}

/// Read-only transcript of a finished run (hidden worker / cron). Renders the
/// same normalized ChatMsg shape as the live chat-ui, parsed straight from the
/// session jsonl — no tab, no tailer, no composer.
pub async fn session_transcript(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(q): Query<SessionTranscriptQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let svc = state.svc.clone();
    let (messages, total) = tokio::task::spawn_blocking(move || {
        let (provider, cwd) = if q.provider.is_empty() {
            let session = svc
                .agent_registry
                .find_session(&q.session_id)
                .ok_or_else(|| "Run session is unavailable".to_string())?;
            (session.provider, session.project_path.unwrap_or(q.cwd))
        } else {
            (q.provider, q.cwd)
        };
        codeterm_service_core::service::tab_chat::session_chat_snapshot(
            &svc,
            &provider,
            &cwd,
            &q.session_id,
            0,
        )
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map_err(|e| (StatusCode::NOT_FOUND, e))?;
    Ok(Json(
        serde_json::json!({ "messages": messages, "total": total }),
    ))
}
