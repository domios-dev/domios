//! The production legacy codecs behind the Storage importer. They decode captured bytes
//! only: no file, scheduler, sender or agent is reachable from here.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

use crate::cron::store::{
    decode_legacy_cron_file, CronJob, CronRunInfo, CronRunStatus, LegacyCronError,
};
use crate::sentinel::storage::legacy_import;
use crate::storage::{
    DecodeFailure, DecodeFailureKind, DomainImport, IgnoredKeys, ImportDomain, ImportOutcome,
    ImportRequest, LegacyDecoder, MigrationError, Storage,
};

const CRON_FILE_KEYS: [&str; 3] = ["version", "jobs", "history"];

/// Decoders of every legacy domain this build imports.
pub(crate) struct LegacyCodecs;

impl LegacyDecoder for LegacyCodecs {
    fn decode(&self, domain: ImportDomain, bytes: &[u8]) -> Result<DomainImport, DecodeFailure> {
        json_shape(bytes)?;
        match domain {
            ImportDomain::Cron => decode_legacy_cron_file(bytes)
                .map(DomainImport::Cron)
                .map_err(cron_failure),
            ImportDomain::Sentinel => {
                let text = std::str::from_utf8(bytes).map_err(|error| {
                    DecodeFailure::new(DecodeFailureKind::Malformed, error.to_string())
                })?;
                legacy_import(text)
                    .map(DomainImport::Sentinel)
                    .map_err(|detail| DecodeFailure::new(DecodeFailureKind::Malformed, detail))
            }
            _ => Err(DecodeFailure::new(
                DecodeFailureKind::Unsupported,
                "no legacy decoder for this domain",
            )),
        }
    }

    fn ignored_keys(&self, domain: ImportDomain, bytes: &[u8]) -> Vec<IgnoredKeys> {
        match domain {
            ImportDomain::Cron => ignored_cron_keys(bytes),
            _ => Vec::new(),
        }
    }
}

/// A run with every optional field present, so its serialized form names every kept key.
fn full_run() -> CronRunInfo {
    CronRunInfo {
        run_id: String::new(),
        started_at: 0,
        finished_at: Some(0),
        status: CronRunStatus::Success,
        tab_id: Some(String::new()),
        session_id: Some(String::new()),
        cwd: Some(String::new()),
        summary: Some(String::new()),
        report_delivery: Some(String::new()),
    }
}

/// A job with every field present; a new field fails to compile here until it is listed.
fn full_job() -> CronJob {
    CronJob {
        id: String::new(),
        name: String::new(),
        description: Some(String::new()),
        workspace_id: String::new(),
        team_id: Some(String::new()),
        schedule: String::new(),
        enabled: true,
        agent: String::new(),
        model: Some(String::new()),
        reasoning_effort: Some(String::new()),
        task: String::new(),
        cwd: Some(String::new()),
        role: Some(String::new()),
        skill: Some(String::new()),
        creator_tab_id: Some(String::new()),
        report_target_tab_id: Some(String::new()),
        oneshot: true,
        max_history: 1,
        created_at: 0,
        last_run: Some(full_run()),
        next_run: Some(0),
        jitter_max_ms: Some(0),
        tz: Some(String::new()),
        timeout_seconds: Some(0),
        consecutive_failures: 1,
        protected: true,
    }
}

fn serialized_keys<T: serde::Serialize>(value: &T) -> BTreeSet<String> {
    match serde_json::to_value(value) {
        Ok(Value::Object(map)) => map.into_iter().map(|(key, _)| key).collect(),
        _ => BTreeSet::new(),
    }
}

fn unknown(
    object: &Map<String, Value>,
    known: &BTreeSet<String>,
    prefix: &str,
    into: &mut Vec<String>,
) {
    let extra = object.keys().filter(|key| !known.contains(*key));
    into.extend(extra.map(|key| format!("{prefix}{key}")));
}

/// Keys of crons.json the typed decoder drops, by job id (empty for the file envelope).
fn ignored_cron_keys(bytes: &[u8]) -> Vec<IgnoredKeys> {
    let Ok(Value::Object(file)) = serde_json::from_slice::<Value>(bytes) else {
        return Vec::new();
    };
    let job_keys = serialized_keys(&full_job());
    let run_keys = serialized_keys(&full_run());
    let file_keys: BTreeSet<String> = CRON_FILE_KEYS.iter().map(|key| key.to_string()).collect();
    let mut owners: BTreeMap<String, Vec<String>> = BTreeMap::new();
    unknown(
        &file,
        &file_keys,
        "",
        owners.entry(String::new()).or_default(),
    );
    let jobs = file
        .get("jobs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten();
    for job in jobs.filter_map(Value::as_object) {
        let owner = job.get("id").and_then(Value::as_str).unwrap_or_default();
        let keys = owners.entry(owner.to_string()).or_default();
        unknown(job, &job_keys, "", keys);
        if let Some(run) = job.get("last_run").and_then(Value::as_object) {
            unknown(run, &run_keys, "last_run.", keys);
        }
    }
    let histories = file
        .get("history")
        .and_then(Value::as_object)
        .into_iter()
        .flatten();
    for (owner, runs) in histories {
        let keys = owners.entry(owner.clone()).or_default();
        for run in runs
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_object)
        {
            let run_id = run
                .get("run_id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            unknown(run, &run_keys, &format!("history.{run_id}."), keys);
        }
    }
    owners
        .into_iter()
        .filter(|(_, keys)| !keys.is_empty())
        .map(|(owner, mut keys)| {
            keys.sort();
            keys.dedup();
            IgnoredKeys { owner, keys }
        })
        .collect()
}

/// Bytes that end inside a value are a partial write, not a malformed file.
fn json_shape(bytes: &[u8]) -> Result<(), DecodeFailure> {
    match serde_json::from_slice::<serde::de::IgnoredAny>(bytes) {
        Ok(_) => Ok(()),
        Err(error) if error.is_eof() => Err(DecodeFailure::new(
            DecodeFailureKind::Partial,
            error.to_string(),
        )),
        Err(error) => Err(DecodeFailure::new(
            DecodeFailureKind::Malformed,
            error.to_string(),
        )),
    }
}

fn cron_failure(error: LegacyCronError) -> DecodeFailure {
    let kind = match &error {
        LegacyCronError::Unreadable(_) | LegacyCronError::Corrupt(_) => {
            DecodeFailureKind::Malformed
        }
        LegacyCronError::Unsupported(_) => DecodeFailureKind::Unsupported,
        LegacyCronError::Conflict(_) => DecodeFailureKind::Conflict,
    };
    DecodeFailure::new(kind, error.to_string())
}

/// Imports an inert copy of an instance root. Blocking: call it off the async runtime.
pub(crate) fn import_copied_root(
    storage: &Storage,
    request: &ImportRequest,
) -> Result<ImportOutcome, MigrationError> {
    storage.import_legacy_root(request, &LegacyCodecs)
}
