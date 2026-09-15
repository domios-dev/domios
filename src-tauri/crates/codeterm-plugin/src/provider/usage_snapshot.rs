use serde::Serialize;

/// Cumulative token usage for one agent session.
#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct UsageSnapshot {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub cache_read_input_tokens: u64,
    pub message_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_started_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_activity_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Normalised platform tag used by `install_command(platform)`.
pub fn current_platform_tag() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macos",
        "linux" => "linux",
        "windows" => "windows",
        other => other,
    }
}
