//! Reads of closed agent tabs. A closed read answers only the owner, the tab's
//! parent and its Team; a mesh peer or remote agent is refused, as a closed tab
//! has no live ownership for peer authority to bind to.

use axum::{
    extract::{Path, Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use codeterm_service_core::multiplexer::state::ClosedTranscript;
use codeterm_service_core::service::tab_chat::closed_transcript;
use serde::Deserialize;

use super::{snapshot_failure, SnapshotError, SnapshotErrorBody};
use crate::auth::RequestActor;
use crate::routes::AppState;

/// Newest closed children a parent listing returns.
const CLOSED_CHILDREN_LIMIT: usize = 200;

const REFUSED: &str = "A closed tab's transcript is readable by its parent, its Team and the owner";

enum Reader {
    Owner,
    Agent {
        tab_id: String,
        team_id: Option<String>,
    },
    Other,
}

fn reader(state: &AppState, headers: &HeaderMap) -> Reader {
    match RequestActor::authenticate(state, headers) {
        Ok(RequestActor::Owner) => Reader::Owner,
        Ok(RequestActor::LocalAgent(caller)) => {
            let team_id = state
                .svc
                .mux
                .read()
                .get_tab(&caller.tab_id)
                .and_then(|tab| tab.team_id.clone());
            Reader::Agent {
                tab_id: caller.tab_id,
                team_id,
            }
        }
        _ => Reader::Other,
    }
}

fn may_read(reader: &Reader, record: &ClosedTranscript) -> bool {
    match reader {
        Reader::Owner => true,
        Reader::Agent { tab_id, team_id } => record.readable_by(tab_id, team_id.as_deref()),
        Reader::Other => false,
    }
}

/// A listing names children, so it needs the owner, the parent itself or its Team.
fn may_list(reader: &Reader, parent: &str, parent_team: Option<&str>) -> bool {
    match reader {
        Reader::Owner => true,
        Reader::Agent { tab_id, team_id } => {
            tab_id == parent
                || team_id
                    .as_deref()
                    .is_some_and(|team| !team.is_empty() && Some(team) == parent_team)
        }
        Reader::Other => false,
    }
}

fn refused() -> SnapshotError {
    SnapshotError(
        StatusCode::FORBIDDEN,
        SnapshotErrorBody {
            code: "closed_tab_read_refused",
            state: "refused",
            error: REFUSED.into(),
        },
    )
}

fn transcript_body(
    state: &AppState,
    record: &ClosedTranscript,
    since: usize,
) -> Result<serde_json::Value, SnapshotError> {
    let read = closed_transcript::snapshot(&state.svc, record, since)
        .map_err(|error| snapshot_failure(&record.tab_id, error))?;
    Ok(serde_json::json!({
        "messages": read.messages,
        "count": read.count,
        "pending": false,
        "state": "closed",
        "closed": true,
        "truncated": read.truncated,
        "tab": record,
    }))
}

/// `None` when no closed tab answers to this id, leaving the caller's not-found.
pub(super) fn snapshot(
    state: &AppState,
    raw_tab_id: &str,
    since: usize,
    headers: &HeaderMap,
) -> Option<Result<Json<serde_json::Value>, SnapshotError>> {
    let closed = state.svc.mux.read().closed_transcripts();
    let record = match closed.by_tab(raw_tab_id) {
        Ok(record) => record?,
        Err(error) => return Some(Err(snapshot_failure(raw_tab_id, error))),
    };
    if !may_read(&reader(state, headers), &record) {
        return Some(Err(refused()));
    }
    Some(transcript_body(state, &record, since).map(Json))
}

/// The closed record `sessions get` may disclose for a session or closed tab id:
/// a tab that reopened is live again, and an unauthorized caller learns nothing.
pub(crate) fn session_record(
    state: &AppState,
    key: &str,
    headers: &HeaderMap,
) -> Option<ClosedTranscript> {
    let closed = state.svc.mux.read().closed_transcripts();
    let record = match closed.by_session(key) {
        Ok(Some(record)) => record,
        Ok(None) => closed.by_tab(key).ok()??,
        Err(error) => {
            log::warn!("[closed-history] session lookup failed: {error}");
            return None;
        }
    };
    if state.svc.mux.read().get_tab(&record.tab_id).is_some() {
        return None;
    }
    may_read(&reader(state, headers), &record).then_some(record)
}

#[derive(Deserialize)]
pub struct ClosedChildrenQuery {
    #[serde(default)]
    child: Option<String>,
    #[serde(default)]
    since: usize,
}

/// A parent's closed children, or with `child` that one child's transcript.
pub async fn closed_children(
    AxumState(state): AxumState<AppState>,
    Path(raw_parent): Path<String>,
    Query(query): Query<ClosedChildrenQuery>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, SnapshotError> {
    tokio::task::spawn_blocking(move || closed_children_blocking(state, raw_parent, query, headers))
        .await
        .map_err(|error| snapshot_failure("closed-children-worker", error.to_string()))?
}

fn closed_children_blocking(
    state: AppState,
    raw_parent: String,
    query: ClosedChildrenQuery,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, SnapshotError> {
    let (live_parent, closed) = {
        let mux = state.svc.mux.read();
        let parent = mux.resolve_tab_id(&raw_parent).map(|id| {
            let team = mux.get_tab(&id).and_then(|tab| tab.team_id.clone());
            (id, team)
        });
        (parent, mux.closed_transcripts())
    };
    let (parent, parent_team) = match live_parent {
        Some(parent) => parent,
        None => {
            let record = closed
                .by_tab(&raw_parent)
                .map_err(|error| snapshot_failure(&raw_parent, error))?
                .ok_or((StatusCode::NOT_FOUND, "Tab not found"))?;
            (record.tab_id, record.team_id)
        }
    };
    if !may_list(&reader(&state, &headers), &parent, parent_team.as_deref()) {
        return Err(refused());
    }
    if let Some(child) = query.child {
        let record = closed
            .by_tab(&child)
            .map_err(|error| snapshot_failure(&child, error))?
            .filter(|record| record.parent_tab_id.as_deref() == Some(parent.as_str()))
            .ok_or((StatusCode::NOT_FOUND, "Closed child not found"))?;
        return transcript_body(&state, &record, query.since).map(Json);
    }
    let children = closed
        .children(&parent, CLOSED_CHILDREN_LIMIT)
        .map_err(|error| snapshot_failure(&parent, error))?;
    Ok(Json(serde_json::json!({
        "parent_tab_id": parent,
        "count": children.len(),
        "children": children,
    })))
}

#[cfg(test)]
mod tests;
