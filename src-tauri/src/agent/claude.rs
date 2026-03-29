use std::path::PathBuf;
use std::time::Duration;

use crate::shell::ShellSyntax;

use super::provider::{AgentLaunchParams, AgentProvider, ReadinessStrategy, TaskDelivery};

/// Claude Code agent provider.
pub struct ClaudeProvider;

impl ClaudeProvider {
    pub fn new() -> Self {
        Self
    }

    /// Encode a CWD path the same way Claude CLI does for project directories.
    fn encode_cwd(cwd: &str) -> String {
        cwd.chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect()
    }

    /// Get the sessions directory for a given CWD.
    fn sessions_dir(cwd: &str) -> Option<PathBuf> {
        let home = dirs::home_dir()?;
        let encoded = Self::encode_cwd(cwd);
        Some(home.join(".claude").join("projects").join(encoded))
    }
}

impl AgentProvider for ClaudeProvider {
    fn id(&self) -> &str {
        "claude"
    }

    fn display_name(&self) -> &str {
        "Claude Code"
    }

    fn build_launch_command(&self, params: &AgentLaunchParams) -> String {
        let mut parts: Vec<String> = Vec::new();

        parts.push(ShellSyntax::UNSET_CLAUDECODE.to_string());
        parts.push("claude".to_string());
        parts.push("--dangerously-skip-permissions".to_string());

        if let Some(ref task) = params.task {
            parts.push(ShellSyntax::task_file_expr(task));
        }

        if let Some(ref name) = params.agent_name {
            parts.push("--agent".to_string());
            parts.push(ShellSyntax::quote(name));
        }

        if let Some(ref extra) = params.args {
            parts.extend(extra.iter().cloned());
        }

        parts.join(" ")
    }

    fn readiness_strategy(&self) -> ReadinessStrategy {
        ReadinessStrategy::PollShellPrompt {
            timeout: Duration::from_secs(30),
        }
    }

    fn task_delivery(&self) -> TaskDelivery {
        TaskDelivery::InlineFlag
    }

    fn detect_from_title(&self, title: &str) -> bool {
        title.to_lowercase().contains("claude")
    }

    fn detect_from_output(&self, text: &str) -> bool {
        text.contains("Do you want to proceed?")
            || text.contains("may I proceed")
            || text.contains("Would you like me to")
            || text.contains("Yes, and don't ask again")
            || text.contains("Enter to confirm")
            || text.contains("Cogitated for")
            || text.contains("Brewed for")
            || text.contains("Reasoned for")
    }

    fn session_file_path(&self, cwd: &str, session_id: &str) -> Option<PathBuf> {
        let dir = Self::sessions_dir(cwd)?;
        Some(dir.join(format!("{}.jsonl", session_id)))
    }

    fn detect_session_id(
        &self,
        cwd: &str,
        exclude: &[String],
        max_age: Option<Duration>,
    ) -> Option<String> {
        let sessions_dir = Self::sessions_dir(cwd)?;

        if !sessions_dir.exists() {
            return None;
        }

        let now = std::time::SystemTime::now();
        // 5 min default: Claude may be idle waiting for input while JSONL sits unmodified
        let recency_limit = max_age.unwrap_or_else(|| Duration::from_secs(300));
        let mut sessions: Vec<(String, std::time::SystemTime)> = Vec::new();

        if let Ok(entries) = std::fs::read_dir(&sessions_dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.ends_with(".jsonl") {
                    let stem = name.trim_end_matches(".jsonl");
                    if stem.len() >= 32
                        && stem.contains('-')
                        && !exclude.contains(&stem.to_string())
                    {
                        if let Ok(meta) = entry.metadata() {
                            if let Ok(modified) = meta.modified() {
                                if let Ok(age) = now.duration_since(modified) {
                                    if age > recency_limit {
                                        continue;
                                    }
                                }
                                sessions.push((stem.to_string(), modified));
                            }
                        }
                    }
                }
            }
        }

