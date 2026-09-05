//! The readable record survives the dump write, including when the dump write dies.
//!
//! A crash on 2026-09-05 left `crash-<ts>-native.dmp` at zero bytes and no `.txt`
//! at all: the handler entered `write_minidump` and never came back, so the code
//! that reported the outcome never ran. The record is therefore written BEFORE the
//! dump is attempted and rewritten after, and a surviving `Attempted` record is
//! itself the evidence that the writer died mid-write.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;

pub const ATTEMPTED: &str = "attempted, outcome unknown";
pub const WRITTEN: &str = "written";
pub const FAILED: &str = "failed";

pub const REASON_RETURNED_FALSE: &str = "write_minidump returned false";
pub const REASON_PANICKED: &str = "write_minidump panicked";
pub const REASON_EMPTY: &str = "write_minidump reported success but the dump is 0 bytes";
pub const REASON_MISSING: &str = "write_minidump reported success but the dump is not on disk";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DumpOutcome {
    /// The record was written before the dump was attempted. Reaching a reader in
    /// this state means the process died inside the dump write.
    Attempted,
    Written {
        bytes: u64,
    },
    Failed(&'static str),
}

impl DumpOutcome {
    pub fn render(&self, path: &Path) -> String {
        match self {
            Self::Attempted => format!("Minidump: {} ({})", path.display(), ATTEMPTED),
            Self::Written { bytes } => {
                format!(
                    "Minidump: {} ({}, {} bytes)",
                    path.display(),
                    WRITTEN,
                    bytes
                )
            }
            Self::Failed(reason) => {
                format!("Minidump: {} ({}: {})", path.display(), FAILED, reason)
            }
        }
    }
}

/// A dump the writer called successful is only believed once the file backs it up.
pub fn classify(reported_ok: bool, dmp_path: &Path) -> DumpOutcome {
    if !reported_ok {
        return DumpOutcome::Failed(REASON_RETURNED_FALSE);
    }
    match std::fs::metadata(dmp_path) {
        Err(_) => DumpOutcome::Failed(REASON_MISSING),
        Ok(meta) if meta.len() == 0 => DumpOutcome::Failed(REASON_EMPTY),
        Ok(meta) => DumpOutcome::Written { bytes: meta.len() },
    }
}

/// Write the record, attempt the dump, rewrite the record with what happened.
///
/// `compose` renders the full sidecar for a given outcome; it is called twice, so
/// it must not consume anything. A panic out of `write` is contained and recorded;
/// a hard fault inside it leaves the `Attempted` record already on disk.
pub fn record_guarded_dump(
    dmp_path: &Path,
    txt_path: &Path,
    compose: impl Fn(&DumpOutcome) -> String,
    write: impl FnOnce() -> bool,
) -> DumpOutcome {
    let _ = std::fs::write(txt_path, compose(&DumpOutcome::Attempted));

    let outcome = match catch_unwind(AssertUnwindSafe(write)) {
        Ok(reported_ok) => classify(reported_ok, dmp_path),
        Err(_) => DumpOutcome::Failed(REASON_PANICKED),
    };

    let _ = std::fs::write(txt_path, compose(&outcome));
    outcome
}

#[cfg(test)]
mod tests;
