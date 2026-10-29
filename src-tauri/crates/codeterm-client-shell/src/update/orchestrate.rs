//! Update teardown ordering seam. Enforces the invariant that viability is
//! proven BEFORE any destructive PTY teardown, and that any failure once
//! teardown has begun leaves a diagnostic artifact + user-visible signal.

use std::future::Future;

use super::failure_report::{UpdateAbort, UpdateFailureSink};

/// Order: viability -> shutdown -> install.
///
/// - Defect 1: `viability` runs first; if it fails, `shutdown` is NEVER invoked
///   and the update aborts with panes still alive.
/// - Defect 4: once `shutdown` starts, teardown is underway, so a failure in
///   either `shutdown` or `install` routes through `sink` (artifact + signal).
///
/// `shutdown` returns the tab ids it tore down so the abort artifact can name
/// them.
pub async fn install_after_shutdown<P, V, VF, S, SF, I, IF>(
    verified_bytes: Vec<u8>,
    viability: V,
    shutdown: S,
    install: I,
    sink: &dyn UpdateFailureSink,
) -> Result<(), String>
where
    V: FnOnce() -> VF,
    VF: Future<Output = Result<P, String>>,
    S: FnOnce(P) -> SF,
    SF: Future<Output = Result<Vec<String>, String>>,
    I: FnOnce(Vec<u8>) -> IF,
    IF: Future<Output = Result<(), String>>,
{
    let prepared = match viability().await {
        Ok(prepared) => prepared,
        Err(cause) => {
            sink.abort(&UpdateAbort::new("preflight", &cause, Vec::new()));
            return Err(cause);
        }
    };

    let torn_down = match shutdown(prepared).await {
        Ok(tabs) => tabs,
        Err(cause) => {
            sink.abort(&UpdateAbort::new("shutdown", &cause, Vec::new()));
            return Err(cause);
        }
    };

    match install(verified_bytes).await {
        Ok(()) => Ok(()),
        Err(cause) => {
            sink.abort(&UpdateAbort::new("install", &cause, torn_down));
            Err(cause)
        }
    }
}

#[cfg(test)]
mod tests;
