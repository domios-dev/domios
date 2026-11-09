use crate::plan::{CuratorRound, GateStatus, TaskStatus};
use crate::service_plan::curator::{digest, movement, synthesis};
use crate::service_plan::store_db::db_error;
use crate::service_plan::PlanError;
use rusqlite::{
    functions::{Context, FunctionFlags},
    Connection,
};
use std::collections::BTreeMap;

fn json_value<T: serde::de::DeserializeOwned>(
    context: &Context<'_>,
    index: usize,
) -> rusqlite::Result<T> {
    let text = context
        .get_raw(index)
        .as_str()
        .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))?;
    serde_json::from_str(text).map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))
}
fn encode(value: &impl serde::Serialize) -> rusqlite::Result<String> {
    serde_json::to_string(value)
        .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))
}
fn enumeration<T: serde::de::DeserializeOwned>(text: &str) -> rusqlite::Result<T> {
    T::deserialize(serde::de::value::StrDeserializer::<serde::de::value::Error>::new(text))
        .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))
}

pub(super) fn register(connection: &Connection) -> Result<(), PlanError> {
    let flags = FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC;
    connection
        .create_scalar_function("domios_curator_task_rows", 11, flags, |context| {
            let id = context
                .get_raw(0)
                .as_str()
                .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))?;
            let name = context
                .get_raw(1)
                .as_str()
                .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))?;
            let status: TaskStatus = enumeration(
                context
                    .get_raw(2)
                    .as_str()
                    .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))?,
            )?;
            let created = json_value::<u64>(context, 3)?;
            let changed = json_value::<u64>(context, 4)?;
            let waits = json_value::<Vec<String>>(context, 5)?;
            let owner: Option<String> = context.get(6)?;
            let prior: Option<String> = context.get(7)?;
            let actionable = context.get::<bool>(8)?;
            let since = json_value::<u64>(context, 9)?;
            let now = json_value::<u64>(context, 10)?;
            let mut rows = BTreeMap::new();
            digest::append_metadata_task_rows(
                &mut rows,
                digest::MetadataTaskInput {
                    id,
                    name,
                    status: &status,
                    changed_at: changed,
                    prior: prior.as_deref(),
                    actionable,
                },
                since,
            );
            movement::append_task_rows(
                &mut rows,
                movement::TaskMovementInput {
                    id,
                    status: &status,
                    created_at: created,
                    status_changed_at: changed,
                    waits,
                    owner: owner.as_deref(),
                },
                now,
            );
            encode(&rows)
        })
        .map_err(db_error)?;
    connection
        .create_scalar_function("domios_curator_age", 3, flags, |context| {
            let age = movement::task_age(
                json_value(context, 0)?,
                json_value(context, 1)?,
                json_value(context, 2)?,
            );
            Ok(format!("{age:020}"))
        })
        .map_err(db_error)?;
    connection
        .create_scalar_function("domios_curator_gate_open", 1, flags, |context| {
            let status: GateStatus = enumeration(&context.get::<String>(0)?)?;
            Ok(status.is_open())
        })
        .map_err(db_error)?;
    connection
        .create_scalar_function("domios_curator_gate_rows", 6, flags, |context| {
            let id: String = context.get(0)?;
            let status: GateStatus = enumeration(&context.get::<String>(1)?)?;
            let asked = json_value(context, 2)?;
            let answer = context
                .get::<Option<String>>(3)?
                .map(|value| serde_json::from_str::<u64>(&value))
                .transpose()
                .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))?;
            let mut rows = BTreeMap::new();
            movement::append_gate_rows(
                &mut rows,
                &id,
                &status,
                asked,
                answer,
                json_value(context, 4)?,
                json_value(context, 5)?,
            );
            encode(&rows)
        })
        .map_err(db_error)?;
    connection
        .create_scalar_function("domios_curator_after", 2, flags, |context| {
            let at: Option<u64> = if matches!(context.get_raw(0), rusqlite::types::ValueRef::Null) {
                None
            } else {
                json_value(context, 0)?
            };
            let since: u64 = json_value(context, 1)?;
            Ok(at.is_some_and(|at| at > since))
        })
        .map_err(db_error)?;
    connection
        .create_scalar_function("domios_curator_elapsed", 2, flags, |context| {
            let at: u64 = json_value(context, 0)?;
            let now: u64 = json_value(context, 1)?;
            Ok(now.saturating_sub(at).to_string())
        })
        .map_err(db_error)?;
    connection
        .create_scalar_function("domios_curator_review_row", 3, flags, |context| {
            let oldest = context
                .get::<String>(2)?
                .parse::<u64>()
                .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))?;
            encode(&movement::review_backlog_row(
                &context.get::<String>(0)?,
                context.get(1)?,
                oldest,
            ))
        })
        .map_err(db_error)?;
    connection
        .create_scalar_function("domios_curator_agent_row", 4, flags, |context| {
            let since: u64 = json_value(context, 2)?;
            let now: u64 = json_value(context, 3)?;
            encode(&digest::Row {
                id: context.get(0)?,
                kind: "agent".into(),
                state: context.get(1)?,
                summary: format!("observed_minutes={}", now.saturating_sub(since) / 60_000),
            })
        })
        .map_err(db_error)?;
    connection
        .create_scalar_function("domios_curator_bounded", 2, flags, |context| {
            let text = context
                .get_raw(0)
                .as_str()
                .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))?;
            Ok(digest::bounded_text(text, context.get(1)?).to_string())
        })
        .map_err(db_error)?;
    connection
        .create_scalar_function("domios_curator_clean", 2, flags, |context| {
            let text = context
                .get_raw(0)
                .as_str()
                .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))?;
            Ok(digest::clean(text, context.get(1)?))
        })
        .map_err(db_error)?;
    connection
        .create_scalar_function("domios_curator_activity", 1, flags, |context| {
            Ok(digest::is_activity_section(&context.get::<String>(0)?))
        })
        .map_err(db_error)?;
    connection
        .create_scalar_function("domios_curator_round_lines", 1, flags, |context| {
            let round: CuratorRound = json_value(context, 0)?;
            let fallback = round.retro.is_empty() && round.learn.is_empty();
            let ended_at = round.ended_at;
            let lines = synthesis::round_history_lines(round, std::iter::empty::<String>());
            encode(&serde_json::json!({"fallback":fallback,"ended_at":ended_at,"lines":lines}))
        })
        .map_err(db_error)?;
    connection
        .create_scalar_function("domios_curator_history_kind", 1, flags, |context| {
            Ok(synthesis::history_learning_kind(&context.get::<String>(0)?))
        })
        .map_err(db_error)?;
    connection
        .create_scalar_function("domios_curator_history_line", 3, flags, |context| {
            Ok(synthesis::history_line(
                &context.get::<String>(0)?,
                &context.get::<String>(1)?,
                &context.get::<String>(2)?,
            ))
        })
        .map_err(db_error)?;
    Ok(())
}
