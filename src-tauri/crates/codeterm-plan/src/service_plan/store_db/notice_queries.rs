use super::read_projections::{column, decode, record_query};
use super::{db_error, PlanDatabase};
use crate::service_plan::status_projection::PlanNoticeRead;
use crate::service_plan::PlanError;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};

const LANES: &str = "WITH eligible AS MATERIALIZED (
    SELECT task_id,position,json_extract(row_json,'$.peer') AS peer,
        json_extract(row_json,'$.lane.worktree_path') AS path,
        coalesce(json_extract(row_json,'$.lane.cleanup_status'),'pending') AS safety,
        EXISTS(SELECT 1 FROM json_each(t.row_json,'$.lane.cleanup_hints')
            WHERE instr(lower(value),'dirty')>0 OR instr(lower(value),'unmerged')>0) AS unsafe_lane
    FROM plan_tasks t WHERE plan_id=?1 AND status IN ('done','cancelled')
        AND coalesce(json_extract(row_json,'$.lane.cleanup_status'),'pending') COLLATE NOCASE
            NOT IN ('done','cleaned','released','complete')
    ), paths AS (
    SELECT position AS task_position,0 AS phase,0 AS seq,safety,unsafe_lane,
        peer,path FROM eligible
    UNION ALL
    SELECT t.position,1,a.position,t.safety,t.unsafe_lane,
        json_extract(a.row_json,'$.mesh_peer'),coalesce(json_extract(a.row_json,'$.remote_worktree_path'),json_extract(a.row_json,'$.worktree_path'))
    FROM eligible t JOIN plan_task_attempts a ON a.plan_id=?1 AND a.task_id=t.task_id
    ), ranked AS (
    SELECT peer,trim(path,?2) AS path,safety,unsafe_lane,
        row_number() OVER(PARTITION BY peer,trim(path,?2) ORDER BY task_position,phase,seq) AS rank
    FROM paths WHERE path IS NOT NULL AND trim(path,?2)<>''
    ) SELECT peer,path,safety,unsafe_lane,count(*) OVER() FROM ranked WHERE rank=1
      ORDER BY unsafe_lane DESC,peer,path LIMIT 5";

#[derive(serde::Deserialize)]
struct ScopeRead {
    mode: crate::plan::PlanMode,
    owner_tab_id: Option<String>,
    team_id: Option<String>,
    complete: bool,
}

impl PlanDatabase {
    pub(in crate::service_plan) fn notice_projection(
        &self,
        id: &str,
    ) -> Result<PlanNoticeRead, PlanError> {
        let connection = Connection::open_with_flags(
            self.path(),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(db_error)?;
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(db_error)?;
        super::status_queries::register_functions(&connection)?;
        super::curator_notice_queries::register_functions(&connection)?;
        connection
            .execute_batch("BEGIN DEFERRED TRANSACTION")
            .map_err(db_error)?;
        record_query();
        let raw:String=connection.query_row("SELECT json_object('owner_tab_id',CASE WHEN json_type(header_json,'$.owner')='object' THEN json_extract(header_json,'$.owner.tab_id') ELSE json('{}') END,
            'team_id',json_extract(header_json,'$.team_id'),'mode',CASE WHEN json_type(header_json,'$.mode') IS NULL THEN 'planning' ELSE json_extract(header_json,'$.mode') END,
            'complete',json(CASE WHEN EXISTS(SELECT 1 FROM plan_tasks WHERE plan_id=?1) AND NOT EXISTS(SELECT 1 FROM plan_tasks WHERE plan_id=?1 AND status NOT IN ('done','cancelled')) THEN 'true' ELSE 'false' END))
            FROM plans WHERE id=?1",[id],|row|column(row,0)).optional().map_err(db_error)?.ok_or(PlanError::NotFound)?;
        let scope: ScopeRead = decode(id, &raw)?;
        Ok(PlanNoticeRead {
            connection,
            plan_id: id.into(),
            mode: scope.mode,
            owner_tab_id: scope.owner_tab_id,
            team_id: scope.team_id,
            complete: scope.complete,
        })
    }
}

impl PlanNoticeRead {
    pub fn digest_input(
        &self,
        now: u64,
        targets: &crate::service_plan::NoticeInventoryTargets,
    ) -> Result<crate::service_plan::NoticeDigestInput, PlanError> {
        super::read_projections::reminder_digests::notice_digest_input(
            &self.connection,
            &self.plan_id,
            now,
            targets,
        )
    }

    pub fn prepend_heartbeat(
        &self,
        now: u64,
        text: &str,
        digests: &[String],
        selection: Option<&[String]>,
    ) -> Result<String, PlanError> {
        super::curator_notice_queries::heartbeat_snapshot(
            &self.connection,
            &self.plan_id,
            now,
            selection,
        )
        .map(|read| read.prepend(text, digests))
    }
}

pub(super) fn recorded_finished_lanes(
    connection: &rusqlite::Connection,
    plan_id: &str,
) -> Result<(usize, Vec<String>), PlanError> {
    let mut finished_lanes = 0;
    let mut lane_candidates = Vec::new();
    {
        record_query();
        let mut statement = connection.prepare_cached(LANES).map_err(db_error)?;
        for row in statement
            .query_map(params![plan_id, super::status_queries::WHITESPACE], |row| {
                Ok((
                    column::<Option<String>>(row, 0)?,
                    column::<String>(row, 1)?,
                    column::<String>(row, 2)?,
                    row.get::<_, bool>(3)?,
                    row.get::<_, usize>(4)?,
                ))
            })
            .map_err(db_error)?
        {
            let (peer, path, safety, unsafe_lane, count) = row.map_err(db_error)?;
            finished_lanes = count;
            let label = match peer {
                Some(peer) => format!("{peer}:{path}"),
                None => path,
            };
            let safety = if unsafe_lane {
                "retain dirty/unmerged"
            } else {
                &safety
            };
            lane_candidates.push(format!("{label} ({safety})"));
        }
    }
    Ok((finished_lanes, lane_candidates))
}
