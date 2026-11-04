use super::read_projections::{column, decode, record_query};
use super::{db_error, PlanDatabase};
use crate::plan::{ControlLoop, CuratorRound, Milestone, Plan, Task};
use crate::service_plan::{CuratorAuthorityRead, CuratorScheduleRead, PlanError};
use rusqlite::{Connection, OpenFlags, OptionalExtension};

impl PlanDatabase {
    pub(in crate::service_plan) fn curator_authority_projection(
        &self,
        plan_id: &str,
    ) -> Result<CuratorAuthorityRead, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let encoded: String = connection.query_row(
            "SELECT json_object('owner',json_extract(header_json,'$.owner'),
                'running',json_extract(header_json,'$.control_loop.running')) FROM plans WHERE id=?1",
            [plan_id], |row| column(row, 0),
        ).optional().map_err(db_error)?.ok_or(PlanError::NotFound)?;
        decode(plan_id, &encoded)
    }

    pub(in crate::service_plan) fn curator_schedule_projection(
        &self,
        plan_id: &str,
    ) -> Result<CuratorScheduleRead, PlanError> {
        let connection = Connection::open_with_flags(
            self.path(),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(db_error)?;
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(db_error)?;
        connection
            .execute_batch("BEGIN DEFERRED TRANSACTION")
            .map_err(db_error)?;
        record_query();
        let header: String = connection
            .query_row(
                "SELECT json_object('id',id,'title',title,'project_path',project_path,
                'created_at',0,'updated_at',updated_at,'status',status,
                'team_id',team_id,'mode',json_extract(header_json,'$.mode'),
                'owner',json_extract(header_json,'$.owner'),
                'task_groups',json_extract(header_json,'$.task_groups'),
                'control_loop',CASE WHEN json_type(header_json,'$.control_loop.rounds') IS NULL
                    OR json_type(header_json,'$.control_loop.rounds')='array'
                    THEN json_remove(json_extract(header_json,'$.control_loop'),'$.rounds')
                    ELSE json_extract(header_json,'$.control_loop') END)
             FROM plans WHERE id=?1",
                [plan_id],
                |row| column(row, 0),
            )
            .optional()
            .map_err(db_error)?
            .ok_or(PlanError::NotFound)?;
        let mut plan: Plan = decode(plan_id, &header)?;
        {
            record_query();
            let mut statement = connection
                .prepare_cached(
                    "SELECT json_object('id',task_id,'name','','type','',
                    'status',status,'group',json_extract(row_json,'$.group'),
                    'status_changed_at',coalesce(json_extract(row_json,'$.status_changed_at'),0))
                 FROM plan_tasks WHERE plan_id=?1 ORDER BY position",
                )
                .map_err(db_error)?;
            for row in statement
                .query_map([plan_id], |row| column::<String>(row, 0))
                .map_err(db_error)?
            {
                plan.tasks
                    .push(decode::<Task>(plan_id, &row.map_err(db_error)?)?);
            }
        }
        record_query();
        let closed: Option<String> = connection
            .query_row(
                "SELECT json_remove(row_json,'$.text','$.attachments','$.canvas')
             FROM plan_strides WHERE plan_id=?1 AND json_extract(row_json,'$.kind')='group_close'
             ORDER BY json_extract(row_json,'$.ts') DESC,seq DESC LIMIT 1",
                [plan_id],
                |row| column(row, 0),
            )
            .optional()
            .map_err(db_error)?;
        if let Some(closed) = closed {
            plan.strides.push(decode::<Milestone>(plan_id, &closed)?);
        }
        Ok(CuratorScheduleRead { plan, connection })
    }
}

impl CuratorScheduleRead {
    /// Applies scheduler metadata only if the complete original control loop still matches.
    /// Keep this read alive for one synchronous preparation, never across an await.
    pub fn apply_control_loop(
        &self,
        current: &mut ControlLoop,
        mut next: ControlLoop,
    ) -> Result<bool, PlanError> {
        if !next.rounds.is_empty() {
            return Err(PlanError::Validation(
                "Scheduler metadata contains round history".into(),
            ));
        }
        let metadata_matches = {
            let without_rounds = WithoutRounds::new(current);
            *without_rounds.control == self.plan.control_loop
        };
        if !metadata_matches {
            return Ok(false);
        }
        record_query();
        let mut statement = self
            .connection
            .prepare_cached(
                "SELECT r.value FROM plans p,json_each(p.header_json,'$.control_loop.rounds') r
             WHERE p.id=?1 ORDER BY r.key",
            )
            .map_err(db_error)?;
        let mut rows = statement.query([&self.plan.id]).map_err(db_error)?;
        let mut index = 0;
        while let Some(row) = rows.next().map_err(db_error)? {
            let encoded: String = column(row, 0).map_err(db_error)?;
            let round: CuratorRound = decode(&self.plan.id, &encoded)?;
            if current.rounds.get(index) != Some(&round) {
                return Ok(false);
            }
            index += 1;
        }
        if index != current.rounds.len() {
            return Ok(false);
        }
        next.rounds = std::mem::take(&mut current.rounds);
        *current = next;
        Ok(true)
    }
}

struct WithoutRounds<'a> {
    control: &'a mut ControlLoop,
    rounds: Vec<CuratorRound>,
}

impl<'a> WithoutRounds<'a> {
    fn new(control: &'a mut ControlLoop) -> Self {
        Self {
            rounds: std::mem::take(&mut control.rounds),
            control,
        }
    }
}

impl Drop for WithoutRounds<'_> {
    fn drop(&mut self) {
        self.control.rounds = std::mem::take(&mut self.rounds);
    }
}
