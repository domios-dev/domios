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

use crate::{
    events::SystemEvent,
    routes::{resolve_tab_async, AppState},
};
use codeterm_service_core::multiplexer::commands::MuxState;
use codeterm_service_core::multiplexer::types::TabBacking;
use codeterm_service_core::service::tab_chat::runtime_stored::StoredHistoryFailure;
use codeterm_service_core::service::tab_chat::snapshot_source::{self, SnapshotSource};
use std::{future::Future, time::Duration};
use tokio::{sync::broadcast, time::Instant};

type SnapshotReply = Result<Json<serde_json::Value>, SnapshotError>;
const BINDING_CHANGED: &str = "chat_binding_changed";

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
    let failure = StoredHistoryFailure::from_error(&error);
    if failure == Some(StoredHistoryFailure::BindingChanged) {
        log::debug!("[chat-snapshot] tab={tab_id} error={error}");
    } else {
        log::error!("[chat-snapshot] tab={tab_id} error={error}");
    }
    let (status, code, state) = match failure {
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
        Some(StoredHistoryFailure::BindingChanged) => (
            StatusCode::SERVICE_UNAVAILABLE,
            BINDING_CHANGED,
            "restoring",
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

pub(crate) mod closed;

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
) -> SnapshotReply {
    let tab_id = match resolve_tab_async(&state, &raw_tab_id).await {
        Ok(tab_id) => tab_id,
        Err(refusal) => {
            return tokio::task::spawn_blocking(move || {
                closed::snapshot(&state, &raw_tab_id, query.since, &headers)
                    .unwrap_or_else(|| Err(refusal.into()))
            })
            .await
            .map_err(|error| snapshot_failure("snapshot-worker", error.to_string()))?;
        }
    };
    let events = state.events.subscribe();
    let since = query.since;
    await_snapshot(&tab_id, events, || {
        let state = state.clone();
        let tab_id = tab_id.clone();
        async move {
            tokio::task::spawn_blocking(move || snapshot_blocking(&state, &tab_id, since))
                .await
                .map_err(|error| snapshot_failure("snapshot-worker", error.to_string()))?
        }
    })
    .await
}

async fn await_snapshot<F: Future<Output = SnapshotReply>>(
    tab_id: &str,
    mut events: broadcast::Receiver<crate::events::SeqEvent>,
    mut read: impl FnMut() -> F,
) -> SnapshotReply {
    let mut result = read().await;
    if !binding_changed(&result) {
        return result;
    }
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        match tokio::time::timeout_at(deadline, events.recv()).await {
            Ok(Ok(event)) if !recheck_binding_on(&event.event, tab_id) => continue,
            Ok(Ok(_)) | Ok(Err(broadcast::error::RecvError::Lagged(_))) => {}
            _ => return result,
        }
        match tokio::time::timeout_at(deadline, read()).await {
            Ok(next) if binding_changed(&next) => result = next,
            Ok(next) => return next,
            Err(_) => return result,
        }
    }
}

fn binding_changed(result: &SnapshotReply) -> bool {
    matches!(result, Err(error) if error.1.code == BINDING_CHANGED)
}

fn recheck_binding_on(event: &SystemEvent, tab_id: &str) -> bool {
    match event {
        SystemEvent::Generic { event_type, data }
            if event_type == "agent-runtime-state" && data["tab_id"].as_str() == Some(tab_id) =>
        {
            let state = &data["state"];
            match state["state"].as_str() {
                Some("loaded") => matches!(
                    state["history"].as_str(),
                    Some("complete" | "failed") | None
                ),
                Some("degraded" | "loginPending" | "unloaded") => true,
                _ => false,
            }
        }
        SystemEvent::TabClosed { tab_id: closed } => closed == tab_id,
        _ => false,
    }
}

fn snapshot_blocking(state: &AppState, tab_id: &str, since: usize) -> SnapshotReply {
    if chat_read_target(&state.svc.mux, tab_id)? == ChatReadTarget::Unbound {
        let journal = state.svc.lifecycle_journal.clone();
        let (messages, count) = codeterm_service_core::service::tab_chat::restore_notice_snapshot(
            &journal,
            tab_id,
            Vec::new(),
            since,
        );
        return Ok(Json(
            serde_json::json!({ "messages": messages, "count": count, "pending": true }),
        ));
    }

    let source = snapshot_source::for_tab(&state.svc, tab_id)
        .map_err(|error| snapshot_failure(tab_id, error))?;
    let (provider_id, session_id) = {
        let mux = state.svc.mux.read();
        mux.get_tab(tab_id)
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
        codeterm_service_core::service::tab_chat::tab_chat_snapshot(&state.svc, tab_id, since)
            .map_err(|error| snapshot_failure(tab_id, error))?;

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
