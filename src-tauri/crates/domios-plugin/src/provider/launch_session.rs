use serde::{Deserialize, Serialize};

/// One process in the tab's own PTY process tree. `start_token` is the OS
/// process start time in its native encoding, the guard against PID reuse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchProcess {
    pub pid: u32,
    pub depth: u32,
    pub start_token: String,
}

/// Everything core knows about one launch, and nothing about any other.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchSessionEvidence {
    pub cwd: String,
    /// Inert token CodeTerm wrote into this launch's own provider-visible text.
    pub launch_marker: Option<String>,
    pub launched_at_ms: u64,
    pub processes: Vec<LaunchProcess>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchSessionSource {
    PidRegistry,
    LaunchMarker,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchSessionDetection {
    pub session_id: String,
    pub source: LaunchSessionSource,
}
