//! General Agent — always-on system agent lifecycle management.
//!
//! Handles: directory creation, instructions file generation,
//! workspace/pane setup, startup, resume, recovery.

use std::path::PathBuf;

/// Get the General Agent workspace directory.
/// `~/.codeterm/general/` (or `~/.codeterm-dev/general/` for dev builds).
pub fn general_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(crate::paths::DIR_NAME).join("general")
}

/// Get the secrets directory.
pub fn secrets_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(crate::paths::DIR_NAME).join("secrets")
}

/// Get the integrations config directory.
pub fn integrations_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(crate::paths::DIR_NAME).join("integrations")
}

/// Ensure all General Agent directories exist.
pub fn ensure_directories() {
    let dirs = [general_dir(), secrets_dir(), integrations_dir(), general_dir().join("state")];
    for dir in &dirs {
        if !dir.exists() {
            if let Err(e) = std::fs::create_dir_all(dir) {
                log::error!("[general-agent] Failed to create directory {:?}: {}", dir, e);
            }
        }
    }
}

/// Generate or update the provider instructions file (e.g., CLAUDE.md).
/// Uses marker-based sections — auto-generated content between markers is replaced,
/// user content outside markers is preserved.
pub fn generate_instructions(provider_name: &str, instructions_filename: &str) {
    let path = general_dir().join(instructions_filename);
    let version = env!("CARGO_PKG_VERSION");
    let platform = if cfg!(target_os = "windows") { "Windows" }
        else if cfg!(target_os = "macos") { "macOS" }
        else { "Linux" };
    let hostname = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    let auto_section = format!(
r#"<!-- CODETERM-AUTO-START v{version} -->
# CodeTerm General Agent

You are the General Agent — the always-on system brain of this CodeTerm installation.
You run inside CodeTerm, an AI-first terminal multiplexer.

## Your role
- Manage this machine's CodeTerm environment
- Respond to user requests via terminal or connected services (Telegram, Discord, etc.)
- Spawn and coordinate agents in project workspaces
- Manage cron jobs, notifications, system health
- Self-recover if you crash — resume your session or start fresh

## Available tools
Use the `codeterm` CLI (available in your PATH):
- `codeterm workspace create "name"` — create a workspace
- `codeterm pane list` — list all panes
- `codeterm pane screen --pane ID` — see another pane's screen
- `codeterm send-text --pane ID "text"` — send text to another pane
- `codeterm agent spawn --title "Worker" --task "do X"` — spawn a worker
- `codeterm cron create "name" --schedule "*/5 * * * *" --task "do Y"` — schedule tasks
- `codeterm notify "title" --body "message"` — send notification

## Your constraints
- You CANNOT close your own tab or workspace (protected)
- You CAN spawn agents in other workspaces
- You always run with skip-permissions
- If you need to restart, use /exit — CodeTerm will auto-resume you
- Your CWD is {general_dir}

## Secrets
Read credentials from {secrets_dir}/*.md when needed.
You can create and update secret files yourself.

## Machine info
- Platform: {platform}
- Machine: {hostname}
- CodeTerm: v{version}
- Provider: {provider_name}
<!-- CODETERM-AUTO-END -->"#,
        version = version,
        platform = platform,
        hostname = hostname,
        provider_name = provider_name,
        general_dir = general_dir().display(),
        secrets_dir = secrets_dir().display(),
    );

    // Read existing file to preserve user content outside markers
    let existing = std::fs::read_to_string(&path).unwrap_or_default();

    let new_content = if existing.contains("<!-- CODETERM-AUTO-START") && existing.contains("<!-- CODETERM-AUTO-END -->") {
        // Replace content between markers
        let start_marker = "<!-- CODETERM-AUTO-START";
        let end_marker = "<!-- CODETERM-AUTO-END -->";
        let start = existing.find(start_marker).unwrap();
        let end = existing.find(end_marker).unwrap() + end_marker.len();
        format!("{}{}{}", &existing[..start], auto_section, &existing[end..])
    } else if existing.is_empty() {
        // Fresh file — add auto section + pointer to extra instructions
        format!("{}\n\n# Extra Instructions\nSee EXTRA_INSTRUCTIONS.md for user-defined instructions.\n", auto_section)
    } else {
        // File exists but no markers — prepend auto section, keep everything
        format!("{}\n\n{}", auto_section, existing)
    };

    if let Err(e) = std::fs::write(&path, new_content) {
        log::error!("[general-agent] Failed to write instructions file {:?}: {}", path, e);
    } else {
        log::info!("[general-agent] Instructions file updated: {:?}", path);
    }

    // Ensure EXTRA_INSTRUCTIONS.md exists
    let extra = general_dir().join("EXTRA_INSTRUCTIONS.md");
    if !extra.exists() {
        let _ = std::fs::write(&extra, "# Extra Instructions\n\nAdd your custom instructions for the General Agent here.\nThis file is never overwritten by CodeTerm.\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_general_dir_under_home() {
        let dir = general_dir();
        let home = dirs::home_dir().unwrap();
        assert!(dir.starts_with(home));
        assert!(dir.ends_with("general"));
    }

    #[test]
    fn test_secrets_dir_under_home() {
        let dir = secrets_dir();
        let home = dirs::home_dir().unwrap();
        assert!(dir.starts_with(home));
        assert!(dir.ends_with("secrets"));
    }

    #[test]
    fn test_generate_instructions_fresh() {
        let tmp = std::env::temp_dir().join("codeterm-test-general");
        let _ = std::fs::create_dir_all(&tmp);
        let path = tmp.join("CLAUDE.md");
        let _ = std::fs::remove_file(&path);

        // Can't easily test generate_instructions directly (uses general_dir()),
        // but we can test the marker logic
        let auto_start = "<!-- CODETERM-AUTO-START v0.1.0 -->\nAuto content\n<!-- CODETERM-AUTO-END -->";
        let full = format!("{}\n\nUser content here", auto_start);

        // Simulate update: replace auto section
        let new_auto = "<!-- CODETERM-AUTO-START v0.2.0 -->\nNew auto content\n<!-- CODETERM-AUTO-END -->";
        let start_marker = "<!-- CODETERM-AUTO-START";
        let end_marker = "<!-- CODETERM-AUTO-END -->";
        let start = full.find(start_marker).unwrap();
        let end = full.find(end_marker).unwrap() + end_marker.len();
        let result = format!("{}{}{}", &full[..start], new_auto, &full[end..]);

        assert!(result.contains("v0.2.0"));
        assert!(result.contains("User content here"));
        assert!(!result.contains("v0.1.0"));

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
