use super::records::{
    BlueprintJournalEntry, BlueprintJournalWrite, Fence, MutationOutcome, SentinelCommit,
};
use crate::error::{sqlite_error, StorageError};
use crate::owner::{ReadCx, WriteCx};
use crate::repositories::{from_sql_u64, to_sql_u64};
use codeterm_types::blueprint_journal::{
    JournalMarker, JournalOutcome, JournalReason, JournalState,
};
use rusqlite::{params, OptionalExtension};

const RETAIN_RUNS: i64 = 32;

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
    body_ref: Option<&str>,
) -> Result<(), StorageError> {
    cx.write(
        "INSERT INTO blueprint_step_journal \
         (blueprint_id, run_id, step_index, step_name, state, outcome, started_at_ms, ended_at_ms, result_summary, reason, body_ref) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
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
            reason,
            body_ref
        ],
    )?;
    cx.write(
        "DELETE FROM blueprint_step_journal WHERE blueprint_id = ?1 AND run_id NOT IN \
         (SELECT run_id FROM blueprint_step_journal WHERE blueprint_id = ?1 \
          GROUP BY run_id ORDER BY MAX(entry_id) DESC LIMIT ?2)",
        params![id, RETAIN_RUNS],
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
        None,
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
            marker,
            step_name,
            generation,
            started_at_ms,
        } => {
            let step_index = marker.index();
            let step_name = marker.name(&step_name).to_string();
            let eligible: bool = cx
                .conn
                .query_row(
                    "SELECT EXISTS (SELECT 1 FROM blueprint_runtime WHERE blueprint_id = ?1 \
                     AND generation = ?2 AND paused = 0 AND stopped = 0 AND held IS NULL)",
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
                    "SELECT claim.run_id FROM blueprint_step_journal AS claim \
                     WHERE claim.blueprint_id = ?1 AND claim.state = 'running' \
                     AND NOT EXISTS (SELECT 1 FROM blueprint_step_journal AS resolved \
                       WHERE resolved.blueprint_id = claim.blueprint_id \
                       AND resolved.run_id = claim.run_id \
                       AND (resolved.step_index = claim.step_index OR (resolved.step_index = -1 AND resolved.step_name = '$run')) \
                       AND resolved.entry_id > claim.entry_id \
                       AND resolved.state IN ('done', 'failed', 'skipped', 'held')) \
                     ORDER BY claim.entry_id DESC LIMIT 1",
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
                    "SELECT run_id FROM blueprint_step_journal WHERE blueprint_id = ?1 \
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
                     WHERE blueprint_id = ?1 AND run_id = ?2 AND step_index = -1 AND step_name = '$run' \
                     AND state IN ('done', 'failed', 'skipped', 'held'))",
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
                None,
            )?;
            Ok(commit(MutationOutcome::Applied))
        }
        BlueprintJournalWrite::Finish {
            run_id,
            marker,
            state,
            outcome,
            ended_at_ms,
            result_summary,
            body_ref,
            reason,
        } => {
            let step_index = marker.index();
            let claim: Option<(String, i64)> = cx
                .conn
                .query_row(
                    "SELECT claim.step_name, claim.started_at_ms \
                     FROM blueprint_step_journal AS claim \
                     WHERE claim.blueprint_id = ?1 AND claim.run_id = ?2 \
                     AND claim.step_index = ?3 AND claim.state = 'running' \
                     AND NOT EXISTS (SELECT 1 FROM blueprint_step_journal AS resolved \
                       WHERE resolved.blueprint_id = claim.blueprint_id \
                       AND resolved.run_id = claim.run_id \
                       AND (resolved.step_index = claim.step_index OR (resolved.step_index = -1 AND resolved.step_name = '$run')) \
                       AND resolved.entry_id > claim.entry_id \
                       AND resolved.state IN ('done', 'failed', 'skipped', 'held')) \
                     ORDER BY claim.entry_id DESC LIMIT 1",
                    params![id, run_id, step_index],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(|error| sqlite_error("inspect blueprint step finish", error))?;
            let Some((step_name, started_at_ms)) = claim else {
                return Ok(SentinelCommit::fenced(Fence::NotClaimed, None));
            };
            append(
                cx,
                id,
                &run_id,
                step_index,
                &step_name,
                state.as_str(),
                Some(outcome.as_str()),
                from_sql_u64("journal start", started_at_ms)?,
                Some(ended_at_ms),
                Some(&result_summary),
                reason.as_deref(),
                body_ref.as_deref(),
            )?;
            Ok(commit(MutationOutcome::Applied))
        }
        BlueprintJournalWrite::Decision {
            run_id,
            marker,
            step_name,
            outcome,
            at_ms,
            result_summary,
            reason,
        } => {
            let step_index = marker.index();
            let step_name = marker.name(&step_name).to_string();
            let state = if outcome.is_skip() { "skipped" } else { "done" };
            append(
                cx,
                id,
                &run_id,
                step_index,
                &step_name,
                state,
                Some(outcome.as_str()),
                at_ms,
                Some(at_ms),
                result_summary.as_deref(),
                reason.as_deref(),
                None,
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
                state.as_str(),
                Some(outcome.as_str()),
                at_ms,
                Some(at_ms),
                None,
                reason.as_deref(),
                None,
            )?;
            if state == JournalState::Held || reason == Some(JournalReason::JournalWriteFailed) {
                let reason = reason.as_deref().unwrap_or("journal_write_failed");
                cx.write("UPDATE blueprint_runtime SET held = ?2, last_reason = ?2 WHERE blueprint_id = ?1", params![id, reason])?;
            }
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
             ended_at_ms, result_summary, reason, body_ref FROM blueprint_step_journal \
             WHERE blueprint_id = ?1 AND run_id IN \
             (SELECT run_id FROM blueprint_step_journal WHERE blueprint_id = ?1 \
              GROUP BY run_id ORDER BY MAX(entry_id) DESC LIMIT ?2) ORDER BY entry_id DESC",
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
                row.get::<_, Option<String>>(10)?,
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
            body_ref,
        ) = row.map_err(|error| sqlite_error("read blueprint journal row", error))?;
        JournalMarker::from_columns(step_index, &step_name)
            .map_err(|error| StorageError::invalid("journal marker", error))?;
        entries.push(BlueprintJournalEntry {
            entry_id,
            run_id,
            step_index,
            step_name,
            state: JournalState::parse(&state)
                .map_err(|error| StorageError::invalid("journal state", error))?,
            outcome: outcome.map(JournalOutcome::from),
            started_at_ms: from_sql_u64("journal start", started_at_ms)?,
            ended_at_ms: ended_at_ms
                .map(|value| from_sql_u64("journal end", value))
                .transpose()?,
            result_summary,
            reason: reason.map(JournalReason::from),
            body_ref,
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
