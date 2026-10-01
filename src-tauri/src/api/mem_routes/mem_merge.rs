//! Reversible near-duplicate merge: the lesser is archived and linked to the
//! keeper in one transaction, never deleted; `mem restore <lesser>` undoes it.

use std::path::Path;

use axum::{
    extract::State as AxumState,
    http::{HeaderMap, StatusCode},
    Json,
};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;

use super::calibration::archive::{archive_item, unarchive_item, ARCHIVED_KIND};
use super::calibration::stale::{julian_day, utc_day_start};
use super::items::{pick_restore_body, RestoreBody};
use super::{emit_activity, sanitize_actor, WORKER_ID_HEADER};
use crate::api::routes::{check_auth, AppState};
use crate::mem::graph::{insert_relation, Relation};
use crate::mem::store::{self, with_changed_by, HistoryEntry, MemUpdate};
use crate::service::ServiceState;

pub(crate) const MERGE_CHANGE_TYPE: &str = "merge";
pub(crate) const DREAMER_MERGE_ACTOR: &str = "dreamer-rem:merge";
pub(crate) const MERGED_INTO: &str = "merged_into";
pub(crate) const SUPERSEDES: &str = "supersedes";
pub(crate) const MERGE_RELATION_REASON: &str = "mem-merge";
const PROTECTED_LESSER_KINDS: &[&str] = &["skill", "rule", "preference"];
const MAX_BODY_BYTES: usize = 1_048_576;

#[derive(Deserialize)]
pub struct MergeBody {
    pub keeper: String,
    pub lesser: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub conflict: bool,
    #[serde(default)]
    pub changed_by: Option<String>,
    #[serde(default)]
    pub worker_id: Option<String>,
}

#[derive(Debug)]
pub(crate) enum MergeError {
    Invalid(String),
    NotFound(String),
    Refused(String),
    CapReached(String),
    Internal(String),
}

