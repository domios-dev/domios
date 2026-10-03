use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

#[cfg(unix)]
mod unix;
#[cfg(windows)]
pub(super) mod windows;

pub(super) mod cancellation;
pub(super) mod capture;
pub(super) mod owner;
pub(super) mod process;

#[cfg(test)]
mod native_tests;
#[cfg(test)]
mod tests;

#[cfg(windows)]
pub const PROBE_RUNTIME: Duration = Duration::from_secs(10);
#[cfg(not(windows))]
pub const PROBE_RUNTIME: Duration = Duration::from_secs(3);
pub const MAX_OUTPUT_BYTES: usize = 65_536;

#[derive(Debug, Default)]
pub struct Output {
    pub bytes: Vec<u8>,
    pub text: String,
    pub exit_code: Option<i32>,
    pub success: bool,
    pub stdout: super::command::Capture,
    pub stderr: super::command::Capture,
}

pub(super) fn run(
    command: &str,
    cwd: &Path,
    limit: Duration,
    owner: &Arc<owner::ProbeResourceOwner>,
) -> Result<Output, process::ProbeFailure> {
    let deadlines = process::Deadlines::start(limit, PROBE_RUNTIME)?;
    run_transaction(
        command,
        cwd,
        deadlines,
        owner,
        &cancellation::Cancellation::default(),
        false,
    )
}

#[cfg(test)]
pub(super) fn run_with_receipt(
    command: &str,
    cwd: &Path,
    limit: Duration,
    owner: &Arc<owner::ProbeResourceOwner>,
) -> Result<
    (Output, process::DeadlineReceipt),
    (process::ProbeFailure, Option<process::DeadlineReceipt>),
> {
    let admitted = std::time::Instant::now();
    let deadlines = process::Deadlines::at(limit, PROBE_RUNTIME, admitted)
        .map_err(|failure| (failure, None))?;
    let receipt = deadlines.receipt();
    run_transaction(
        command,
        cwd,
        deadlines,
        owner,
        &cancellation::Cancellation::default(),
        false,
    )
    .map(|output| (output, receipt))
    .map_err(|failure| (failure, Some(receipt)))
}

pub(super) fn run_cancellable(
    command: &str,
    cwd: &Path,
    limit: Duration,
    owner: &Arc<owner::ProbeResourceOwner>,
    cancellation: &cancellation::Cancellation,
) -> Result<Output, process::ProbeFailure> {
    let deadlines = process::Deadlines::start(limit, PROBE_RUNTIME)?;
    run_transaction(command, cwd, deadlines, owner, cancellation, true)
}

fn run_transaction(
    command: &str,
    cwd: &Path,
    deadlines: process::Deadlines,
    owner: &Arc<owner::ProbeResourceOwner>,
    cancellation: &cancellation::Cancellation,
    _cancellation_required: bool,
) -> Result<Output, process::ProbeFailure> {
    if cancellation.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(process::ProbeFailure::new("probe_cancelled"));
    }
    admit_then(owner, |mut reservation| {
        reservation.register_cancellation(cancellation.clone())?;
        #[cfg(unix)]
        {
            unix::run(
                command,
                cwd,
                deadlines,
                reservation,
                owner,
                cancellation,
                _cancellation_required,
            )
        }
        #[cfg(windows)]
        {
            windows::run(command, cwd, deadlines, reservation, owner, cancellation)
        }
    })
}

fn admit_then<T>(
    owner: &Arc<owner::ProbeResourceOwner>,
    start: impl FnOnce(owner::Reservation) -> Result<T, process::ProbeFailure>,
) -> Result<T, process::ProbeFailure> {
    let reservation = owner.reserve().map_err(|error| match error {
        owner::AdmissionError::Full => process::ProbeFailure::new("probe_owner_capacity"),
        owner::AdmissionError::Closed => process::ProbeFailure::new("probe_owner_closing"),
        owner::AdmissionError::Unavailable(detail) => {
            let mut failure = process::ProbeFailure::new("probe_owner_unavailable");
            failure.cleanup_error(detail);
            failure
        }
    })?;
    start(reservation)
}

#[cfg(test)]
pub(super) fn fixture_output(code: i32, stdout: &[u8], stderr: &[u8]) -> Output {
    #[cfg(unix)]
    let status = {
        use std::os::unix::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(code << 8)
    };
    #[cfg(windows)]
    let status = {
        use std::os::windows::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(code as u32)
    };
    let mut out = capture::CaptureData::default();
    let mut err = capture::CaptureData::default();
    out.append(stdout);
    err.append(stderr);
    out.mark_eof();
    err.mark_eof();
    capture::assemble_confirmed(
        status,
        out,
        err,
        capture::CompletionFacts {
            parent_complete: true,
            boundary_complete: true,
            stdout_eof: true,
            stderr_eof: true,
            error: None,
        },
    )
    .unwrap()
}
