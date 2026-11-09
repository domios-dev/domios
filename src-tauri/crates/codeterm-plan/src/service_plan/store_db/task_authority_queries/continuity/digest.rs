use crate::plan::{Milestone, Task, TaskExecutionAttempt};
use crate::service_plan::store_db::{db_error, read_projections::decode};
use crate::service_plan::PlanError;
use rusqlite::functions::{Aggregate, Context, FunctionFlags};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::{self, Write};

struct Continuity;

struct State {
    hash: Sha256,
    last_phase: Option<u8>,
}

fn function_error(error: impl std::error::Error + Send + Sync + 'static) -> rusqlite::Error {
    rusqlite::Error::UserFunctionError(Box::new(error))
}

fn typed_value<T: DeserializeOwned + Serialize>(plan: &str, raw: &str) -> rusqlite::Result<Value> {
    let row: T = decode(plan, raw).map_err(function_error)?;
    serde_json::to_value(row).map_err(function_error)
}

impl Aggregate<State, Vec<u8>> for Continuity {
    fn init(&self, _ctx: &mut Context<'_>) -> rusqlite::Result<State> {
        let mut hash = Sha256::new();
        hash.update(b"domios-task-continuity-v1\0");
        Ok(State {
            hash,
            last_phase: None,
        })
    }

    fn step(&self, ctx: &mut Context<'_>, state: &mut State) -> rusqlite::Result<()> {
        let phase: u8 = ctx.get(0)?;
        let raw: String = ctx.get(1)?;
        let plan: String = ctx.get(2)?;
        if (phase == 0 && state.last_phase.is_some())
            || (phase != 0 && state.last_phase.is_none())
            || state.last_phase.is_some_and(|prior| phase < prior)
        {
            return Err(function_error(io::Error::other(
                "task continuity rows are not ordered",
            )));
        }
        let value = match phase {
            0 => typed_value::<Task>(&plan, &raw)?,
            1 => typed_value::<Milestone>(&plan, &raw)?,
            2 => typed_value::<TaskExecutionAttempt>(&plan, &raw)?,
            _ => {
                return Err(function_error(io::Error::other(
                    "unknown task continuity row kind",
                )))
            }
        };
        let mut count = Counter(0);
        canonical_json(&value, &mut count).map_err(function_error)?;
        state.hash.update([phase]);
        state.hash.update(count.0.to_le_bytes());
        canonical_json(&value, &mut HashWriter(&mut state.hash)).map_err(function_error)?;
        state.last_phase = Some(phase);
        Ok(())
    }

    fn finalize(&self, _ctx: &mut Context<'_>, state: Option<State>) -> rusqlite::Result<Vec<u8>> {
        let state = state.ok_or_else(|| {
            function_error(io::Error::other("task continuity has no metadata row"))
        })?;
        Ok(state.hash.finalize().to_vec())
    }
}

struct Counter(u64);

impl Write for Counter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| io::Error::other("task continuity row length overflow"))?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct HashWriter<'a>(&'a mut Sha256);

impl Write for HashWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn canonical_json(value: &Value, out: &mut impl Write) -> io::Result<()> {
    match value {
        Value::Array(values) => {
            out.write_all(b"[")?;
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    out.write_all(b",")?;
                }
                canonical_json(value, out)?;
            }
            out.write_all(b"]")
        }
        Value::Object(fields) => {
            let mut fields: Vec<_> = fields.iter().collect();
            fields.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
            out.write_all(b"{")?;
            for (index, (key, value)) in fields.into_iter().enumerate() {
                if index > 0 {
                    out.write_all(b",")?;
                }
                serde_json::to_writer(&mut *out, key).map_err(io::Error::other)?;
                out.write_all(b":")?;
                canonical_json(value, out)?;
            }
            out.write_all(b"}")
        }
        Value::Number(number) if number.is_f64() && number.as_f64() == Some(0.0) => {
            out.write_all(b"0.0")
        }
        value => serde_json::to_writer(out, value).map_err(io::Error::other),
    }
}

pub(super) fn register(connection: &rusqlite::Connection) -> Result<(), PlanError> {
    connection
        .create_aggregate_function(
            "domios_task_continuity",
            3,
            FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
            Continuity,
        )
        .map_err(db_error)
}
