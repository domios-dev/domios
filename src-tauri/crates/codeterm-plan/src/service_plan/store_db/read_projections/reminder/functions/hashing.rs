use super::{error, text};
use crate::plan::{
    AttemptInterruptionState, DecisionGate, Milestone, QaState, ReviewEvidence, ReviewStage,
    TaskExecutionReportSnapshot, TaskInterruptionState, TaskStatus,
};
use crate::service_plan::state::DependencyWait;
use crate::service_plan::PlanError;
use rusqlite::{functions::FunctionFlags, Connection};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{self, Write};

struct HashWriter(Sha256);

impl Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn encoded(value: &impl Serialize) -> rusqlite::Result<String> {
    let mut output = HashWriter(Sha256::new());
    serde_json::to_writer(&mut output, value).map_err(error)?;
    Ok(format!("{:x}", output.0.finalize()))
}

fn document_hash<T: DeserializeOwned + Serialize>(raw: &str) -> rusqlite::Result<String> {
    let value: T = serde_json::from_str(raw).map_err(error)?;
    encoded(&value)
}

#[derive(Deserialize)]
struct TaskFingerprint {
    status: TaskStatus,
    status_label: Option<String>,
    pause_reason: Option<String>,
    interruption: TaskInterruptionState,
    review_stage: ReviewStage,
    qa_state: Option<QaState>,
    assigned_tab_id: Option<String>,
    dependency_waits: Vec<DependencyWait>,
}

pub(super) fn register(connection: &Connection) -> Result<(), PlanError> {
    let flags = FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC;
    connection
        .create_scalar_function("domios_reminder_default", 1, flags, |context| {
            match text(context, 0)? {
                Some("task") => {
                    serde_json::to_string(&TaskInterruptionState::default()).map_err(error)
                }
                Some("attempt") => {
                    serde_json::to_string(&AttemptInterruptionState::default()).map_err(error)
                }
                _ => Err(error(io::Error::other("unknown reminder default"))),
            }
        })
        .map_err(super::super::super::super::db_error)?;
    connection
        .create_scalar_function("domios_reminder_block", 2, flags, |context| {
            let kind =
                text(context, 0)?.ok_or_else(|| error(io::Error::other("missing blocker kind")))?;
            let reason = text(context, 1)?
                .ok_or_else(|| error(io::Error::other("missing blocker reason")))?;
            encoded(&(kind, reason))
        })
        .map_err(super::super::super::super::db_error)?;
    connection
        .create_scalar_function("domios_reminder_block3", 3, flags, |context| {
            let kind =
                text(context, 0)?.ok_or_else(|| error(io::Error::other("missing blocker kind")))?;
            let first = text(context, 1)?
                .ok_or_else(|| error(io::Error::other("missing blocker identity")))?;
            let second = text(context, 2)?
                .ok_or_else(|| error(io::Error::other("missing blocker state")))?;
            encoded(&(kind, first, second))
        })
        .map_err(super::super::super::super::db_error)?;
    connection
        .create_scalar_function("domios_reminder_hash", 2, flags, |context| {
            let raw = text(context, 1)?
                .ok_or_else(|| error(io::Error::other("missing reminder fingerprint input")))?;
            match text(context, 0)? {
                Some("task") => {
                    let task: TaskFingerprint = serde_json::from_str(raw).map_err(error)?;
                    encoded(&(
                        &task.status,
                        &task.status_label,
                        &task.pause_reason,
                        &task.interruption,
                        &task.review_stage,
                        &task.qa_state,
                        &task.assigned_tab_id,
                        &task.dependency_waits,
                    ))
                }
                Some("review") => document_hash::<Vec<ReviewEvidence>>(raw),
                Some("attempt") => document_hash::<AttemptInterruptionState>(raw),
                Some("report") => document_hash::<TaskExecutionReportSnapshot>(raw),
                Some("stride") => document_hash::<Milestone>(raw),
                Some("gate") => document_hash::<DecisionGate>(raw),
                _ => Err(error(io::Error::other("unknown reminder fingerprint kind"))),
            }
        })
        .map_err(super::super::super::super::db_error)
}
