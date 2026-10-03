use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

#[cfg(unix)]
mod unix;
#[cfg(windows)]
pub(super) mod windows;

mod capture;
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

#[derive(Debug)]
pub struct Output {
    pub bytes: Vec<u8>,
    pub text: String,
}

pub(super) fn run(
    command: &str,
    cwd: &Path,
    limit: Duration,
    owner: &Arc<owner::ProbeResourceOwner>,
) -> Result<Output, process::ProbeFailure> {
    let deadlines = process::Deadlines::start(limit, PROBE_RUNTIME)?;
    run_transaction(command, cwd, deadlines, owner)
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
    run_transaction(command, cwd, deadlines, owner)
        .map(|output| (output, receipt))
        .map_err(|failure| (failure, Some(receipt)))
}

fn run_transaction(
    command: &str,
    cwd: &Path,
    deadlines: process::Deadlines,
    owner: &Arc<owner::ProbeResourceOwner>,
) -> Result<Output, process::ProbeFailure> {
    admit_then(owner, |reservation| {
        #[cfg(unix)]
        {
            unix::run(command, cwd, deadlines, reservation, owner)
        }
        #[cfg(windows)]
        {
            windows::run(command, cwd, deadlines, reservation, owner)
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
