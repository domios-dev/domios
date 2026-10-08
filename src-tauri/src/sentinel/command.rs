use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::handlers::{Effect, Status as HandlerStatus};
use super::probe::{
    self, cancellation::Cancellation, owner::ProbeResourceOwner, process::ProbeFailure,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Parameters {
    pub command: String,
    pub label: String,
    pub timeout_ms: u64,
}

impl Parameters {
    pub fn validate(&self) -> Result<(), String> {
        if self.command.trim().is_empty()
            || self.command.len() > 8192
            || self.command.contains('\0')
        {
            return Err("command requires nonempty text of at most 8192 bytes without NUL".into());
        }
        if self.label.trim().is_empty()
            || self.label.len() > 256
            || self.label.chars().any(char::is_control)
        {
            return Err("command label requires nonempty display text of at most 256 bytes".into());
        }
        if self.timeout_ms == 0 || u128::from(self.timeout_ms) > probe::PROBE_RUNTIME.as_millis() {
            return Err(
                "command timeout_ms must be positive and within the platform probe runtime".into(),
            );
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Success,
    Nonzero,
    Error,
    Cancelled,
    Deadline,
    Unconfirmed,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capture {
    pub total_bytes: u64,
    pub retained_bytes: u64,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResultMetadata {
    pub status: Status,
    pub exit_code: Option<i32>,
    pub resources_confirmed: bool,
    pub stdout: Option<Capture>,
    pub stderr: Option<Capture>,
    pub started_at: u64,
    pub finished_at: u64,
    pub primary: Option<String>,
    pub cleanup: Vec<String>,
}

impl ResultMetadata {
    pub(super) fn validate(&self) -> Result<(), String> {
        let settled = self.status != Status::Unconfirmed;
        if self.resources_confirmed != settled
            || (self.status == Status::Success && self.exit_code != Some(0))
            || (self.status == Status::Nonzero && self.exit_code == Some(0))
            || self.cleanup.len() > 8
            || self.cleanup.iter().any(|error| error.len() > 768)
            || self
                .primary
                .as_ref()
                .is_some_and(|primary| primary.len() > 128)
        {
            return Err("invalid stored command completion metadata".into());
        }
        for capture in [&self.stdout, &self.stderr].into_iter().flatten() {
            if capture.retained_bytes > 32 * 1024
                || capture.total_bytes < capture.retained_bytes
                || capture.truncated != (capture.total_bytes > capture.retained_bytes)
            {
                return Err("invalid stored command capture accounting".into());
            }
        }
        Ok(())
    }
}

pub(super) fn execute(
    parameters: &Parameters,
    cwd: &Path,
    remaining: Duration,
    owner: &Arc<ProbeResourceOwner>,
    cancellation: &Cancellation,
) -> Effect {
    execute_with(parameters, remaining, now_ms, |limit| {
        probe::run_cancellable(&parameters.command, cwd, limit, owner, cancellation)
    })
}

pub(super) fn execute_with(
    parameters: &Parameters,
    remaining: Duration,
    clock: impl Fn() -> u64,
    launch: impl FnOnce(Duration) -> Result<probe::Output, ProbeFailure>,
) -> Effect {
    let execution_id = uuid::Uuid::new_v4().to_string();
    crate::flow_trace::record(&execution_id, "blueprint.started", "", "", "command", None);
    let started_at = clock();
    let result = parameters
        .validate()
        .map_err(|_| ProbeFailure::new("command_parameters_invalid"))
        .and_then(|()| launch(Duration::from_millis(parameters.timeout_ms).min(remaining)));
    let metadata = match result {
        Ok(output) => ResultMetadata {
            status: if output.success {
                Status::Success
            } else {
                Status::Nonzero
            },
            exit_code: output.exit_code,
            resources_confirmed: true,
            stdout: Some(output.stdout),
            stderr: Some(output.stderr),
            started_at,
            finished_at: clock(),
            primary: None,
            cleanup: Vec::new(),
        },
        Err(failure) => ResultMetadata {
            status: if failure.is_unconfirmed() {
                Status::Unconfirmed
            } else if failure.primary == "probe_cancelled" {
                Status::Cancelled
            } else if failure.is_deadline() {
                Status::Deadline
            } else {
                Status::Error
            },
            exit_code: failure.exit_code,
            resources_confirmed: !failure.is_unconfirmed(),
            stdout: failure.stdout,
            stderr: failure.stderr,
            started_at,
            finished_at: clock(),
            primary: Some(failure.primary.into()),
            cleanup: failure.cleanup_errors,
        },
    };
    Effect {
        status: match metadata.status {
            Status::Success => HandlerStatus::Completed,
            Status::Unconfirmed => HandlerStatus::Unconfirmed,
            _ => HandlerStatus::Failed,
        },
        execution_id: Some(execution_id),
        command_result: Some(metadata),
        reason: None,
    }
}

fn now_ms() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}
