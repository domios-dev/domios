use super::read_projections::{column, decode, record_query};
use super::{db_error, PlanDatabase};
use crate::plan::{ControlLoop, CuratorRound, TaskStatus};
use crate::service_plan::{
    CuratorAuthorityRead, CuratorScheduleMetadata, CuratorScheduleRead, PlanError,
};
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
        register_timestamp(&connection)?;
        record_query();
        let header: String = connection
            .query_row(
                "SELECT json_object('id',id,'project_path',project_path,'status',status,
                'team_id',team_id,'mode',json_extract(header_json,'$.mode'),
                'owner',json_extract(header_json,'$.owner'),
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
        let metadata: CuratorScheduleMetadata = decode(plan_id, &header)?;
        let (mut total, mut terminal) = (0_u64, 0_u64);
        {
            record_query();
            let mut statement = connection.prepare_cached(
                "SELECT json_quote(status),count(*) FROM plan_tasks WHERE plan_id=?1 GROUP BY status"
            ).map_err(db_error)?;
            for row in statement
                .query_map([plan_id], |row| {
                    Ok((column::<String>(row, 0)?, row.get::<_, u64>(1)?))
                })
                .map_err(db_error)?
            {
                let (status, count) = row.map_err(db_error)?;
                let status: TaskStatus = decode(plan_id, &status)?;
                total += count;
                if status.is_terminal() {
                    terminal += count;
                }
            }
        }
        let group_close = schedule_group_close(&connection, plan_id)?;
        Ok(CuratorScheduleRead {
            metadata,
            connection,
            complete: total > 0 && terminal == total,
            group_close,
        })
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
            *without_rounds.control == self.metadata.control_loop
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
        let mut rows = statement.query([&self.metadata.id]).map_err(db_error)?;
        let mut index = 0;
        while let Some(row) = rows.next().map_err(db_error)? {
            let encoded: String = column(row, 0).map_err(db_error)?;
            let round: CuratorRound = decode(&self.metadata.id, &encoded)?;
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

pub(super) fn register_timestamp(connection: &Connection) -> Result<(), PlanError> {
    use rusqlite::functions::FunctionFlags;
    connection
        .create_scalar_function(
            "domios_schedule_timestamp",
            1,
            FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
            |context| {
                let value = context
                    .get_raw(0)
                    .as_str()
                    .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))?;
                let timestamp = serde_json::from_str::<u64>(value)
                    .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))?;
                Ok(format!("{timestamp:020}"))
            },
        )
        .map_err(db_error)?;
    connection
        .create_scalar_function(
            "domios_schedule_group_key",
            1,
            FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
            |context| {
                let value = context
                    .get_raw(0)
                    .as_str()
                    .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))?;
                serde_json::from_str::<String>(value)
                    .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))
            },
        )
        .map_err(db_error)?;
    connection
        .create_scalar_function(
            "domios_schedule_task_group",
            1,
            FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
            |context| {
                let value = context.get_raw(0);
                if matches!(value, rusqlite::types::ValueRef::Null) {
                    return Ok(None);
                }
                let value = value
                    .as_str()
                    .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))?;
                serde_json::from_str::<Option<String>>(value)
                    .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))
            },
        )
        .map_err(db_error)
}

pub(super) fn schedule_group_close(
    connection: &Connection,
    plan_id: &str,
) -> Result<Option<u64>, PlanError> {
    record_query();
    let group_close: Option<String> = connection
        .query_row(
            "WITH declared AS MATERIALIZED (
                SELECT domios_schedule_group_key(g.value->'$.key') AS group_key
                FROM plans p,json_each(p.header_json,'$.task_groups') g WHERE p.id=?1
             ), task_groups AS MATERIALIZED (
                SELECT status,domios_schedule_task_group(row_json->'$.group') AS group_key,
                    coalesce(row_json->'$.status_changed_at','0') AS changed
                FROM plan_tasks WHERE plan_id=?1
             ), completed_groups AS (
                SELECT max(domios_schedule_timestamp(changed)) AS ts
                FROM task_groups WHERE group_key IN (SELECT group_key FROM declared)
                GROUP BY group_key
                HAVING sum(status NOT IN ('done','cancelled'))=0 AND sum(status='done')>0
             ), closes AS (
                SELECT ts FROM completed_groups UNION ALL
                SELECT domios_schedule_timestamp(row_json->'$.ts') FROM plan_strides
                    WHERE plan_id=?1 AND json_extract(row_json,'$.kind')='group_close')
             SELECT max(ts) FROM closes",
            [plan_id],
            |row| column(row, 0),
        )
        .map_err(db_error)?;
    let group_close = group_close
        .map(|value| value.parse::<u64>())
        .transpose()
        .map_err(|error| PlanError::Io(error.to_string()))?;
    Ok(group_close)
}
