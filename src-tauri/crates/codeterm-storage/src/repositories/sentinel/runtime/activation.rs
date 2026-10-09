use super::{check_batch, corrupt, fenced, finished, live, touch};
use crate::error::{sqlite_error, StorageError};
use crate::owner::WriteCx;
use crate::repositories::sentinel::records::{
    BatchFence, Fence, MutationOutcome, SentinelCommit, SentinelId,
};
use crate::repositories::to_sql_u64;
use codeterm_types::blueprint_run::Context;
use rusqlite::{params, Connection, OptionalExtension};

type ScopeRow = (String, String, Option<String>);

fn scope_row(conn: &Connection, id: &str) -> Result<Option<ScopeRow>, StorageError> {
    conn.prepare_cached(
        "SELECT creator_tab_id, workspace_id, team_id FROM blueprints WHERE id = ?1",
    )
    .and_then(|mut statement| {
        statement
            .query_row([id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .optional()
    })
    .map_err(|e| sqlite_error("read blueprint scope", e))
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
            "SELECT COUNT(*) FROM blueprint_handler_results r \
             JOIN blueprint_transitions t \
               ON t.blueprint_id = r.blueprint_id AND t.transition_id = r.transition_id \
             JOIN blueprint_heads h ON h.blueprint_id = r.blueprint_id \
             JOIN blueprint_handlers hd ON hd.blueprint_id = h.blueprint_id \
               AND hd.version = h.version AND hd.handler_id = r.handler_id \
               AND hd.bucket = t.bucket \
             WHERE r.blueprint_id = ?1 AND r.transition_id = ?2 AND hd.kind = 'blueprint' \
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
    let (context, handler): (Option<String>, String) = cx.conn.query_row(
        "SELECT t.causal_run_json, r.handler_id FROM blueprint_transitions t \
         JOIN blueprint_handler_results r ON r.blueprint_id = t.blueprint_id AND r.transition_id = t.transition_id \
         JOIN blueprint_heads h ON h.blueprint_id = t.blueprint_id \
         JOIN blueprint_handlers hd ON hd.blueprint_id = h.blueprint_id AND hd.version = h.version AND hd.handler_id = r.handler_id AND hd.bucket = t.bucket \
         WHERE t.blueprint_id = ?1 AND t.transition_id = ?2 AND hd.kind = 'blueprint' AND hd.target_ref = ?3 \
         AND r.claim_id IS NOT NULL AND r.status = 'unconfirmed' ORDER BY r.ordinal LIMIT 1",
        params![source, fence.transition_id, target],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ).map_err(|e| sqlite_error("read activation run context", e))?;
    let context = context
        .map(|json| {
            let run: Context =
                serde_json::from_str(&json).map_err(|_| corrupt("invalid causal run context"))?;
            serde_json::to_string(&run.child(source, &fence.transition_id, &handler))
                .map_err(|_| corrupt("invalid causal run context"))
        })
        .transpose()?;
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
            "SELECT COUNT(*) FROM blueprint_runtime WHERE stopped = 0 AND paused = 0",
            [],
            |row| row.get(0),
        )
        .map_err(|e| sqlite_error("count active blueprints", e))?;
    if usize::try_from(active).map_err(|_| corrupt("negative active count"))? >= active_limit {
        return fenced(cx.conn, Fence::Capacity, target);
    }
    if let Some(next_run) = input_due_at {
        cx.write(
            "UPDATE blueprint_runtime SET paused = 0, generation = generation + 1, next_run = ?2, causal_run_json = ?3 WHERE blueprint_id = ?1",
            params![target, to_sql_u64("input activation due time", next_run)?, context],
        )?;
    } else {
        cx.write(
            "UPDATE blueprint_runtime SET paused = 0, generation = generation + 1, causal_run_json = ?2 WHERE blueprint_id = ?1",
            params![target, context],
        )?;
    }
    touch(cx, target)?;
    finished(cx.conn, MutationOutcome::Applied, &[target.to_string()])
}
