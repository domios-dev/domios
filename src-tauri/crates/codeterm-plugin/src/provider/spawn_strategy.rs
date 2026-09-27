//! How a provider is spawned: when it counts as ready, how a submission is
//! confirmed, how the task is delivered, and the parameters a launch carries.
//! Detecting the session that results is `launch_session`.
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::StructuredPromptServer;

/// How to wait for agent readiness after PTY spawn.
#[derive(Debug, Clone)]
pub enum ReadinessStrategy {
    PollShellPrompt { timeout: Duration },
    FixedDelay { duration: Duration },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderReadinessStrategy {
    TuiFrame { timeout: Duration },
    StructuralComposer { timeout: Duration },
}

impl Default for ProviderReadinessStrategy {
    fn default() -> Self {
        Self::TuiFrame {
            timeout: Duration::from_secs(30),
        }
    }
}

impl ProviderReadinessStrategy {
    pub fn timeout(&self) -> Duration {
        match self {
            Self::TuiFrame { timeout } | Self::StructuralComposer { timeout } => *timeout,
        }
    }

    pub fn requires_composer(&self) -> bool {
        matches!(self, Self::StructuralComposer { .. })
    }
}

/// How to confirm that text submitted to a launched provider was actually
/// processed (Enter consumed, REPL accepted it). Used to gate `OK` on a
/// real receiver-side observation rather than an optimistic write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifySubmitStrategy {
    /// Wait for a new shell prompt to appear (default for plain shells and
    /// providers that pipe through the shell unchanged).
    ShellPrompt { timeout: Duration },
    /// Submit through an interactive TUI with bounded provider-declared timing.
    TuiInputDrained {
        initial_key_delay: Duration,
        retry_offsets: [Duration; 6],
    },
    /// No verification — for providers where post-submit state is not
    /// observable from the PTY (session-id-injection, headless flag delivery, etc.).
    None,
}

impl VerifySubmitStrategy {
    pub fn initial_key_delay(&self) -> Duration {
        match self {
            Self::TuiInputDrained {
                initial_key_delay, ..
            } => *initial_key_delay,
            Self::ShellPrompt { .. } | Self::None => Duration::ZERO,
        }
    }

    pub fn retry_offsets(&self) -> [Duration; 6] {
        match self {
            Self::TuiInputDrained { retry_offsets, .. } => *retry_offsets,
            Self::ShellPrompt { .. } | Self::None => DEFAULT_SUBMIT_RETRY_OFFSETS,
        }
    }
}

pub const DEFAULT_SUBMIT_RETRY_OFFSETS: [Duration; 6] = [
    Duration::from_millis(200),
    Duration::from_millis(500),
    Duration::from_millis(900),
    Duration::from_millis(1500),
    Duration::from_millis(2200),
    Duration::from_millis(3000),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BusySubmitStrategy {
    SubmitNormally,
    Key(String),
}

/// How to deliver the task to the agent.
#[derive(Debug, Clone)]
pub enum TaskDelivery {
    /// Task is embedded in the launch command (Claude's temp file approach).
    InlineFlag,
    /// Task is sent as PTY text after agent is ready.
    PtyInput,
}

/// The single visible turn that starts an interactive provider session.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StarterPrompt {
    #[default]
    AssignedTask,
    NoStarter,
    IdleWakeup,
    TeamBootstrap,
    StartupHandshake,
}

impl StarterPrompt {
    pub fn host_text(self) -> Option<&'static str> {
        match self {
            Self::TeamBootstrap => Some(crate::prompts::TEAM_BOOTSTRAP_STARTER.trim()),
            Self::StartupHandshake => Some(
                "Before any other action, run `codeterm agent startup-ack`. Then continue with the assigned role or task.",
            ),
            Self::NoStarter | Self::AssignedTask | Self::IdleWakeup => None,
        }
    }

    pub fn system_prefix(self) -> Option<String> {
        let context = match self {
            Self::AssignedTask => "assigned-task",
            Self::IdleWakeup => "idle-wakeup",
            Self::TeamBootstrap => "team-bootstrap",
            Self::StartupHandshake => "startup-handshake",
            Self::NoStarter => return None,
        };
        Some(
            codeterm_types::message_markers::Marker::new("system")
                .attr("context", context)
                .build(),
        )
    }
}

#[derive(Debug, Clone, Default)]
pub struct AgentLaunchParams {
    pub agent_name: Option<String>,
    pub task: Option<String>,
    pub starter_prompt: StarterPrompt,
    pub args: Option<Vec<String>>,
    pub skip_permissions: bool,
    // ignore it (they generate ids themselves and CodeTerm has to detect after launch).
    pub session_id: Option<String>,
    /// Fixed local server the host wants the agent's structured-prompt handle to
    /// listen on (opencode `--hostname/--port`). Set by the host only for
    /// providers that `supports_structured_prompts`; the plugin maps it to the
    /// provider's server flags. Fixes the W3 leading-dash `--args` ergonomics
    /// gap — the host allocates the port instead of the caller hand-passing args.
    pub structured_prompt_server: Option<StructuredPromptServer>,
    pub launch_marker: Option<String>,
    pub instance_id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchArtifactSweepRequest {
    pub instance_id: String,
    pub live_tab_ids: Vec<String>,
    #[serde(default)]
    pub confirm: bool,
    #[serde(default)]
    pub include_legacy: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchArtifactSweepReport {
    pub dead: Vec<String>,
    pub legacy: Vec<String>,
    pub removed: Vec<String>,
}

/// Everything a resumed session needs beyond its id. `system_prompt` is the
/// composed launch prompt, present only when the host has one to re-apply.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentResumeParams {
    pub session_id: String,
    pub skip_permissions: bool,
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
}

/// Splice trailing launch args onto a command the same way
/// `build_launch_command` splices `AgentLaunchParams::args`.
pub fn append_launch_args(command: String, args: &[String]) -> String {
    if args.is_empty() {
        return command;
    }
    format!("{command} {}", args.join(" "))
}
