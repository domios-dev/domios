//! Daemon-orchestrated orchestrator refresh: a verified-ordered S1..S5 state
//! machine that snapshots (handoff), compacts, waits for settle, re-orients, and
//! verifies resumption — or raises a loud alert. The one forbidden outcome is
//! silence. Mirrors `pty_io::wake`: pure transitions in `machine`, an idempotent
//! per-tab guard, and a wake-confirmer-backed injection path.

mod driver;
mod machine;
mod signature;
mod template;

pub use driver::{RefreshOutcome, RefreshRequest, RefreshTrigger};
pub use machine::RefreshTuning;
