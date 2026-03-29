use std::path::PathBuf;
use std::time::Duration;

/// How to wait for agent readiness after PTY spawn.
#[derive(Debug, Clone)]
pub enum ReadinessStrategy {
    PollShellPrompt { timeout: Duration },
    FixedDelay { duration: Duration },
}

/// How to deliver the task to the agent.
#[derive(Debug, Clone)]
pub enum TaskDelivery {
    /// Task is embedded in the launch command (Claude's temp file approach).
    InlineFlag,
    /// Task is sent as PTY text after agent is ready.
    PtyInput,
}

#[derive(Debug, Clone)]
pub struct AgentLaunchParams {
    pub agent_name: Option<String>,
    pub task: Option<String>,
    pub args: Option<Vec<String>>,
}

/// Trait for agent backend providers (Claude, Codex, Aider, etc.)
pub trait AgentProvider: Send + Sync {
    /// Unique identifier for this provider (e.g. "claude").
    fn id(&self) -> &str;

    /// Human-readable name (e.g. "Claude Code").
    fn display_name(&self) -> &str;

    /// Build the shell command to launch this agent.
    fn build_launch_command(&self, params: &AgentLaunchParams) -> String;

    /// How to wait for the agent to be ready after PTY spawn.
    fn readiness_strategy(&self) -> ReadinessStrategy;

    /// How to deliver the task to the agent.
    fn task_delivery(&self) -> TaskDelivery;

    /// Detect this agent from the pane/tab title string.
    fn detect_from_title(&self, title: &str) -> bool;

    /// Detect this agent from PTY output text.
    fn detect_from_output(&self, text: &str) -> bool;

    /// Build the path to the session file for this agent.
    fn session_file_path(&self, cwd: &str, session_id: &str) -> Option<PathBuf>;

    /// Detect the most recent session ID for the given working directory.
    fn detect_session_id(&self, cwd: &str, exclude: &[String], max_age: Option<Duration>) -> Option<String>;

    /// Build a command to resume an existing session.
    fn build_resume_command(&self, session_id: &str, skip_permissions: bool) -> Option<String>;

    /// Whether this agent supports hook-based detection (vs PTY parsing fallback).
    /// Agents with hooks get detected via API endpoints, not PTY output parsing.
    fn has_hooks(&self) -> bool { false }

    /// Filter TUI noise from agent output. Returns true if the text is noise (should be dropped).
    /// Each provider knows its own status bar patterns, progress indicators, etc.
    fn is_output_noise(&self, text: &str) -> bool { let _ = text; false }



    /// Quick check: does this chunk contain markers worth investigating?
    /// Called on every PTY chunk (must be fast). If true, full detection runs on ring buffer.
    /// Default: check for numbered options and keyboard hints.
    fn has_event_markers(&self, chunk: &str) -> bool {
        chunk.contains("❯") || chunk.contains("proceed")
            || chunk.contains("Esc to cancel") || chunk.contains("Tab to amend")
            || (chunk.contains("1.") && chunk.contains("2."))
    }

    /// Detect agent events from terminal output (full ring buffer text).
    /// Default: generic structural fingerprint (numbered menu + keyboard hints).
    fn detect_events(&self, text: &str) -> Vec<crate::agent::detector::AgentEvent> {
        let mut events = Vec::new();
        if crate::agent::detector::detect_interactive_prompt(text) {
            events.push(crate::agent::detector::AgentEvent::PermissionRequest);
        }
        events
    }

    /// Parse a pending interactive prompt from terminal output.
    /// Default: generic numbered option parser.
    fn parse_prompt(&self, text: &str) -> Option<(String, Vec<(String, String)>)> {
        crate::agent::detector::parse_permission_prompt(text)
    }
}
