use super::read_projections::{column, record_query};
use super::{db_error, PlanDatabase};
use crate::plan::task_key;
use crate::service_plan::{ports::TaskKeyScope, PlanError};
use rusqlite::{functions::FunctionFlags, Connection, OptionalExtension};
use std::collections::HashMap;

pub(in crate::service_plan) struct TaskKeyIssueInput {
    pub(in crate::service_plan) scope: TaskKeyScope,
    pub(in crate::service_plan) high_water: HashMap<String, u64>,
}

pub(super) fn register_functions(connection: &Connection) -> Result<(), PlanError> {
    let flags = FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC;
    connection
        .create_scalar_function("domios_key_prefix", 1, flags, |context| {
            let text = context
                .get_raw(0)
                .as_str()
                .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))?;
            Ok(task_key::parse_key(text).map(|(prefix, _)| prefix.to_string()))
        })
        .map_err(db_error)?;
    connection
        .create_scalar_function("domios_key_number", 1, flags, |context| {
            let text = context
                .get_raw(0)
                .as_str()
                .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))?;
            Ok(task_key::parse_key(text).map(|(_, number)| format!("{number:020}")))
        })
        .map_err(db_error)
}

fn high_water(connection: &Connection) -> Result<HashMap<String, u64>, PlanError> {
    record_query();
    let mut statement = connection
        .prepare_cached(
            "WITH keys AS MATERIALIZED (
        SELECT domios_key_prefix(json_extract(row_json,'$.key')) AS prefix,
            domios_key_number(json_extract(row_json,'$.key')) AS number
        FROM plan_tasks WHERE json_type(row_json,'$.key')='text')
        SELECT prefix,max(number) FROM keys WHERE prefix IS NOT NULL GROUP BY prefix",
        )
        .map_err(db_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((column::<String>(row, 0)?, column::<String>(row, 1)?))
        })
        .map_err(db_error)?;
    rows.map(|row| {
        let (prefix, number) = row.map_err(db_error)?;
        Ok((
            prefix,
            number
                .parse()
                .map_err(|error: std::num::ParseIntError| PlanError::Io(error.to_string()))?,
        ))
    })
    .collect()
}

impl PlanDatabase {
    pub(in crate::service_plan) fn task_key_high_water(
        &self,
    ) -> Result<HashMap<String, u64>, PlanError> {
        high_water(&self.connection.lock())
    }

    pub(in crate::service_plan) fn task_key_issue_input(
        &self,
        id: &str,
    ) -> Result<Option<TaskKeyIssueInput>, PlanError> {
        let mut guard = self.connection.lock();
        let connection = guard.transaction().map_err(db_error)?;
        record_query();
        let scope = connection.query_row("SELECT id,team_id,project_path,coalesce(json_extract(header_json,'$.canonical_project_path'),'') FROM plans WHERE id=?1", [id], |row| Ok(TaskKeyScope {
            plan_id:column(row,0)?,team_id:column(row,1)?,project_path:column(row,2)?,canonical_project_path:column(row,3)?
        })).optional().map_err(db_error)?;
        let input = scope
            .map(|scope| {
                high_water(&connection).map(|high_water| TaskKeyIssueInput { scope, high_water })
            })
            .transpose()?;
        connection.commit().map_err(db_error)?;
        Ok(input)
    }

    pub(in crate::service_plan) fn task_display_key(
        &self,
        id: &str,
        reference: &str,
    ) -> Result<Option<String>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let selected:Option<(String,Option<String>)> = connection.query_row("WITH exact AS MATERIALIZED (
            SELECT task_id,json_extract(row_json,'$.key') AS display_key FROM plan_tasks WHERE plan_id=?1 AND task_id=?2
        ), matched AS MATERIALIZED (
            SELECT task_id,display_key FROM exact UNION ALL
            SELECT task_id,json_extract(row_json,'$.key') AS display_key FROM plan_tasks WHERE plan_id=?1 AND NOT EXISTS(SELECT 1 FROM exact)
                AND domios_task_ref_rank(task_id,json_extract(row_json,'$.handle'),json_extract(row_json,'$.key'),?2)=1
        ) SELECT task_id,display_key FROM matched WHERE (SELECT count(*) FROM matched)=1",
            rusqlite::params![id,reference],|row|Ok((column(row,0)?,column(row,1)?))).optional().map_err(db_error)?;
        Ok(selected.map(|(id, key)| key.unwrap_or_else(|| task_key::fallback_key(&id))))
    }
}