        sessions.sort_by(|a, b| b.1.cmp(&a.1));
        sessions.first().map(|(id, _)| id.clone())
    }

    fn build_resume_command(&self, session_id: &str, skip_permissions: bool) -> Option<String> {
        let mut parts = vec![
            ShellSyntax::UNSET_CLAUDECODE.to_string(),
            "claude".to_string(),
        ];
        if skip_permissions {
            parts.push("--dangerously-skip-permissions".to_string());
        }
        parts.push("--resume".to_string());
        parts.push(ShellSyntax::quote(session_id));
        Some(parts.join(" "))
    }

    fn has_event_markers(&self, chunk: &str) -> bool {
        chunk.contains("❯") || chunk.contains("proceed")
            || chunk.contains("Esc to cancel") || chunk.contains("Tab to amend")
            || (chunk.contains("1.") && chunk.contains("2."))
            || chunk.contains(" for 0") || chunk.contains(" for 1") || chunk.contains(" for 2")
            || chunk.contains(" for 3") || chunk.contains(" for 4") || chunk.contains(" for 5")
            || chunk.contains(" for 6") || chunk.contains(" for 7") || chunk.contains(" for 8")
            || chunk.contains(" for 9")
    }

    fn detect_events(&self, text: &str) -> Vec<crate::agent::detector::AgentEvent> {
        crate::agent::detector::detect_claude_events(text)
    }

    fn parse_prompt(&self, text: &str) -> Option<(String, Vec<(String, String)>)> {
        crate::agent::detector::parse_permission_prompt(text)
    }


    fn has_hooks(&self) -> bool { true }

    fn status_bar_rows(&self) -> u16 { 3 }

    fn is_output_noise(&self, text: &str) -> bool {
        let t = text.trim();
        if t.is_empty() { return true; }
        let noise = [
            "bypass permissions on",
            "shift+tab to cycle",
            "Update available! Run:",
            "brew upgrade claude-code",
            "esc to interrupt",
            "? for shortcuts",
            "ctrl+g to edit",
            "Monthly spend limit",
            "hold Space",
        ];
        noise.iter().any(|n| t.contains(n))
            || t.chars().all(|c| c == '─' || c == '━' || c == '═' || c.is_whitespace())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_claude_provider_id() {
        let p = ClaudeProvider::new();
        assert_eq!(p.id(), "claude");
        assert_eq!(p.display_name(), "Claude Code");
    }

    #[test]
    fn test_build_launch_no_task() {
        let p = ClaudeProvider::new();
        let params = AgentLaunchParams {
            agent_name: None,
            task: None,
            args: None,
        };
        let cmd = p.build_launch_command(&params);
        if cfg!(target_os = "windows") {
            assert_eq!(
                cmd,
                "$env:CLAUDECODE=$null; claude --dangerously-skip-permissions"
            );
        } else {
            assert_eq!(
                cmd,
                "unset CLAUDECODE && claude --dangerously-skip-permissions"
            );
        }
    }

    #[test]
    fn test_build_launch_with_task() {
        let p = ClaudeProvider::new();
        let params = AgentLaunchParams {
            agent_name: None,
            task: Some("count files".to_string()),
            args: None,
        };
        let cmd = p.build_launch_command(&params);
        assert!(cmd.contains("--dangerously-skip-permissions"));
        assert!(cmd.contains(".ct-task-"), "task via temp file");
    }

    #[test]
    fn test_build_launch_with_agent_name() {
        let p = ClaudeProvider::new();
        let params = AgentLaunchParams {
            agent_name: Some("architect".to_string()),
            task: None,
            args: None,
        };
        let cmd = p.build_launch_command(&params);
        assert!(cmd.contains("--agent"));
        assert!(cmd.contains("'architect'"));
    }

    #[test]
    fn test_detect_from_title() {
        let p = ClaudeProvider::new();
        assert!(p.detect_from_title("Claude Code"));
        assert!(p.detect_from_title("claude"));
        assert!(!p.detect_from_title("Aider"));
    }

    #[test]
    fn test_detect_from_output() {
        let p = ClaudeProvider::new();
        assert!(p.detect_from_output("Do you want to proceed?"));
        assert!(p.detect_from_output("Cogitated for 3s"));
        assert!(!p.detect_from_output("random text"));
    }

    #[test]
    fn test_session_file_path() {
        let p = ClaudeProvider::new();
        let path = p
            .session_file_path("/home/user/project", "abc-def-123-456")
            .unwrap();
        let path_str = path.to_string_lossy();
        assert!(path_str.contains("abc-def-123-456.jsonl"));
        // encode_cwd replaces non-alphanumeric chars with '-'
        assert!(path_str.contains(".claude"));
        assert!(path_str.contains("projects"));
        assert!(path_str.contains("-home-user-project"));
    }

    #[test]
    fn test_build_resume_command() {
        let p = ClaudeProvider::new();
        let cmd = p
            .build_resume_command("abc-def-123-456", true)
            .unwrap();
        assert!(cmd.contains("--resume"));
        assert!(cmd.contains("abc-def-123-456"));
        assert!(cmd.contains("--dangerously-skip-permissions"));
    }

    #[test]
    fn test_readiness_strategy() {
        let p = ClaudeProvider::new();
        match p.readiness_strategy() {
            ReadinessStrategy::PollShellPrompt { timeout } => {
                assert_eq!(timeout, Duration::from_secs(30));
            }
            _ => panic!("Expected PollShellPrompt"),
        }
    }

    #[test]
    fn test_encode_cwd() {
        assert_eq!(ClaudeProvider::encode_cwd("/home/user/project"), "-home-user-project");
        assert_eq!(ClaudeProvider::encode_cwd("C:\\Users\\test"), "C--Users-test");
    }
}
