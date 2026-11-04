use super::read_projections::{column, decode, record_query};
use super::{db_error, PlanDatabase};
use crate::plan::task_key;
use crate::service_plan::resolve::{match_rank, RefCandidate, TaskReferenceRead};
use crate::service_plan::PlanError;
use rusqlite::{
    functions::{Context, FunctionFlags},
    types::ValueRef,
    Connection,
};

fn text<'a>(context: &'a Context<'_>, index: usize) -> rusqlite::Result<Option<&'a str>> {
    match context.get_raw(index) {
        ValueRef::Null => Ok(None),
        ValueRef::Text(bytes) => std::str::from_utf8(bytes)
            .map(Some)
            .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error))),
        _ => Err(rusqlite::Error::UserFunctionError(
            "invalid task reference text field".into(),
        )),
    }
}

pub(super) fn register_functions(connection: &Connection) -> Result<(), PlanError> {
    connection
        .create_scalar_function(
            "domios_task_ref_rank",
            4,
            FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
            |context| {
                let id = text(context, 0)?.ok_or_else(|| {
                    rusqlite::Error::UserFunctionError("missing task reference id".into())
                })?;
                let handle = text(context, 1)?;
                let key = text(context, 2)?;
                let reference = text(context, 3)?.ok_or_else(|| {
                    rusqlite::Error::UserFunctionError("missing task reference".into())
                })?;
                let fallback = key.is_none().then(|| task_key::fallback_key(id));
                Ok(match_rank(id, handle, key.or(fallback.as_deref()), reference).map(i64::from))
            },
        )
        .map_err(db_error)
}

#[derive(serde::Deserialize)]
struct Diagnostic {
    handle: Option<String>,
    key: Option<String>,
    label: String,
}

impl PlanDatabase {
    pub(in crate::service_plan) fn resolve_task_reference(
        &self,
        plan_id: Option<&str>,
        reference: &str,
    ) -> Result<Vec<TaskReferenceRead>, PlanError> {
        let connection = self.connection.lock();
        let scope = if plan_id.is_some() {
            "t.plan_id=?1"
        } else {
            "?1 IS NULL"
        };
        let sql=format!("WITH exact AS MATERIALIZED (
            SELECT t.plan_id,t.task_id,t.position FROM plan_tasks t
            WHERE {scope} AND t.task_id=?2 ORDER BY t.plan_id,t.position LIMIT 1
        ),ranked AS MATERIALIZED (
            SELECT t.plan_id,t.task_id,t.position,
                domios_task_ref_rank(t.task_id,json_extract(t.row_json,'$.handle'),json_extract(t.row_json,'$.key'),?2) AS tier
            FROM plan_tasks t WHERE {scope} AND NOT EXISTS(SELECT 1 FROM exact)
        ),matched AS (
            SELECT plan_id,task_id,position FROM exact
            UNION ALL SELECT plan_id,task_id,position FROM ranked WHERE tier=(SELECT min(tier) FROM ranked)
        ),counted AS (SELECT *,count(*) OVER() AS total FROM matched)
        SELECT m.plan_id,m.task_id,CASE WHEN m.total>1 THEN (
            SELECT json_object('handle',json_extract(t.row_json,'$.handle'),'key',json_extract(t.row_json,'$.key'),
                'label',coalesce(json_extract(t.row_json,'$.name'),'')) FROM plan_tasks t
            WHERE t.plan_id=m.plan_id AND t.task_id=m.task_id) END
        FROM counted m ORDER BY m.plan_id,m.position");
        record_query();
        let mut statement = connection.prepare_cached(&sql).map_err(db_error)?;
        let rows = statement
            .query_map(rusqlite::params![plan_id, reference], |row| {
                Ok((
                    column::<String>(row, 0)?,
                    column::<String>(row, 1)?,
                    column::<Option<String>>(row, 2)?,
                ))
            })
            .map_err(db_error)?;
        rows.map(|row| {
            let (plan_id, task_id, diagnostic) = row.map_err(db_error)?;
            let candidate = diagnostic
                .map(|encoded| -> Result<RefCandidate, PlanError> {
                    let diagnostic: Diagnostic = decode(&plan_id, &encoded)?;
                    Ok(RefCandidate {
                        id: task_id.clone(),
                        handle: diagnostic.handle,
                        key: Some(
                            diagnostic
                                .key
                                .unwrap_or_else(|| task_key::fallback_key(&task_id)),
                        ),
                        label: diagnostic.label,
                        plan_id: Some(plan_id.clone()),
                    })
                })
                .transpose()?;
            Ok(TaskReferenceRead {
                plan_id,
                task_id,
                candidate,
            })
        })
        .collect()
    }
}
