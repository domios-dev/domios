//! Stateless transcript read for a tab: `GET /api/tabs/:id/chat?since=N`.
//!
//! The only non-UI reader of chat history (CLI `tab chat`, mesh RPC). Resolves
//! the transcript from the tab's OWN session record; an unbound tab reads as an
//! honest empty transcript and never widens to a neighbouring session.

use axum::{
    extract::{Path, Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::routes::{check_auth, resolve_tab, AppState};
use codeterm_service_core::multiplexer::commands::MuxState;
use codeterm_service_core::multiplexer::types::TabBacking;

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
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    tokio::task::spawn_blocking(move || snapshot_blocking(state, raw_tab_id, query))
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "chat snapshot cancelled"))?
}

fn snapshot_blocking(
    state: AppState,
    raw_tab_id: String,
    query: ChatQuery,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
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
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "chat snapshot failed"))?;

    Ok(Json(
        serde_json::json!({ "messages": messages, "count": count, "pending": false }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use codeterm_service_core::multiplexer::state::MultiplexerState;
    use codeterm_util::lock_hold::TimedRwLock;
    use std::sync::Arc;

    fn shared_mux() -> (MuxState, String) {
        let (state, tab_id) = MultiplexerState::new();
        (Arc::new(TimedRwLock::new("mux-test", state)), tab_id)
    }

    #[test]
    fn transcript_contention_leaves_the_runtime_available() {
        use codeterm_service_core::service::test_fixture::ServiceFixture;
        use std::{future::Future, sync::mpsc, task::Poll, time::Duration};

        let (mux, tab_id) = MultiplexerState::new();
        let fixture = ServiceFixture::new(mux);
        let state = crate::transport_tabs::fixture::app_state(&fixture);
        let mux = state.svc.mux.clone();
        let (held_tx, held_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let holder = std::thread::spawn(move || {
            let guard = mux.write();
            held_tx.send(()).unwrap();
            let released = release_rx.recv_timeout(Duration::from_secs(2)).is_ok();
            drop(guard);
            released
        });
        held_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let response = runtime.block_on(async {
            let mut request = Box::pin(snapshot(
                AxumState(state),
                Path(tab_id),
                Query(ChatQuery { since: 0 }),
                HeaderMap::new(),
            ));
            std::future::poll_fn(|cx| {
                assert!(request.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
            release_tx.send(()).unwrap();
            request.await.unwrap()
        });
        assert!(holder.join().unwrap());
        assert_eq!(
            response.0,
            serde_json::json!({ "messages": [], "count": 0, "pending": true })
        );
    }

    #[test]
    fn unknown_tab_is_not_found_rather_than_another_tabs_transcript() {
        let (mux, _tab_id) = shared_mux();
        let err = chat_read_target(&mux, "no-such-tab").unwrap_err();
        assert_eq!(err.0, StatusCode::NOT_FOUND);
    }

    #[test]
    fn unbound_tab_reads_empty_even_when_a_sibling_owns_a_session() {
        let (mux, tab_id) = shared_mux();
        let sibling = mux.write().create_tab_record();
        {
            let mut m = mux.write();
            let owner = m.get_tab_mut(&sibling).unwrap();
            owner.agent_type = Some("provider-a".into());
            owner.agent_session_id = Some("sid-owned-by-sibling".into());
        }
        assert_eq!(
            chat_read_target(&mux, &tab_id).unwrap(),
            ChatReadTarget::Unbound
        );
    }

    #[test]
    fn agent_tab_without_a_bound_session_reads_empty_not_pending_forever() {
        let (mux, tab_id) = shared_mux();
        {
            let mut m = mux.write();
            let tab = m.get_tab_mut(&tab_id).unwrap();
            tab.agent_type = Some("provider-a".into());
            tab.agent_session_id = None;
        }
        assert_eq!(
            chat_read_target(&mux, &tab_id).unwrap(),
            ChatReadTarget::Unbound
        );
    }

    #[test]
    fn tab_owning_a_session_resolves_that_session() {
        let (mux, tab_id) = shared_mux();
        {
            let mut m = mux.write();
            let tab = m.get_tab_mut(&tab_id).unwrap();
            tab.agent_type = Some("provider-a".into());
            tab.agent_session_id = Some("sid-owned".into());
        }
        assert_eq!(
            chat_read_target(&mux, &tab_id).unwrap(),
            ChatReadTarget::OwnedSession
        );
    }

    #[test]
    fn chat_backend_tab_resolves_its_own_backing_session() {
        let (mux, tab_id) = shared_mux();
        {
            let mut m = mux.write();
            let tab = m.get_tab_mut(&tab_id).unwrap();
            tab.backing = TabBacking::ChatBackend {
                plugin_id: "plugin-a".into(),
                session_id: "sid-backing".into(),
            };
        }
        assert_eq!(
            chat_read_target(&mux, &tab_id).unwrap(),
            ChatReadTarget::OwnedSession
        );
    }
}
