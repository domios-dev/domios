use serde::{Deserialize, Serialize};

/// One stage of a tab's agent-boot lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BootStage {
    /// Tab exists and is waiting for PTY materialization.
    PtyStarting,
    /// PTY exists and its stream is claimed.
    PtySpawned,
    /// The backend reader owns the PTY stream and can observe launch output.
    ReaderReady,
    /// Shell prompt is ready to accept the launch command.
    ShellReady,
    /// Launch command sent; the agent TUI is coming up.
    AgentLaunching,
    /// The launch command write and submit verification completed.
    LaunchWritten,
    /// The provider rendered its structural TUI frame.
    ProviderReady,
    /// Role-specific instructions are being composed for the launch.
    PreparingRoleInstructions,
    /// The initial instructions are being submitted through the fallback channel.
    DeliveringInstructions,
    /// Agent booted and the initial task (if any) was delivered.
    AgentReady,
    /// A restored tab proved its PTY is live and re-attached without a spawn.
    PtyReattached,
    /// The launch-output deadline elapsed with no provider evidence yet.
    /// Provisional, not a verdict: only authoritative late readiness from the
    /// same PTY and launch attempt retires it. Carries the deadline reason.
    LaunchDeadlineElapsed,
    /// Boot aborted; carries a human-readable reason on the event payload.
    SpawnFailed,
}

/// The PTY and launch attempt a deadline-sourced provisional stage describes.
/// Retirement compares late readiness against both, so a prior PTY or a
/// superseded attempt can never speak for the launch that is latched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeadlineLatchedLaunch {
    pub pty_id: String,
    pub binding_nonce: String,
    pub reason: String,
}

impl BootStage {
    /// Canonical happy-path order.
    pub const HAPPY_PATH: [BootStage; 7] = [
        BootStage::PtySpawned,
        BootStage::ReaderReady,
        BootStage::ShellReady,
        BootStage::AgentLaunching,
        BootStage::LaunchWritten,
        BootStage::ProviderReady,
        BootStage::AgentReady,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            BootStage::PtyStarting => "pty_starting",
            BootStage::PtySpawned => "pty_spawned",
            BootStage::ReaderReady => "reader_ready",
            BootStage::ShellReady => "shell_ready",
            BootStage::AgentLaunching => "agent_launching",
            BootStage::LaunchWritten => "launch_written",
            BootStage::ProviderReady => "provider_ready",
            BootStage::PreparingRoleInstructions => "preparing_role_instructions",
            BootStage::DeliveringInstructions => "delivering_instructions",
            BootStage::AgentReady => "agent_ready",
            BootStage::PtyReattached => "pty_reattached",
            BootStage::LaunchDeadlineElapsed => "launch_deadline_elapsed",
            BootStage::SpawnFailed => "spawn_failed",
        }
    }

    /// Stages whose payload reason is the stage's own evidence and must survive
    /// on the tab record for diagnostics.
    pub fn carries_reason(&self) -> bool {
        matches!(
            self,
            BootStage::SpawnFailed | BootStage::LaunchDeadlineElapsed
        )
    }

    /// A deadline is one observer giving up on a launch, never proof it failed.
    pub fn is_provisional(&self) -> bool {
        matches!(self, BootStage::LaunchDeadlineElapsed)
    }
}

/// Where a boot-stage transition goes.
pub trait BootStageSink {
    fn record(&self, tab_id: &str, stage: BootStage, reason: Option<&str>);
}
