use super::super::{
    db_error,
    document_queries::regression::{completed_group_facts, unsigned_timestamp},
};
use super::{column, decode, record_query};
use crate::service_plan::{
    NoticeCartographyRead, NoticeClosedGroup, NoticeDigestInput, NoticeFinishedBinding,
    NoticeInventoryTargets, NoticeQueueRead, PlanError,
};
use rusqlite::{params, Connection, OptionalExtension};

fn timestamp(token: Option<String>) -> Result<Option<u64>, PlanError> {
    token
        .map(|token| {
            token
                .trim()
                .parse::<u64>()
                .map_err(|error| PlanError::Io(error.to_string()))
        })
        .transpose()
}

pub(in crate::service_plan::store_db) fn notice_digest_input(
    connection: &Connection,
    id: &str,
    now: u64,
    targets: &NoticeInventoryTargets,
) -> Result<NoticeDigestInput, PlanError> {
    record_query();
    let (team_id,owner_tab_id,project_root)=connection.query_row(
        "SELECT team_id,owner_tab_id,CASE WHEN coalesce(json_extract(header_json,'$.canonical_project_path'),'')='' THEN project_path ELSE json_extract(header_json,'$.canonical_project_path') END FROM plans WHERE id=?1",
        [id],|row|Ok((column::<Option<String>>(row,0)?,column::<Option<String>>(row,1)?,column::<String>(row,2)?))).optional().map_err(db_error)?.ok_or(PlanError::NotFound)?;
    let changed = unsigned_timestamp("t.row_json", "status_changed_at", true);
    record_query();
    let sql=format!("SELECT count(CASE WHEN status='in_review' THEN 1 END),count(CASE WHEN status='in_progress' THEN 1 END),
        min(CASE WHEN status='in_review' THEN printf('%20s',{changed}) END),count(*),count(CASE WHEN status NOT IN ('done','cancelled') THEN 1 END),max(printf('%20s',{changed}))
        FROM plan_tasks t WHERE plan_id=?1");
    let (in_review, in_progress, oldest, total, open, latest) = connection
        .query_row(&sql, [id], |row| {
            Ok((
                row.get::<_, usize>(0)?,
                row.get::<_, usize>(1)?,
                column::<Option<String>>(row, 2)?,
                row.get::<_, usize>(3)?,
                row.get::<_, usize>(4)?,
                column::<Option<String>>(row, 5)?,
            ))
        })
        .map_err(db_error)?;
    let oldest_in_review_age_ms = timestamp(oldest)?
        .map(|at| now.saturating_sub(at))
        .unwrap_or(0);
    let release_at = if total > 0 && open == 0 {
        timestamp(latest)?
    } else {
        None
    };
    record_query();
    let facts = super::super::curator_notice_queries::reminder_task_facts();
    let sql=format!("{facts} SELECT count(*) FROM plan_tasks t JOIN facts f ON f.task_id=t.task_id WHERE t.plan_id=?1 AND t.status='in_progress' AND domios_notice_stalled({changed},?3,json_extract(f.decision,'$.aged_eligible'))");
    let stalled_lanes = connection
        .query_row(
            &sql,
            params![
                id,
                super::super::status_queries::WHITESPACE,
                now.to_string()
            ],
            |row| row.get::<_, usize>(0),
        )
        .map_err(db_error)?;
    let (finished_lanes, lane_candidates) =
        super::super::notice_queries::recorded_finished_lanes(connection, id)?;
    let cartography = cartography(connection, id, release_at)?;
    record_query();
    let task_ids = serde_json::to_string(&targets.task_ids)
        .map_err(|error| PlanError::Io(error.to_string()))?;
    let tab_ids = serde_json::to_string(&targets.tab_ids)
        .map_err(|error| PlanError::Io(error.to_string()))?;
    let mut statement=connection.prepare_cached("SELECT task_id,assigned_tab_id FROM plan_tasks WHERE plan_id=?1 AND status='done' AND (task_id IN(SELECT value FROM json_each(?2)) OR assigned_tab_id IN(SELECT value FROM json_each(?3))) ORDER BY position").map_err(db_error)?;
    let finished_bindings = statement
        .query_map(params![id, task_ids, tab_ids], |row| {
            Ok(NoticeFinishedBinding {
                task_id: column(row, 0)?,
                assigned_tab_id: column(row, 1)?,
            })
        })
        .map_err(db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)?;
    Ok(NoticeDigestInput {
        plan_id: id.into(),
        team_id,
        owner_tab_id,
        project_root,
        queue: NoticeQueueRead {
            in_review,
            in_progress,
            oldest_in_review_age_ms,
            stalled_lanes,
            finished_lanes,
            lane_candidates,
        },
        cartography,
        finished_bindings,
    })
}

fn cartography(
    connection: &Connection,
    id: &str,
    release_at: Option<u64>,
) -> Result<NoticeCartographyRead, PlanError> {
    record_query();
    let stride_at = unsigned_timestamp("s.row_json", "ts", false);
    let sql=format!("SELECT max(at) FROM (
        SELECT printf('%20s',{stride_at}) AS at FROM plan_strides s WHERE plan_id=?1 AND json_extract(row_json,'$.kind') IN ('map_requested','map_generated')
        UNION ALL SELECT printf('%20s',{stride_at}) AS at FROM plan_task_milestones s WHERE plan_id=?1 AND json_extract(row_json,'$.kind') IN ('map_requested','map_generated'))");
    let latest_map_at = timestamp(
        connection
            .query_row(&sql, [id], |row| column(row, 0))
            .map_err(db_error)?,
    )?
    .unwrap_or(0);
    record_query();
    let completed = completed_group_facts();
    let changed = unsigned_timestamp("t.row_json", "status_changed_at", true);
    let sql=format!("{completed}, ranked AS MATERIALIZED (
        SELECT g.group_key,(SELECT max(printf('%20s',{changed})) FROM members t WHERE t.group_key=g.group_key) AS closed_at
        FROM completed g ORDER BY closed_at DESC,g.group_key DESC LIMIT 1)
        SELECT json_object('key',g.group_key,'closed_at',json(trim(g.closed_at)),
            'label',CASE WHEN EXISTS(SELECT 1 FROM plans p,json_each(p.header_json,'$.task_groups') d WHERE p.id=?1 AND json_extract(d.value,'$.key')=g.group_key)
                THEN (SELECT json_extract(d.value,'$.label') FROM plans p,json_each(p.header_json,'$.task_groups') d WHERE p.id=?1 AND json_extract(d.value,'$.key')=g.group_key ORDER BY d.key LIMIT 1)
                ELSE g.group_key END) FROM ranked g");
    let raw: Option<String> = connection
        .query_row(&sql, [id], |row| column(row, 0))
        .optional()
        .map_err(db_error)?;
    let closed_group = raw
        .map(|raw| decode::<NoticeClosedGroup>(id, &raw))
        .transpose()?;
    Ok(NoticeCartographyRead {
        latest_map_at,
        release_at,
        closed_group,
    })
}
