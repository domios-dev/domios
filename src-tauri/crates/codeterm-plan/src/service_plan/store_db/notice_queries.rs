use super::read_projections::{column, record_query};
use super::{db_error, PlanDatabase};
use crate::plan::Milestone;
use crate::service_plan::status_projection::PlanNoticeRead;
use crate::service_plan::PlanError;
use rusqlite::params;

const LANES: &str = "WITH eligible AS (
    SELECT task_id,position,row_json,
        coalesce(json_extract(row_json,'$.lane.cleanup_status'),'pending') AS safety,
        EXISTS(SELECT 1 FROM json_each(t.row_json,'$.lane.cleanup_hints')
            WHERE instr(lower(value),'dirty')>0 OR instr(lower(value),'unmerged')>0) AS unsafe_lane
    FROM plan_tasks t WHERE plan_id=?1 AND status IN ('done','cancelled')
        AND coalesce(json_extract(row_json,'$.lane.cleanup_status'),'pending') COLLATE NOCASE
            NOT IN ('done','cleaned','released','complete')
    ), paths AS (
    SELECT position AS task_position,0 AS phase,0 AS seq,safety,unsafe_lane,
        json_extract(row_json,'$.peer') AS peer,json_extract(row_json,'$.lane.worktree_path') AS path FROM eligible
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

impl PlanDatabase {
    pub(in crate::service_plan) fn notice_projection(
        &self,
        plan_id: &str,
        now_ms: u64,
        selected_learnings: &[String],
    ) -> Result<PlanNoticeRead, PlanError> {
        let mut connection = self.connection.lock();
        let transaction = connection.transaction().map_err(db_error)?;
        let status = super::status_queries::load_status_projection(
            &transaction,
            plan_id,
            now_ms,
            selected_learnings,
        )?;
        let mut plan = status.plan;
        record_query();
        let latest_map: u64 = transaction.query_row(
            "SELECT coalesce(max(ts),0) FROM (
                SELECT json_extract(row_json,'$.ts') AS ts FROM plan_strides
                WHERE plan_id=?1 AND json_extract(row_json,'$.kind') IN ('map_requested','map_generated')
                UNION ALL SELECT json_extract(row_json,'$.ts') FROM plan_task_milestones
                WHERE plan_id=?1 AND json_extract(row_json,'$.kind') IN ('map_requested','map_generated'))",
            [plan_id], |row| row.get(0),
        ).map_err(db_error)?;
        if latest_map > 0 {
            plan.strides.push(Milestone {
                id: "sql-notice-map-marker".into(),
                from: Default::default(),
                kind: "map_requested".into(),
                text: String::new(),
                ts: latest_map,
                links: vec![],
                loop_id: None,
                iteration: None,
                canvas: Default::default(),
                attachments: vec![],
            });
        }
        let mut finished_lanes = 0;
        let mut lane_candidates = Vec::new();
        {
            record_query();
            let mut statement = transaction.prepare_cached(LANES).map_err(db_error)?;
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
        transaction.commit().map_err(db_error)?;
        Ok(PlanNoticeRead {
            plan,
            finished_lanes,
            lane_candidates,
        })
    }
}
