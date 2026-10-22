use std::ffi::OsString;
use std::io::{Seek, Write};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use super::{cancellation::Cancellation, environment::Environment, owner::ProbeResourceOwner};
use super::{
    process::{Deadlines, ProbeFailure},
    Output,
};

pub fn run(
    program: &Path,
    args: &[OsString],
    cwd: &Path,
    timeout: Duration,
    owner: &Arc<ProbeResourceOwner>,
    cancellation: &Cancellation,
    environment: &Environment,
    stdin: &[u8],
) -> Result<Output, ProbeFailure> {
    run_limited(
        program,
        args,
        cwd,
        timeout,
        owner,
        cancellation,
        environment,
        stdin,
        None,
        32768,
    )
}

pub fn run_limited(
    program: &Path,
    args: &[OsString],
    cwd: &Path,
    timeout: Duration,
    owner: &Arc<ProbeResourceOwner>,
    cancellation: &Cancellation,
    environment: &Environment,
    stdin: &[u8],
    memory_bytes: Option<u64>,
    output_bytes: u64,
) -> Result<Output, ProbeFailure> {
    let deadlines = Deadlines::start(timeout, timeout)?;
    if stdin.len() > 32768 {
        return Err(ProbeFailure::new("contract_violation"));
    }
    let mut file = tempfile::tempfile().map_err(|_| ProbeFailure::new("probe_input_failed"))?;
    file.write_all(stdin)
        .and_then(|_| file.rewind())
        .map_err(|_| ProbeFailure::new("probe_input_failed"))?;
    super::admit_then(owner, |mut reservation| {
        reservation.register_cancellation(cancellation.clone())?;
        super::unix::run(
            "",
            cwd,
            deadlines,
            reservation,
            owner,
            cancellation,
            true,
            environment,
            Some(file),
            Some(super::unix::Executable {
                program,
                args,
                memory_bytes,
                output_bytes: Some(output_bytes.min(32768)),
            }),
        )
    })
}