impl MergeError {
    pub(crate) fn status(&self) -> StatusCode {
        match self {
            Self::Invalid(_) => StatusCode::BAD_REQUEST,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Refused(_) => StatusCode::CONFLICT,
            Self::CapReached(_) => StatusCode::TOO_MANY_REQUESTS,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub(crate) fn into_message(self) -> String {
        match self {
            Self::Invalid(m)
            | Self::NotFound(m)
            | Self::Refused(m)
            | Self::CapReached(m)
            | Self::Internal(m) => m,
        }
    }
}

/// The audit actor is resolved server-side: a dreaming worker always gets the
/// reserved dreamer label, so the nightly cap never depends on the prompt.
pub(crate) fn merge_actor(worker_intent: Option<&str>, changed_by: Option<&str>) -> String {
    match worker_intent {
        Some(intent) if crate::mem::workers::is_dreaming_intent(intent) => {
            DREAMER_MERGE_ACTOR.to_string()
        }
        Some(intent) => format!("worker:{intent}"),
        None => changed_by
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(sanitize_actor)
            .unwrap_or_else(|| "user".to_string()),
    }
}

pub(crate) struct MergeRequest<'a> {
    pub keeper: &'a str,
    pub lesser: &'a str,
    pub body: Option<&'a str>,
    pub conflict: bool,
}

struct ItemRow {
    kind: String,
    scope: String,
    pinned: bool,
}

fn item_row(conn: &Connection, id: &str) -> Result<Option<ItemRow>, MergeError> {
    conn.query_row(
        "SELECT kind, scope, COALESCE(is_pinned_wakeup, 0) FROM mem_items WHERE id = ?1",
        params![id],
        |r| {
            Ok(ItemRow {
                kind: r.get(0)?,
                scope: r.get(1)?,
                pinned: r.get::<_, i64>(2)? != 0,
            })
        },
    )
    .optional()
    .map_err(|e| MergeError::Internal(format!("read {id}: {e}")))
}

pub(crate) fn dreamer_merges_today(conn: &Connection, now: DateTime<Utc>) -> Result<i64, String> {
    conn.query_row(
        "SELECT COUNT(*) FROM mem_item_history
         WHERE change_type = ?1 AND changed_by = ?2 AND julianday(changed_at) >= ?3",
        params![
            MERGE_CHANGE_TYPE,
            DREAMER_MERGE_ACTOR,
            julian_day(utc_day_start(now))
        ],
        |r| r.get(0),
    )
    .map_err(|e| format!("count merges: {e}"))
}

fn validate(req: &MergeRequest) -> Result<(), MergeError> {
    if req.keeper.trim().is_empty() || req.lesser.trim().is_empty() {
        return Err(MergeError::Invalid("keeper and lesser are required".into()));
    }
    if req.keeper == req.lesser {
        return Err(MergeError::Invalid("keeper and lesser must differ".into()));
    }
    match req.body {
        Some(b) if b.trim().is_empty() => Err(MergeError::Invalid("body must not be empty".into())),
        Some(b) if b.len() > MAX_BODY_BYTES => {
            Err(MergeError::Invalid("body too large (max 1 MB)".into()))
        }
        _ => Ok(()),
    }
}

/// One transaction: optional keeper body, lesser archived with a `merge`
/// history row, and the merge relation. Protected and pinned lessers are refused.
pub(crate) fn run_merge(
    conn: &Connection,
    blobs_dir: &Path,
    req: &MergeRequest,
    actor: &str,
    merge_cap: u32,
    now: DateTime<Utc>,
) -> Result<serde_json::Value, MergeError> {
    validate(req)?;
    let lesser = item_row(conn, req.lesser)?
        .ok_or_else(|| MergeError::NotFound(format!("mem item {} not found", req.lesser)))?;
    let keeper = item_row(conn, req.keeper)?
        .ok_or_else(|| MergeError::NotFound(format!("mem item {} not found", req.keeper)))?;
    if lesser.pinned {
        return Err(MergeError::Refused(format!(
            "{} is pinned; a pinned item is never merged away",
            req.lesser
        )));
    }
    if PROTECTED_LESSER_KINDS.contains(&lesser.kind.as_str()) {
        return Err(MergeError::Refused(format!(
            "{} is a {}; skill, rule and preference items are never merged away",
            req.lesser, lesser.kind
        )));
    }
    if lesser.kind == ARCHIVED_KIND || keeper.kind == ARCHIVED_KIND {
        return Err(MergeError::Refused(
            "an archived item cannot take part in a merge; restore it first".into(),
        ));
    }
    if actor == DREAMER_MERGE_ACTOR {
        let done = dreamer_merges_today(conn, now).map_err(MergeError::Internal)?;
        if done >= i64::from(merge_cap) {
            return Err(MergeError::CapReached(format!(
                "nightly merge cap reached: {done} of {merge_cap} dreamer merges today \
                 (memory.dream_hygiene.merge_cap); leave the rest for the next run"
            )));
        }
    }

    let (relation, from, to) = if req.conflict {
        (SUPERSEDES, req.keeper, req.lesser)
    } else {
        (MERGED_INTO, req.lesser, req.keeper)
    };
    conn.execute_batch("BEGIN")
        .map_err(|e| MergeError::Internal(e.to_string()))?;
    let applied = (|| -> Result<bool, String> {
        let body_updated = match req.body {
            Some(body) => with_changed_by(actor, || {
                let patch = MemUpdate {
                    body: Some(body.to_string()),
                    ..Default::default()
                };
                store::update(conn, req.keeper, &patch, blobs_dir)
            })?,
            None => false,
        };
        let reason = format!("{relation} {from} -> {to}");
        archive_item(
            conn,
            req.lesser,
            &lesser.kind,
            actor,
            MERGE_CHANGE_TYPE,
            Some(&reason),
        )?;
        insert_relation(
            conn,
            &Relation {
                from_id: from.to_string(),
                relation_type: relation.to_string(),
                to_id: to.to_string(),
                scope: lesser.scope.clone(),
                valid_from: now.to_rfc3339(),
                valid_to: None,
                confidence: 1.0,
                properties: serde_json::json!({}),
                reason: Some(MERGE_RELATION_REASON.to_string()),
            },
        )?;
        Ok(body_updated)
    })();
    let body_updated = match applied {
        Ok(updated) => {
            conn.execute_batch("COMMIT")
                .map_err(|e| MergeError::Internal(e.to_string()))?;
            updated
        }
        Err(e) => {
            conn.execute_batch("ROLLBACK").ok();
            return Err(MergeError::Internal(e));
        }
    };
    Ok(serde_json::json!({
        "merged": req.lesser,
        "keeper": req.keeper,
        "archived_kind": lesser.kind,
        "relation": { "from": from, "type": relation, "to": to },
        "body_updated": body_updated,
        "changed_by": actor,
    }))
}

/// Shared by the HTTP and socket transports. Config and the worker registry
/// are read before the mem connection lock is taken.
pub(crate) fn merge_items(
    svc: &ServiceState,
    body: &MergeBody,
    worker_id: Option<&str>,
) -> Result<serde_json::Value, MergeError> {
    let intent = worker_id.and_then(|wid| svc.workers.get(wid).map(|w| w.intent));
    let actor = merge_actor(intent.as_deref(), body.changed_by.as_deref());
    let merge_cap = crate::config::memory_config().dream_hygiene.merge_cap;
    let req = MergeRequest {
        keeper: body.keeper.trim(),
        lesser: body.lesser.trim(),
        body: body.body.as_deref(),
        conflict: body.conflict,
    };
    let out = svc.mem.with_conn(|conn| {
        run_merge(
            conn,
            svc.mem.blobs_dir(),
            &req,
            &actor,
            merge_cap,
            Utc::now(),
        )
    })?;
    if out["body_updated"] == true {
        svc.mem.refresh_embedding(req.keeper);
    }
    Ok(out)
}

pub async fn mem_merge(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(b): Json<MergeBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let worker_id = headers
        .get(WORKER_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(String::from)
        .or_else(|| b.worker_id.clone());
    let ids = vec![b.keeper.clone(), b.lesser.clone()];
    let svc = state.svc.clone();
    let out = tokio::task::spawn_blocking(move || merge_items(&svc, &b, worker_id.as_deref()))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("join: {e}")))?
        .map_err(|e| (e.status(), e.into_message()))?;
    emit_activity(&state, &headers, "update", ids);
    Ok(Json(out))
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct KindRestore {
    pub kind: String,
    pub history_id: i64,
}

/// The latest kind change decides: when it archived the item (a calibration
/// archive or a merge), restore brings back the kind it had before.
pub(crate) fn pick_restore_kind(history: &[HistoryEntry]) -> Option<KindRestore> {
    let last = history.iter().rev().find(|h| h.new_kind.is_some())?;
    let archived_here = last.new_kind.as_deref() == Some(ARCHIVED_KIND)
        && matches!(last.change_type.as_str(), "archive" | MERGE_CHANGE_TYPE);
    if !archived_here {
        return None;
    }
    last.old_kind
        .clone()
        .filter(|kind| kind != ARCHIVED_KIND)
        .map(|kind| KindRestore {
            kind,
            history_id: last.id,
        })
}

/// Un-archive in one transaction, optionally restore a body version, and
/// invalidate (never delete) the edge a merge created for this item.
pub(crate) fn apply_kind_restore(
    conn: &Connection,
    blobs_dir: &Path,
    id: &str,
    restore: &KindRestore,
    body: Option<&str>,
    actor: &str,
) -> Result<serde_json::Value, String> {
    conn.execute_batch("BEGIN").map_err(|e| e.to_string())?;
    let reason = format!("undo history row {}", restore.history_id);
    let applied = (|| -> Result<usize, String> {
        unarchive_item(conn, id, &restore.kind, actor, Some(&reason))?;
        if let Some(body) = body {
            with_changed_by(actor, || {
                let patch = MemUpdate {
                    body: Some(body.to_string()),
                    ..Default::default()
                };
                store::update(conn, id, &patch, blobs_dir)
            })?;
        }
        conn.execute(
            "UPDATE relations SET valid_to = ?1
             WHERE valid_to IS NULL AND reason = ?2
               AND ((from_id = ?3 AND relation_type = ?4)
                 OR (to_id = ?3 AND relation_type = ?5))",
            params![
                Utc::now().to_rfc3339(),
                MERGE_RELATION_REASON,
                id,
                MERGED_INTO,
                SUPERSEDES
            ],
        )
        .map_err(|e| format!("invalidate merge relation: {e}"))
    })();
    match applied {
        Ok(invalidated) => {
            conn.execute_batch("COMMIT").map_err(|e| e.to_string())?;
            Ok(serde_json::json!({
                "restored": id,
                "kind": restore.kind,
                "body_restored": body.is_some(),
                "invalidated_relations": invalidated,
            }))
        }
        Err(e) => {
            conn.execute_batch("ROLLBACK").ok();
            Err(e)
        }
    }
}

/// `mem restore` for both transports: an archived item gets its kind back
/// (and the body of `version` when given); otherwise the latest body change is undone.
pub(crate) fn restore_item(
    svc: &ServiceState,
    id: &str,
    body: &RestoreBody,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let internal = |e: String| (StatusCode::INTERNAL_SERVER_ERROR, e);
    let history = svc.mem.get_item_history(id).map_err(internal)?;
    if history.is_empty() {
        return Err((StatusCode::NOT_FOUND, format!("no history for {}", id)));
    }
    let actor = body
        .changed_by
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(sanitize_actor)
        .unwrap_or_else(|| "restore".to_string());

    if let Some(kind) = pick_restore_kind(&history) {
        let restored_body = body
            .version
            .map(|v| pick_restore_body(&history, Some(v)))
            .transpose()?;
        let out = svc
            .mem
            .with_conn(|conn| {
                apply_kind_restore(
                    conn,
                    svc.mem.blobs_dir(),
                    id,
                    &kind,
                    restored_body.as_deref(),
                    &actor,
                )
            })
            .map_err(internal)?;
        svc.mem.refresh_embedding(id);
        return Ok(out);
    }

    let restored_body = pick_restore_body(&history, body.version)?;
    let patch = MemUpdate {
        body: Some(restored_body.clone()),
        ..Default::default()
    };
    let updated = with_changed_by(&actor, || svc.mem.update(id, patch)).map_err(internal)?;
    if !updated {
        return Err((StatusCode::NOT_FOUND, format!("mem item {} not found", id)));
    }
    Ok(serde_json::json!({ "restored": id, "body_len": restored_body.len() }))
}

#[cfg(test)]
mod tests;
