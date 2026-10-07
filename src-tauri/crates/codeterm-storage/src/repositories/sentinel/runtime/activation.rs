use super::{check_batch, corrupt, fenced, finished, live, touch};
use crate::error::{sqlite_error, StorageError};
use crate::owner::WriteCx;
use crate::repositories::sentinel::records::{
    BatchFence, Fence, MutationOutcome, SentinelCommit, SentinelId,
};
use crate::repositories::to_sql_u64;
use rusqlite::{params, Connection, OptionalExtension};

type ScopeRow = (String, String, Option<String>);

fn scope_row(conn: &Connection, id: &str) -> Result<Option<ScopeRow>, StorageError> {
    conn.prepare_cached("SELECT creator_tab_id, workspace_id, team_id FROM sentinels WHERE id = ?1")
        .and_then(|mut statement| {
            statement
                .query_row([id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
                .optional()
        })
        .map_err(|e| sqlite_error("read sentinel scope", e))
}

pub(super) fn activate_target(
    cx: &mut WriteCx<'_>,
    source: &SentinelId,
    fence: &BatchFence,
    target: &SentinelId,
    active_limit: usize,
    input_due_at: Option<u64>,
) -> Result<SentinelCommit, StorageError> {
    let (source, target) = (source.as_str(), target.as_str());
    if let Some(refused) = check_batch(cx.conn, source, fence)? {
        return fenced(cx.conn, refused, source);
    }
    let claimed: i64 = cx
        .conn
        .prepare_cached(
            "SELECT COUNT(*) FROM sentinel_handler_results r \
             JOIN sentinel_transitions t \
               ON t.sentinel_id = r.sentinel_id AND t.transition_id = r.transition_id \
             JOIN sentinel_heads h ON h.sentinel_id = r.sentinel_id \
             JOIN sentinel_handlers hd ON hd.sentinel_id = h.sentinel_id \
               AND hd.version = h.version AND hd.handler_id = r.handler_id \
               AND hd.bucket = t.bucket \
             WHERE r.sentinel_id = ?1 AND r.transition_id = ?2 AND hd.kind = 'sentinel' \
               AND hd.target_ref = ?3 AND r.claim_id IS NOT NULL AND r.status = 'unconfirmed'",
        )
        .and_then(|mut statement| {
            statement.query_row(params![source, fence.transition_id, target], |row| {
                row.get(0)
            })
        })
        .map_err(|e| sqlite_error("read activation claim", e))?;
    if claimed == 0 {
        return fenced(cx.conn, Fence::NotClaimed, source);
    }
    let Some(target_scope) = scope_row(cx.conn, target)? else {
        return Ok(SentinelCommit::fenced(Fence::Missing, None));
    };
    if source == target || scope_row(cx.conn, source)? != Some(target_scope) {
        return fenced(cx.conn, Fence::Scope, target);
    }
    let Some(state) = live(cx.conn, target)? else {
        return Err(corrupt("stage has no runtime"));
    };
    if state.stopped {
        return fenced(cx.conn, Fence::Stopped, target);
    }
    if !state.paused {
        return finished(cx.conn, MutationOutcome::Unchanged, &[target.to_string()]);
    }
    let active: i64 = cx
        .conn
        .query_row(
            "SELECT COUNT(*) FROM sentinel_runtime WHERE stopped = 0 AND paused = 0",
            [],
            |row| row.get(0),
        )
        .map_err(|e| sqlite_error("count active sentinels", e))?;
    if usize::try_from(active).map_err(|_| corrupt("negative active count"))? >= active_limit {
        return fenced(cx.conn, Fence::Capacity, target);
    }
    if let Some(next_run) = input_due_at {
        cx.write(
            "UPDATE sentinel_runtime SET paused = 0, generation = generation + 1, next_run = ?2 WHERE sentinel_id = ?1",
            params![target, to_sql_u64("input activation due time", next_run)?],
        )?;
    } else {
        cx.write(
            "UPDATE sentinel_runtime SET paused = 0, generation = generation + 1 WHERE sentinel_id = ?1",
            [target],
        )?;
    }
    touch(cx, target)?;
    finished(cx.conn, MutationOutcome::Applied, &[target.to_string()])
}
