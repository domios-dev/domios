use super::records::{
    BlueprintJournalEntry, BlueprintJournalWrite, Fence, MutationOutcome, SentinelCommit,
};
use crate::error::{sqlite_error, StorageError};
use crate::owner::{ReadCx, WriteCx};
use crate::repositories::{from_sql_u64, to_sql_u64};
use rusqlite::{params, OptionalExtension};

const RETAIN: i64 = 200;

fn commit(outcome: MutationOutcome) -> SentinelCommit {
    SentinelCommit {
        outcome,
        sentinels: Vec::new(),
        removed: Vec::new(),
    }
}

fn append(
    cx: &mut WriteCx<'_>,
    id: &str,
    run_id: &str,
    step_index: i64,
    step_name: &str,
    state: &str,
    outcome: Option<&str>,
    started_at_ms: u64,
    ended_at_ms: Option<u64>,
    result_summary: Option<&str>,
    reason: Option<&str>,
) -> Result<(), StorageError> {
    cx.write(
        "INSERT INTO blueprint_step_journal \
         (sentinel_id, run_id, step_index, step_name, state, outcome, started_at_ms, ended_at_ms, result_summary, reason) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            id,
            run_id,
            step_index,
            step_name,
            state,
            outcome,
            to_sql_u64("journal start", started_at_ms)?,
            ended_at_ms
                .map(|value| to_sql_u64("journal end", value))
                .transpose()?,
            result_summary,
            reason
        ],
    )?;
    cx.write(
        "DELETE FROM blueprint_step_journal WHERE sentinel_id = ?1 AND entry_id NOT IN \
         (SELECT entry_id FROM blueprint_step_journal WHERE sentinel_id = ?1 \
          ORDER BY entry_id DESC LIMIT ?2)",
        params![id, RETAIN],
    )?;
    Ok(())
}

fn refused_claim(
    cx: &mut WriteCx<'_>,
    id: &str,
    run_id: &str,
    step_index: i64,
    step_name: &str,
    reason: &str,
    now: u64,
) -> Result<SentinelCommit, StorageError> {
    let outcome = if reason == "claim_fenced" {
        "skipped"
    } else {
        "skip:uncertain_claim"
    };
    append(
        cx,
        id,
        run_id,
        -1,
        "$run",
        "skipped",
        Some(outcome),
        now,
        Some(now),
        Some(&format!("step {step_index} was not replayed")),
        Some(&format!("skip:{reason} at {step_name}")),
    )?;
    Ok(commit(MutationOutcome::Fenced(Fence::Claimed)))
}

