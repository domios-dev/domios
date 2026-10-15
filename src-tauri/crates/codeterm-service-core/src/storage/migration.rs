//! The production legacy codecs behind the Storage importer. They decode captured bytes
//! only: no file, scheduler, sender or agent is reachable from here.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

use crate::cron::store::{
    decode_legacy_cron_file, CronJob, CronRunInfo, CronRunStatus, LegacyCronError,
};
use crate::storage::{
    DecodeFailure, DecodeFailureKind, DomainImport, IgnoredKeys, ImportDomain, ImportOutcome,
    ImportRequest, LegacyDecoder, MigrationError, Storage,
};
use codeterm_pty::session::store::legacy_decode;
use codeterm_sentinel::storage::{legacy_import, LegacySentinelError};
use codeterm_storage::{
    DeskAgentSessionIdentity, DeskLayoutNode, DeskSnapshot, DeskTab, DeskTeam, DeskTeamLifecycle,
    DeskWorkspace, LiveRootImportRequest, WorkspaceId, WorkspaceRecord,
};

const CRON_FILE_KEYS: [&str; 3] = ["version", "jobs", "history"];

/// Decoders of every legacy domain this build imports.
pub(crate) struct LegacyCodecs;

pub(crate) fn cutover_import_live(storage: &Storage) -> Result<ImportOutcome, MigrationError> {
    storage.import_live_root(&LiveRootImportRequest::new(), &LegacyCodecs)
}

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
                    .map_err(sentinel_failure)
            }
            ImportDomain::Desk => Err(DecodeFailure::new(
                DecodeFailureKind::Unsupported,
                "desk sources require their snapshot and identity catalogue",
            )),
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

    fn decode_desk_snapshot(&self, bytes: &[u8]) -> Result<DeskSnapshot, DecodeFailure> {
        json_shape(bytes)?;
        let text = std::str::from_utf8(bytes)
            .map_err(|error| DecodeFailure::new(DecodeFailureKind::Malformed, error.to_string()))?;
        let decoded = legacy_decode::decode_snapshot(text)
            .map_err(|detail| DecodeFailure::new(DecodeFailureKind::Malformed, detail))?;
        map_desk_snapshot(&decoded.snapshot, decoded.layouts)
    }

    fn decode_desk_identities(
        &self,
        bytes: &[u8],
    ) -> Result<Vec<DeskAgentSessionIdentity>, DecodeFailure> {
        json_shape(bytes)?;
        let text = std::str::from_utf8(bytes)
            .map_err(|error| DecodeFailure::new(DecodeFailureKind::Malformed, error.to_string()))?;
        let mut identities = legacy_decode::decode_identity_catalog(Some(text))
            .map_err(|detail| DecodeFailure::new(DecodeFailureKind::Malformed, detail))?;
        identities.sort_by(|left, right| {
            (&left.provider, &left.session_id).cmp(&(&right.provider, &right.session_id))
        });
        Ok(identities)
    }
}

fn map_desk_snapshot(
    snapshot: &codeterm_pty::session::store::SessionSnapshot,
    layouts: Vec<Vec<DeskLayoutNode>>,
) -> Result<DeskSnapshot, DecodeFailure> {
    if snapshot.workspaces.len() != layouts.len() {
        return Err(DecodeFailure::new(
            DecodeFailureKind::Conflict,
            "desk layout count does not match workspaces",
        ));
    }
    let workspaces = snapshot
        .workspaces
        .iter()
        .zip(layouts)
        .map(|(workspace, layouts)| {
            if workspace.tabs.len() != layouts.len() {
                return Err(DecodeFailure::new(
                    DecodeFailureKind::Conflict,
                    "desk layout count does not match tabs",
                ));
            }
            let mut record = WorkspaceRecord::new(
                WorkspaceId::new(workspace.id.clone()).map_err(|error| {
                    DecodeFailure::new(DecodeFailureKind::Conflict, error.to_string())
                })?,
                workspace.name.clone(),
            );
            record.cwd = workspace.cwd.clone();
            record.provisioned_cwd = workspace.provisioned_cwd.clone();
            record.protected = workspace.protected;
            record.task_key_prefix = workspace.task_keys.prefix.clone();
            record.task_key_next = workspace.task_keys.next;
            let tabs = workspace
                .tabs
                .iter()
                .zip(layouts)
                .map(|(tab, layout)| DeskTab {
                    id: tab.id.clone(),
                    title: tab.title.clone(),
                    description: tab.description.clone(),
                    sort_order: tab.sort_order,
                    layout,
                })
                .collect();
            Ok(DeskWorkspace {
                record,
                sort_order: workspace.sort_order,
                active_tab_id: workspace.active_tab_id.clone(),
                tabs,
            })
        })
        .collect::<Result<Vec<_>, DecodeFailure>>()?;
    Ok(DeskSnapshot {
        version: snapshot.version.clone(),
        timestamp: snapshot.timestamp,
        active_workspace_id: snapshot.active_workspace_id.clone(),
        workspaces,
        teams: snapshot
            .teams
            .iter()
            .map(|team| DeskTeam {
                id: team.id.clone(),
                name: team.name.clone(),
                lifecycle: match team.lifecycle {
                    codeterm_types::team::TeamLifecycle::Forming => DeskTeamLifecycle::Forming,
                    codeterm_types::team::TeamLifecycle::Active => DeskTeamLifecycle::Active,
                    codeterm_types::team::TeamLifecycle::Closing => DeskTeamLifecycle::Closing,
                },
                created_at: team.created_at,
                workspace_id: team.workspace_id.clone(),
            })
            .collect(),
        agent_session_identities: Vec::new(),
    })
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

/// Valid JSON that breaks a Sentinel invariant is a conflict, never recoverable damage.
fn sentinel_failure(error: LegacySentinelError) -> DecodeFailure {
    match error {
        LegacySentinelError::Malformed(detail) => {
            DecodeFailure::new(DecodeFailureKind::Malformed, detail)
        }
        LegacySentinelError::Conflict(detail) => {
            DecodeFailure::new(DecodeFailureKind::Conflict, detail)
        }
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
