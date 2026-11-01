//! Stateless transcript read for a tab: `GET /api/tabs/:id/chat?since=N`.
//!
//! The only non-UI reader of chat history (CLI `tab chat`, mesh RPC). Resolves
//! the transcript from the tab's OWN session record; an unbound tab reads as an
//! honest empty transcript and never widens to a neighbouring session.

use axum::{
    extract::{Path, Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};

use crate::routes::{check_auth, resolve_tab, AppState};
use codeterm_service_core::multiplexer::commands::MuxState;
use codeterm_service_core::multiplexer::types::TabBacking;
use codeterm_service_core::service::tab_chat::snapshot_source::{self, SnapshotSource};

#[derive(Debug, Serialize)]
pub struct SnapshotErrorBody {
    code: &'static str,
    state: &'static str,
    error: String,
}

#[derive(Debug)]
pub struct SnapshotError(StatusCode, SnapshotErrorBody);

impl From<(StatusCode, &'static str)> for SnapshotError {
    fn from((status, error): (StatusCode, &'static str)) -> Self {
        Self(
            status,
            SnapshotErrorBody {
                code: "chat_snapshot_refused",
                state: "refused",
                error: error.into(),
            },
        )
    }
}

impl IntoResponse for SnapshotError {
    fn into_response(self) -> Response {
        (self.0, Json(self.1)).into_response()
    }
}

fn snapshot_failure(tab_id: &str, error: String) -> SnapshotError {
    log::error!("[chat-snapshot] tab={} error={}", tab_id, error);
    use codeterm_service_core::service::tab_chat::runtime_stored::StoredHistoryFailure;
    let (status, code, state) = match StoredHistoryFailure::from_error(&error) {
        Some(StoredHistoryFailure::Missing) => {
            (StatusCode::NOT_FOUND, "chat_history_missing", "missing")
        }
        Some(StoredHistoryFailure::Corrupt) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "chat_history_corrupt",
            "corrupt",
        ),
        Some(StoredHistoryFailure::Unavailable) => (
            StatusCode::SERVICE_UNAVAILABLE,
            "chat_history_unavailable",
            "unavailable",
        ),
        None => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "chat_snapshot_failed",
            "failed",
        ),
    };
    SnapshotError(status, SnapshotErrorBody { code, state, error })
}

#[cfg(test)]
#[path = "tab_chat_routes/sleeping_tests.rs"]
mod sleeping_tests;

#[derive(Deserialize)]
pub struct ChatQuery {
    #[serde(default)]
    pub since: usize,
}

/// What a transcript read may honestly resolve for a tab.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ChatReadTarget {
    /// No session belongs to this tab yet — read as empty, never search for one.
    Unbound,
    /// The tab owns a session record; read that exact session.
    OwnedSession,
}

pub(crate) fn chat_read_target(
    mux: &MuxState,
    tab_id: &str,
) -> Result<ChatReadTarget, (StatusCode, &'static str)> {
    let m = mux.read();
    let tab = m
        .get_tab(tab_id)
        .ok_or((StatusCode::NOT_FOUND, "Tab not found"))?;
    match &tab.backing {
        TabBacking::ChatBackend { .. } => Ok(ChatReadTarget::OwnedSession),
        TabBacking::Surface(_) => Ok(ChatReadTarget::Unbound),
        TabBacking::Pty => {
            if tab.agent_type.is_some() && tab.agent_session_id.is_some() {
                Ok(ChatReadTarget::OwnedSession)
            } else {
                Ok(ChatReadTarget::Unbound)
            }
        }
    }
}

pub async fn snapshot(
    AxumState(state): AxumState<AppState>,
    Path(raw_tab_id): Path<String>,
    Query(query): Query<ChatQuery>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, SnapshotError> {
    check_auth(&state, &headers)?;
    tokio::task::spawn_blocking(move || snapshot_blocking(state, raw_tab_id, query))
        .await
        .map_err(|error| snapshot_failure("snapshot-worker", error.to_string()))?
}

fn snapshot_blocking(
    state: AppState,
    raw_tab_id: String,
    query: ChatQuery,
) -> Result<Json<serde_json::Value>, SnapshotError> {
    let tab_id = resolve_tab(&state, &raw_tab_id)?;
    if chat_read_target(&state.svc.mux, &tab_id)? == ChatReadTarget::Unbound {
        let since = query.since;
        let journal = state.svc.lifecycle_journal.clone();
        let (messages, count) = codeterm_service_core::service::tab_chat::restore_notice_snapshot(
            &journal,
            &tab_id,
            Vec::new(),
            since,
        );
        return Ok(Json(
            serde_json::json!({ "messages": messages, "count": count, "pending": true }),
        ));
    }

    let since = query.since;
    let source = snapshot_source::for_tab(&state.svc, &tab_id)
        .map_err(|error| snapshot_failure(&tab_id, error))?;
    let (provider_id, session_id) = {
        let mux = state.svc.mux.read();
        mux.get_tab(&tab_id)
            .map(|tab| match &tab.backing {
                TabBacking::ChatBackend {
                    plugin_id,
                    session_id,
                } => (plugin_id.clone(), session_id.clone()),
                TabBacking::Pty => (
                    tab.agent_type.clone().unwrap_or_default(),
                    tab.agent_session_id.clone().unwrap_or_default(),
                ),
                TabBacking::Surface(surface) => (surface.plugin_id.clone(), "-".to_string()),
            })
            .unwrap_or_else(|| ("-".to_string(), "-".to_string()))
    };
    log::info!(
        "[delivery-timeline] event=transcript-query tab={} provider={} session={} since={}",
        tab_id,
        provider_id,
        session_id,
        since,
    );
    let (messages, count) =
        codeterm_service_core::service::tab_chat::tab_chat_snapshot(&state.svc, &tab_id, since)
            .map_err(|error| snapshot_failure(&tab_id, error))?;

    if source == SnapshotSource::Waking {
        return Ok(Json(serde_json::json!({
            "messages": messages, "count": count, "pending": true, "state": "waking",
        })));
    }

    Ok(Json(
        serde_json::json!({ "messages": messages, "count": count, "pending": false }),
    ))
}

#[cfg(test)]
mod tests;
