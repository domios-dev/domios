use super::super::CronFile;
use super::codec::{job_record, run_record};
use crate::storage::{CronHistoryImport, CronImport, CronJobId, CronJobImport, StorageError};
use std::{fmt, path::Path};

/// Why an original crons.json cannot be imported; a genuinely absent file is not one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegacyCronError {
    Unreadable(String),
    Corrupt(String),
    Unsupported(String),
    /// The original holds facts that cannot be stored as given.
    Conflict(String),
}

impl fmt::Display for LegacyCronError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LegacyCronError::Unreadable(detail) => write!(f, "crons.json unreadable: {detail}"),
            LegacyCronError::Corrupt(detail) => write!(f, "crons.json corrupt: {detail}"),
            LegacyCronError::Unsupported(detail) => write!(f, "crons.json unsupported: {detail}"),
            LegacyCronError::Conflict(detail) => write!(f, "crons.json conflict: {detail}"),
        }
    }
}

/// Reads the original without recovery, quarantine or rewrite; `Ok(None)` only when the
/// file is genuinely absent.
pub fn read_legacy_cron_file(path: &Path) -> Result<Option<CronImport>, LegacyCronError> {
    match std::fs::read(path) {
        Ok(bytes) => decode_legacy_cron_file(&bytes).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(LegacyCronError::Unreadable(error.to_string())),
    }
}

/// Decodes original bytes into a conserved import: definition order, each history list
/// in order, nulls and the last-run mirror as stored. Nothing is rescheduled.
pub fn decode_legacy_cron_file(bytes: &[u8]) -> Result<CronImport, LegacyCronError> {
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|error| LegacyCronError::Corrupt(error.to_string()))?;
    match value.get("version").and_then(serde_json::Value::as_str) {
        Some("1") => {}
        Some(other) => return Err(LegacyCronError::Unsupported(format!("version {other}"))),
        None => return Err(LegacyCronError::Corrupt("missing version".to_string())),
    }
    let file: CronFile = serde_json::from_value(value)
        .map_err(|error| LegacyCronError::Corrupt(error.to_string()))?;
    let conflict = |error: StorageError| LegacyCronError::Conflict(error.to_string());
    let mut jobs = Vec::with_capacity(file.jobs.len());
    for job in &file.jobs {
        jobs.push(CronJobImport {
            record: job_record(job).map_err(conflict)?,
            last_run: job
                .last_run
                .as_ref()
                .map(run_record)
                .transpose()
                .map_err(conflict)?,
        });
    }
    let mut histories = Vec::with_capacity(file.history.len());
    for (job_id, runs) in &file.history {
        histories.push(CronHistoryImport {
            job_id: CronJobId::new(job_id.clone()).map_err(conflict)?,
            runs: runs
                .iter()
                .map(run_record)
                .collect::<Result<_, _>>()
                .map_err(conflict)?,
        });
    }
    // History is a map: sorting by identity makes the import deterministic.
    histories.sort_by(|a, b| a.job_id.cmp(&b.job_id));
    let import = CronImport { jobs, histories };
    import.validate().map_err(conflict)?;
    Ok(import)
}
