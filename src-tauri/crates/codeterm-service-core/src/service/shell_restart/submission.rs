use std::cell::Cell;

use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum SubmissionStatus {
    Submitted,
    SubmitUnconfirmed,
}

#[derive(Debug, Serialize)]
pub struct ShellCommandSubmission {
    status: SubmissionStatus,
    pub pty_id: String,
    pub bytes_written: bool,
    pub bytes_queued: usize,
    pub evidence: &'static str,
    pub detail: Option<String>,
}

impl ShellCommandSubmission {
    pub fn submitted(&self) -> bool {
        matches!(self.status, SubmissionStatus::Submitted)
    }

    pub(crate) fn attempt(
        pty_id: &str,
        write: impl FnOnce(&Cell<usize>) -> Result<(), String>,
    ) -> Result<Self, String> {
        let bytes_queued = Cell::new(0);
        let result = write(&bytes_queued);
        if bytes_queued.get() == 0 {
            return Err(format!(
                "shell_command_not_submitted: {}",
                result.err().unwrap_or_else(|| "no bytes queued".into())
            ));
        }
        Ok(Self {
            status: if result.is_ok() {
                SubmissionStatus::Submitted
            } else {
                SubmissionStatus::SubmitUnconfirmed
            },
            pty_id: pty_id.into(),
            bytes_written: true,
            bytes_queued: bytes_queued.get(),
            evidence: "pty_write_queue",
            detail: result.err(),
        })
    }
}

#[cfg(test)]
mod tests;