pub(super) fn write(
    cx: &mut WriteCx<'_>,
    id: &str,
    write: BlueprintJournalWrite,
) -> Result<SentinelCommit, StorageError> {
    write.validate()?;
    match write {
        BlueprintJournalWrite::Claim {
            run_id,
            step_index,
            step_name,
            generation,
            started_at_ms,
        } => {
            let eligible: bool = cx
                .conn
                .query_row(
                    "SELECT EXISTS (SELECT 1 FROM sentinel_runtime WHERE sentinel_id = ?1 \
                     AND generation = ?2 AND paused = 0 AND stopped = 0)",
                    params![id, to_sql_u64("journal generation", generation)?],
                    |row| row.get(0),
                )
                .map_err(|error| sqlite_error("fence blueprint step claim", error))?;
            if !eligible {
                return refused_claim(
                    cx,
                    id,
                    &run_id,
                    step_index,
                    &step_name,
                    "claim_fenced",
                    started_at_ms,
                );
            }
            let running: Option<String> = cx
                .conn
                .query_row(
                    "SELECT run_id FROM blueprint_step_journal \
                     WHERE sentinel_id = ?1 AND state = 'running' ORDER BY entry_id DESC LIMIT 1",
                    [id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| sqlite_error("inspect blueprint step claim", error))?;
            if running.is_some() {
                return refused_claim(
                    cx,
                    id,
                    &run_id,
                    step_index,
                    &step_name,
                    "uncertain_claim",
                    started_at_ms,
                );
            }
            let latest: Option<String> = cx
                .conn
                .query_row(
                    "SELECT run_id FROM blueprint_step_journal WHERE sentinel_id = ?1 \
                     ORDER BY entry_id DESC LIMIT 1",
                    [id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| sqlite_error("inspect blueprint run journal", error))?;
            if let Some(latest) = latest.filter(|latest| latest != &run_id) {
                let closed: bool = cx
                    .conn
                    .query_row(
                        "SELECT EXISTS (SELECT 1 FROM blueprint_step_journal \
                     WHERE sentinel_id = ?1 AND run_id = ?2 AND step_name = '$run' \
                     AND outcome IN ('done', 'failed', 'skipped'))",
                        params![id, latest],
                        |row| row.get(0),
                    )
                    .map_err(|error| sqlite_error("inspect blueprint run completion", error))?;
                if !closed {
                    return refused_claim(
                        cx,
                        id,
                        &run_id,
                        step_index,
                        &step_name,
                        "uncertain_claim",
                        started_at_ms,
                    );
                }
            }
            append(
                cx,
                id,
                &run_id,
                step_index,
                &step_name,
                "running",
                None,
                started_at_ms,
                None,
                None,
                None,
            )?;
            Ok(commit(MutationOutcome::Applied))
        }
        BlueprintJournalWrite::Finish {
            run_id,
            step_index,
            state,
            outcome,
            ended_at_ms,
            result_summary,
            reason,
        } => {
            let changed = cx.write(
                "UPDATE blueprint_step_journal SET state = ?1, outcome = ?2, ended_at_ms = ?3, \
                     result_summary = ?4, reason = ?5 WHERE entry_id = \
                     (SELECT entry_id FROM blueprint_step_journal WHERE sentinel_id = ?6 \
                      AND run_id = ?7 AND step_index = ?8 AND state = 'running' \
                      ORDER BY entry_id DESC LIMIT 1)",
                params![
                    state,
                    outcome,
                    to_sql_u64("journal end", ended_at_ms)?,
                    result_summary,
                    reason,
                    id,
                    run_id,
                    step_index
                ],
            )?;
            if changed == 0 {
                Ok(SentinelCommit::fenced(Fence::NotClaimed, None))
            } else {
                Ok(commit(MutationOutcome::Applied))
            }
        }
        BlueprintJournalWrite::Decision {
            run_id,
            step_index,
            step_name,
            outcome,
            at_ms,
            result_summary,
            reason,
        } => {
            let state = if outcome.starts_with("skip:") {
                "skipped"
            } else {
                "done"
            };
            append(
                cx,
                id,
                &run_id,
                step_index,
                &step_name,
                state,
                Some(&outcome),
                at_ms,
                Some(at_ms),
                result_summary.as_deref(),
                reason.as_deref(),
            )?;
            Ok(commit(MutationOutcome::Applied))
        }
        BlueprintJournalWrite::CloseRun {
            run_id,
            state,
            outcome,
            at_ms,
            reason,
        } => {
            append(
                cx,
                id,
                &run_id,
                -1,
                "$run",
                &state,
                Some(&outcome),
                at_ms,
                Some(at_ms),
                None,
                reason.as_deref(),
            )?;
            Ok(commit(MutationOutcome::Applied))
        }
    }
}

pub(super) fn log(
    cx: &ReadCx<'_>,
    id: &str,
    limit: u16,
) -> Result<Vec<BlueprintJournalEntry>, StorageError> {
    let mut statement = cx
        .conn
        .prepare_cached(
            "SELECT entry_id, run_id, step_index, step_name, state, outcome, started_at_ms, \
             ended_at_ms, result_summary, reason FROM blueprint_step_journal \
             WHERE sentinel_id = ?1 ORDER BY entry_id DESC LIMIT ?2",
        )
        .map_err(|error| sqlite_error("read blueprint journal", error))?;
    let rows = statement
        .query_map(params![id, i64::from(limit)], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, Option<i64>>(7)?,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, Option<String>>(9)?,
            ))
        })
        .map_err(|error| sqlite_error("query blueprint journal", error))?;
    let mut entries = Vec::new();
    for row in rows {
        let (
            entry_id,
            run_id,
            step_index,
            step_name,
            state,
            outcome,
            started_at_ms,
            ended_at_ms,
            result_summary,
            reason,
        ) = row.map_err(|error| sqlite_error("read blueprint journal row", error))?;
        entries.push(BlueprintJournalEntry {
            entry_id,
            run_id,
            step_index,
            step_name,
            state,
            outcome,
            started_at_ms: from_sql_u64("journal start", started_at_ms)?,
            ended_at_ms: ended_at_ms
                .map(|value| from_sql_u64("journal end", value))
                .transpose()?,
            result_summary,
            reason,
        });
    }
    Ok(entries)
}

pub(super) fn logs(
    cx: &ReadCx<'_>,
    ids: &[super::records::SentinelId],
    limit: u16,
) -> Result<std::collections::BTreeMap<String, Vec<BlueprintJournalEntry>>, StorageError> {
    let mut result = std::collections::BTreeMap::new();
    for id in ids {
        result.insert(id.as_str().to_string(), log(cx, id.as_str(), limit)?);
    }
    Ok(result)
}
