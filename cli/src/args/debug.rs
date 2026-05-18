use clap::{Subcommand, ValueEnum};

#[derive(Subcommand)]
pub enum DebugAction {
    /// Trigger a synthetic crash to verify the crash-reporter writes files.
    /// Daemon dies (segv/abort) or just records a panic report (panic).
    /// Only available in dev builds.
    Crash {
        /// Kind of crash to trigger
        #[arg(long, value_enum)]
        kind: CrashKind,
    },
}

#[derive(Copy, Clone, ValueEnum)]
pub enum CrashKind {
    /// Rust panic — hook writes text report, daemon survives
    Panic,
    /// SIGSEGV (null deref) — crash-handler catches, daemon dies
    Segv,
    /// std::process::abort → SIGABRT — daemon dies
    Abort,
}

impl CrashKind {
    pub fn as_str(self) -> &'static str {
        match self {
            CrashKind::Panic => "panic",
            CrashKind::Segv => "segv",
            CrashKind::Abort => "abort",
        }
    }
}
