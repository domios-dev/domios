#[cfg(any(test, feature = "test-support"))]
use super::ReminderPlanRead;
use crate::plan::{QaState, ReviewStage, TaskStatus};
use crate::service_plan::state::{project_task_facts, DependencyWait};
use crate::service_plan::PlanError;
use rusqlite::functions::{Aggregate, Context, FunctionFlags};
use rusqlite::{Connection, Result};
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use std::fmt::Write;

mod hashing;

fn error(error: impl std::error::Error + Send + Sync + 'static) -> rusqlite::Error {
    rusqlite::Error::UserFunctionError(Box::new(error))
}

fn text<'a>(context: &'a Context<'_>, index: usize) -> Result<Option<&'a str>> {
    match context.get_raw(index) {
        rusqlite::types::ValueRef::Null => Ok(None),
        rusqlite::types::ValueRef::Text(bytes) => {
            std::str::from_utf8(bytes).map(Some).map_err(error)
        }
        _ => Err(error(std::io::Error::other("invalid reminder text"))),
    }
}

fn enumeration<T: DeserializeOwned>(value: &str) -> Result<T> {
    T::deserialize(serde::de::value::StrDeserializer::<serde::de::value::Error>::new(value))
        .map_err(error)
}

pub(super) fn register(
    connection: &Connection,
    scrubber: fn(&str) -> String,
) -> std::result::Result<(), PlanError> {
    hashing::register(connection)?;
    connection
        .create_scalar_function(
            "domios_reminder_active",
            4,
            FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
            |context| {
                let status: crate::plan::PlanStatus = enumeration(text(context, 0)?.unwrap_or(""))?;
                let mode: crate::plan::PlanMode =
                    enumeration(text(context, 1)?.unwrap_or("planning"))?;
                let control: crate::plan::ControlLoopMode =
                    enumeration(text(context, 2)?.unwrap_or("curator"))?;
                Ok(super::active_execution(
                    &status,
                    &mode,
                    control,
                    context.get(3)?,
                ))
            },
        )
        .map_err(super::super::super::db_error)?;
    let flags = FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC;
    connection
        .create_scalar_function("domios_reminder_scrubbed", 1, flags, move |context| {
            Ok(text(context, 0)?.map(scrubber))
        })
        .map_err(super::super::super::db_error)?;
    connection
        .create_scalar_function("domios_reminder_projection", 7, flags, |context| {
            let status: TaskStatus = enumeration(text(context, 0)?.unwrap_or(""))?;
            let review: ReviewStage = enumeration(text(context, 1)?.unwrap_or("review"))?;
            let qa: Option<QaState> = text(context, 2)?.map(enumeration).transpose()?;
            let waits: Vec<DependencyWait> =
                serde_json::from_str(text(context, 3)?.unwrap_or("[]")).map_err(error)?;
            let projection = project_task_facts(
                &status,
                waits,
                &review,
                &qa,
                context.get(4)?,
                context.get(5)?,
                context.get(6)?,
            );
        let waits=projection.dependency_waits.iter().map(|wait|serde_json::json!({"task_id":wait.task_id,"status":wait.status,"missing":wait.missing})).collect::<Vec<_>>();
        serde_json::to_string(&serde_json::json!({"presentation":projection.presentation,"dependency_waits":waits,"actionable":projection.needs_orchestrator_action()})).map_err(error)
        })
        .map_err(super::super::super::db_error)?;
    connection
        .create_aggregate_function("domios_reminder_state_digest", 5, flags, StateDigest)
        .map_err(super::super::super::db_error)
}

struct StateDigest;

struct State {
    hash: Sha256,
    tasks: usize,
}

struct HashText<'a>(&'a mut Sha256);

impl Write for HashText<'_> {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        self.0.update(text.as_bytes());
        Ok(())
    }
}

impl Aggregate<State, String> for StateDigest {
    fn init(&self, _: &mut Context<'_>) -> Result<State> {
        Ok(State {
            hash: Sha256::new(),
            tasks: 0,
        })
    }

