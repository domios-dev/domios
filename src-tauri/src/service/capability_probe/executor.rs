use codeterm_types::agent::capability_probe::{InitializeCapabilities, ProbeFailure};
use std::{
    path::{Path, PathBuf},
    sync::{mpsc, OnceLock},
    time::{Duration, Instant},
};
struct Job {
    binary: PathBuf,
    project: PathBuf,
    until: Instant,
    response: mpsc::SyncSender<Result<InitializeCapabilities, ProbeFailure>>,
}
fn executor() -> Result<&'static mpsc::SyncSender<Job>, ProbeFailure> {
    static EXECUTOR: OnceLock<Result<mpsc::SyncSender<Job>, ProbeFailure>> = OnceLock::new();
    EXECUTOR
        .get_or_init(|| {
            let (tx, rx) = mpsc::sync_channel::<Job>(64);
            std::thread::Builder::new()
                .name("initialize-probe-executor".into())
                .spawn(move || {
                    while let Ok(job) = rx.recv() {
                        let remaining = job.until.saturating_duration_since(Instant::now());
                        let result = if remaining.is_zero() {
                            Err(ProbeFailure::Deadline)
                        } else {
                            super::process::capture(&job.binary, &job.project, remaining)
                        };
                        let _ = job.response.send(result);
                    }
                })
                .map(|_| tx)
                .map_err(|_| ProbeFailure::Spawn)
        })
        .as_ref()
        .map_err(|error| *error)
}
pub(super) fn capture(
    binary: &Path,
    project: &Path,
    bound: Duration,
) -> Result<InitializeCapabilities, ProbeFailure> {
    let (tx, rx) = mpsc::sync_channel(1);
    let until = Instant::now() + bound;
    executor()?
        .try_send(Job {
            binary: binary.into(),
            project: project.into(),
            until,
            response: tx,
        })
        .map_err(|error| match error {
            mpsc::TrySendError::Full(_) => ProbeFailure::Busy,
            mpsc::TrySendError::Disconnected(_) => ProbeFailure::Spawn,
        })?;
    rx.recv_timeout(bound + Duration::from_secs(5))
        .unwrap_or(Err(ProbeFailure::Deadline))
}
