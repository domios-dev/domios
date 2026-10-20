use super::super::records::{
    Fence, MutationOutcome, SentinelCommit, SentinelCreate, SentinelEviction,
};
use super::{create_inner, delete_scenario};
use crate::error::{sqlite_error, StorageError};
use crate::owner::WriteCx;
use crate::repositories::to_sql_u64;
use rusqlite::{params, OptionalExtension};

pub(crate) fn create_evicting(
    cx: &mut WriteCx<'_>,
    create: SentinelCreate,
    eviction: SentinelEviction,
) -> Result<SentinelCommit, StorageError> {
    let expected_revision = to_sql_u64("revision", eviction.expected_revision)?;
    let candidate = cx
        .conn
        .query_row(
            "SELECT b.agent_authored, b.scenario_id, b.revision, r.stopped, d.source_kind, \
             d.until_bucket, EXISTS (SELECT 1 FROM blueprint_handlers h WHERE \
             h.blueprint_id = b.id AND h.version = d.version AND h.kind = 'stop'), \
             EXISTS (SELECT 1 FROM blueprint_buckets k WHERE k.blueprint_id = b.id \
             AND k.version = d.version AND k.rearm_on_change = 1) \
             FROM blueprints b JOIN blueprint_runtime r ON r.blueprint_id = b.id \
             JOIN blueprint_heads head ON head.blueprint_id = b.id \
             JOIN blueprint_definitions d ON d.blueprint_id = b.id AND d.version = head.version \
             WHERE b.id = ?1 AND b.workspace_id = ?2 AND b.team_id IS ?3",
            params![
                eviction.id.as_str(),
                eviction.workspace_id,
                eviction.team_id
            ],
            |row| {
                Ok((
                    row.get::<_, bool>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, bool>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, bool>(6)?,
                    row.get::<_, bool>(7)?,
                ))
            },
        )
        .optional()
        .map_err(|error| sqlite_error("read Blueprint eviction candidate", error))?;
    let Some((
        agent_authored,
        scenario_id,
        revision,
        stopped,
        source_kind,
        until_bucket,
        has_stop_handler,
        has_rearm,
    )) = candidate
    else {
        return Ok(SentinelCommit::fenced(Fence::Generation, None));
    };
    if !agent_authored
        || scenario_id.as_deref() != Some(eviction.id.as_str())
        || revision != expected_revision as i64
        || !stopped
        || source_kind == "pipeline"
        || (until_bucket.is_none() && !has_stop_handler)
        || has_rearm
    {
        return Ok(SentinelCommit::fenced(Fence::Generation, None));
    }
    let active_descendants: i64 = cx
        .conn
        .query_row(
            "SELECT COUNT(*) FROM blueprints b JOIN blueprint_runtime r \
             ON r.blueprint_id = b.id WHERE b.workspace_id = ?1 AND b.team_id IS ?2 \
             AND (b.id = ?3 OR b.scenario_id = ?3) AND r.stopped = 0",
            params![
                eviction.workspace_id,
                eviction.team_id,
                eviction.id.as_str()
            ],
            |row| row.get(0),
        )
        .map_err(|error| sqlite_error("check Blueprint eviction descendants", error))?;
    if active_descendants != 0 {
        return Ok(SentinelCommit::fenced(Fence::Stopped, None));
    }
    let deleted = delete_scenario(
        cx,
        eviction.id,
        &eviction.workspace_id,
        eviction.team_id.as_deref(),
    )?;
    if deleted.outcome != MutationOutcome::Applied {
        return Ok(deleted);
    }
    let mut created = create_inner(cx, create)?;
    created.removed = deleted.removed;
    Ok(created)
}