    fn step(&self, context: &mut Context<'_>, state: &mut State) -> Result<()> {
        let mut output = HashText(&mut state.hash);
        if state.tasks == 0 {
            output
                .write_str(text(context, 0)?.unwrap_or(""))
                .map_err(error)?;
        }
        let Some(id) = text(context, 1)? else {
            return Ok(());
        };
        if state.tasks > 0 {
            output.write_char(',').map_err(error)?;
        }
        let presentation: crate::service_plan::state::TaskPresentation =
            enumeration(text(context, 2)?.unwrap_or(""))?;
        write!(output, "{id}:{presentation:?}:{:?}:", text(context, 3)?).map_err(error)?;
        let waits: Vec<super::ReminderDependencyRead> =
            serde_json::from_str(text(context, 4)?.unwrap_or("[]")).map_err(error)?;
        for (position, wait) in waits.iter().enumerate() {
            if position > 0 {
                output.write_char('|').map_err(error)?;
            }
            write!(
                output,
                "{}:{}",
                wait.task_id,
                wait.status
                    .as_ref()
                    .map(TaskStatus::key)
                    .unwrap_or("missing")
            )
            .map_err(error)?;
        }
        state.tasks += 1;
        Ok(())
    }

    fn finalize(&self, _: &mut Context<'_>, state: Option<State>) -> Result<String> {
        let state =
            state.ok_or_else(|| error(std::io::Error::other("missing reminder state prefix")))?;
        Ok(format!("{:x}", state.hash.finalize()))
    }
}

#[cfg(any(test, feature = "test-support"))]
pub(super) fn state_key(
    connection: &Connection,
    read: &ReminderPlanRead,
) -> std::result::Result<String, PlanError> {
    state_key_fields(connection, &read.id, &read.status, &read.mode)
}

pub(super) fn state_key_fields(
    connection: &Connection,
    id: &str,
    status: &crate::plan::PlanStatus,
    mode: &crate::plan::PlanMode,
) -> std::result::Result<String, PlanError> {
    super::record_query();
    let prefix = format!("owner:{}:Orchestrator:{:?}:{:?}:", id, status, mode);
    let facts = super::super::super::curator_notice_queries::reminder_task_facts();
    let sql=format!("{facts}, projected AS MATERIALIZED (
        SELECT t.task_id,t.position,json_extract(t.row_json,'$.status_label') AS status_label,
            domios_reminder_projection(t.status,coalesce(json_extract(t.row_json,'$.review_stage'),'review'),json_extract(t.row_json,'$.qa_state'),
                (SELECT json_group_array(json_object('task_id',dep.value,'name','', 'status',target.status,'missing',json(CASE WHEN target.task_id IS NULL THEN 'true' ELSE 'false' END)))
                 FROM json_each(coalesce(t.row_json->'$.depends_on','[]')) dep LEFT JOIN dependency_targets target ON target.task_id=dep.value
                 WHERE target.task_id IS NULL OR target.status NOT IN ('done','cancelled')),
                coalesce(json_extract(t.row_json,'$.interruption.handoff_required'),0),
                parent.parent_id IS NOT NULL,json_extract(f.decision,'$.parked')) AS projection
        FROM plan_tasks t JOIN facts f ON f.task_id=t.task_id LEFT JOIN open_parents parent ON parent.parent_id=t.task_id WHERE t.plan_id=?1
    ) SELECT domios_reminder_state_digest(?3,task_id,presentation,status_label,waits) FROM (
        SELECT task_id,json_extract(projection,'$.presentation') AS presentation,status_label,projection->'$.dependency_waits' AS waits,position FROM projected
        UNION ALL SELECT NULL,NULL,NULL,NULL,0 WHERE NOT EXISTS(SELECT 1 FROM plan_tasks WHERE plan_id=?1) ORDER BY position)");
    connection
        .query_row(
            &sql,
            rusqlite::params![id, super::super::super::status_queries::WHITESPACE, prefix],
            |row| super::column(row, 0),
        )
        .map_err(super::super::super::db_error)
}
