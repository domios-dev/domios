use serde::{Deserialize, Serialize};

/// One stage of a pane's agent-boot lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BootStage {
    /// Pane exists and is waiting for PTY materialization.
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
    /// A restored pane proved its PTY is live and re-attached without a spawn.
    PtyReattached,
    /// Boot aborted; carries a human-readable reason on the event payload.
    SpawnFailed,
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
            BootStage::SpawnFailed => "spawn_failed",
        }
    }
}

/// Where a boot-stage transition goes.
pub trait BootStageSink {
    fn record(&self, pane_id: &str, stage: BootStage, reason: Option<&str>);
}
