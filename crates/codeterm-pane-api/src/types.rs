//! Types for pane API requests and responses.

use serde::{Deserialize, Serialize};

/// Information about a pane on a peer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneInfo {
    pub pane_id: String,
    pub workspace_id: String,
    pub tab_id: String,
    pub title: Option<String>,
    pub agent_type: Option<String>,
}

/// Request to create a shell pane.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateShellPaneRequest {
    pub workspace_id: Option<String>,
    pub cwd: Option<String>,
    pub title: Option<String>,
}

/// Response from pane creation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePaneResponse {
    pub pane_id: String,
    pub workspace_id: String,
    pub tab_id: String,
}

/// Request to send text to a pane.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendTextRequest {
    pub text: Option<String>,
    #[serde(default = "default_true")]
    pub enter: bool,
    pub batch: Option<Vec<BatchCommand>>,
    #[serde(default)]
    pub raw: bool,
    pub verify: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchCommand {
    pub text: String,
    #[serde(default = "default_true")]
    pub enter: bool,
    pub delay_ms: Option<u64>,
}

fn default_true() -> bool { true }

/// Response from send text.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendTextResponse {
    pub ok: bool,
    #[serde(default)]
    pub verified: bool,
    #[serde(default)]
    pub echo_observed: bool,
    #[serde(default)]
    pub submit_observed: bool,
    #[serde(default)]
    pub elapsed_ms: Option<u64>,
}

/// Request to execute a command in a pane.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecRequest {
    pub command: String,
    pub timeout_ms: Option<u64>,
    pub strip_ansi: Option<bool>,
}

/// Response from exec.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecResponse {
    pub output: String,
    pub offset: usize,
    #[serde(default)]
    pub timeout: Option<bool>,
}

/// Pane output response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneOutputResponse {
    pub output: String,
    #[serde(default)]
    pub offset: Option<usize>,
    #[serde(default)]
    pub full: Option<bool>,
    #[serde(default)]
    pub strip_ansi: Option<bool>,
}

/// Request for pane resize.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResizePaneRequest {
    pub cols: u16,
    pub rows: u16,
}

/// SSE event data for pane output streaming.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneStreamEvent {
    pub output: String,
    pub offset: usize,
}