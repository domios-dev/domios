use super::regression::unsigned_timestamp;
use super::{column, db_error, decode, record_query, PlanDatabase};
use crate::plan::{PlanLifecycleEvent, TaskStatus};
use crate::service_plan::state::TaskCounts;
use crate::service_plan::{PlanError, TeamEventCursor, TeamPlanMetadataRead, TeamPlanRead};
use rusqlite::{params, Connection};

impl PlanDatabase {
    pub(in crate::service_plan) fn team_mission_control_input(
        &self,
        team_id: &str,
        limit: usize,
        cursor: Option<&TeamEventCursor>,
    ) -> Result<Vec<TeamPlanRead>, PlanError> {
        let mut guard = self.connection.lock();
        let transaction = guard.transaction().map_err(db_error)?;
        record_query();
        let headers = {
            let mut statement=transaction.prepare_cached(
                "SELECT json_object('id',id,'title',title,'team_id',json_extract(header_json,'$.team_id'),
                    'status',status,'mode',CASE WHEN json_type(header_json,'$.mode') IS NULL THEN 'planning' ELSE json_extract(header_json,'$.mode') END,
                    'planning_state',CASE WHEN json_type(header_json,'$.planning_state') IS NULL THEN 'drafting' ELSE json_extract(header_json,'$.planning_state') END,
                    'goal',CASE WHEN json_type(header_json,'$.goal') IS NULL OR json_type(header_json,'$.goal')='null' THEN NULL
                        ELSE json_object('vector',json_extract(header_json,'$.goal.vector')) END,
                    'created_at',json(header_json->'$.created_at'),'updated_at',json(header_json->'$.updated_at'))
                 FROM plans WHERE json_extract(header_json,'$.team_id')=?1 ORDER BY id"
            ).map_err(db_error)?;
            let rows = statement
                .query_map([team_id], |row| column::<String>(row, 0))
                .map_err(db_error)?;
            rows.map(|row| decode::<TeamPlanMetadataRead>(team_id, &row.map_err(db_error)?))
                .collect::<Result<Vec<_>, PlanError>>()?
        };
        let mut plans = Vec::with_capacity(headers.len());
        for metadata in headers {
            let id = &metadata.id;
            let task_counts = task_counts(&transaction, id)?;
            let lifecycle_events = lifecycle_page(&transaction, id, limit.min(100) + 1, cursor)?;
            let stored_events = super::super::state_event_queries::event_page(
                &transaction,
                id,
                limit.min(100) + 1,
                cursor.map(|cursor| (cursor.ts, cursor.plan_id.as_str(), cursor.event_id.as_str())),
            )?;
            plans.push(TeamPlanRead {
                metadata,
                task_counts,
                lifecycle_events,
                stored_events,
            });
        }
        transaction.commit().map_err(db_error)?;
        plans.sort_by(|a, b| {
            b.metadata
                .updated_at
                .cmp(&a.metadata.updated_at)
                .then_with(|| b.metadata.created_at.cmp(&a.metadata.created_at))
                .then_with(|| a.metadata.id.cmp(&b.metadata.id))
        });
        Ok(plans)
    }
}

fn task_counts(connection: &Connection, id: &str) -> Result<TaskCounts, PlanError> {
    record_query();
    let mut statement = connection
        .prepare_cached(
            "SELECT json_quote(status),count(*) FROM plan_tasks WHERE plan_id=?1 GROUP BY status",
        )
        .map_err(db_error)?;
    let rows = statement
        .query_map([id], |row| {
            Ok((column::<String>(row, 0)?, row.get::<_, usize>(1)?))
        })
        .map_err(db_error)?;
    let mut counts = TaskCounts::default();
    for row in rows {
        let (status, count) = row.map_err(db_error)?;
        match decode::<TaskStatus>(id, &status)? {
            TaskStatus::Pending => counts.pending = count,
            TaskStatus::InProgress => counts.in_progress = count,
            TaskStatus::InReview => counts.in_review = count,
            TaskStatus::Blocked => counts.blocked = count,
            TaskStatus::Paused => counts.paused = count,
            TaskStatus::Done => counts.done = count,
            TaskStatus::Cancelled => counts.cancelled = count,
        }
        counts.total += count;
    }
    Ok(counts)
}

#[derive(serde::Deserialize)]
struct LifecycleRow {
    ordinal: usize,
    event: PlanLifecycleEvent,
}

fn lifecycle_page(
    connection: &Connection,
    id: &str,
    limit: usize,
    cursor: Option<&TeamEventCursor>,
) -> Result<Vec<(usize, PlanLifecycleEvent)>, PlanError> {
    let at = unsigned_timestamp("e.value", "at", false);
    let sql=format!("WITH events AS (
        SELECT e.key AS ordinal,printf('%20s',{at}) AS at_key,
            CASE WHEN coalesce(json_extract(e.value,'$.id'),'')='' THEN 'lifecycle-'||e.key||'-'||json_extract(e.value,'$.kind')||'-'||({at}) ELSE json_extract(e.value,'$.id') END AS event_id,
            json_object('id',coalesce(json_extract(e.value,'$.id'),''),'kind',json_extract(e.value,'$.kind'),
                'at',json({at}),'author',json_extract(e.value,'$.author')) AS document
        FROM plans p,json_each(p.header_json,'$.lifecycle_events') e WHERE p.id=?1)
        SELECT json_object('ordinal',ordinal,'event',json(document)) FROM events
        WHERE ?2 IS NULL OR at_key<?2 OR (at_key=?2 AND (?1>?3 OR (?1=?3 AND event_id>?4)))
        ORDER BY at_key DESC,event_id LIMIT ?5");
    let (at, plan, event) = match cursor {
        Some(cursor) => (
            Some(format!("{:>20}", cursor.ts)),
            Some(cursor.plan_id.as_str()),
            Some(cursor.event_id.as_str()),
        ),
        None => (None, None, None),
    };
    record_query();
    let mut statement = connection.prepare_cached(&sql).map_err(db_error)?;
    let rows = statement
        .query_map(params![id, at, plan, event, limit], |row| {
            column::<String>(row, 0)
        })
        .map_err(db_error)?;
    rows.map(|row| {
        let row: LifecycleRow = decode(id, &row.map_err(db_error)?)?;
        Ok((row.ordinal, row.event))
    })
    .collect()
}
