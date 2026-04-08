use std::path::PathBuf;
use std::time::Duration;

use crate::agent::provider::{
    ActionStyle, AgentLaunchParams, AgentProvider, BadgeStyle, BadgeVariant, ModelInfo,
    NoteLevel, PathOpenKind, ReadinessStrategy, SettingField, TaskDelivery,
};
use crate::shell::ShellSyntax;

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
        if params.skip_permissions {
            parts.push("--dangerously-skip-permissions".to_string());
        }

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
        let lower = title.to_lowercase();
        if lower.contains("claude") {
            return true;
        }
        // Claude Code v2.1+ uses Braille spinner chars or ✳ as title prefix
        // e.g. "⠐ General coding session", "✳ Claude Code"
        let first = title.chars().next().unwrap_or('\0');
        (('\u{2801}'..='\u{28ff}').contains(&first) || first == '✳' || first == '✻')
            && title.len() > 2
    }

    fn detect_from_output(&self, text: &str) -> bool {
        text.contains("Claude Code")
            || text.contains("Do you want to proceed?")
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
                            // Recency check uses modification time — a resumed session
                            // has an old birth time but recent writes.
                            if let Ok(modified) = meta.modified() {
                                if let Ok(age) = now.duration_since(modified) {
                                    if age > recency_limit {
                                        continue;
                                    }
                                }
                            }
                            // Sort key uses creation time (birth) when available —
                            // a new session creates a new file, so the newest-created
                            // file is the current session. Falls back to modified time
                            // (Linux without birth time, or resumed sessions).
                            let sort_key = meta.created().or_else(|_| meta.modified());
                            if let Ok(ts) = sort_key {
                                sessions.push((stem.to_string(), ts));
                            }
                        }
                    }
                }
            }
        }

        // Sort by newest first — the most recently CREATED file = current session
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
        detect_claude_events(text)
    }

    fn parse_prompt(&self, text: &str) -> Option<(String, Vec<(String, String)>)> {
        crate::agent::detector::parse_permission_prompt(text)
    }


    fn has_hooks(&self) -> bool { true }

    /// Claude TUI supports vim j/k navigation — simpler and more reliable than arrow keys.
    fn build_prompt_response(&self, option_index: usize) -> Vec<Vec<u8>> {
        let mut steps = Vec::new();
        for _ in 0..option_index {
            steps.push(b"j".to_vec());
        }
        steps.push(b"\r".to_vec());
        steps
    }

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

    fn auto_approve_flags(&self) -> Vec<String> {
        vec!["--dangerously-skip-permissions".to_string()]
    }

    fn compact_command(&self) -> Option<&str> {
        Some("/compact")
    }

    fn model_switch_command(&self, model: &str) -> Option<String> {
        Some(format!("/model {}", model))
    }

    fn exit_command(&self) -> Option<&str> {
        Some("/exit")
    }

    fn supports_lazy_skills(&self) -> bool {
        true
    }

    // ── Self-presentation (Chunk B.2) ────────────────────────────

    fn icon_hint(&self) -> Option<&str> {
        Some("✳")
    }

    fn available_models(&self) -> Vec<ModelInfo> {
        vec![
            ModelInfo {
                id: "sonnet".into(),
                display_name: "Claude Sonnet 4.5".into(),
                description: Some("Balanced — fast and capable. Default for most tasks.".into()),
            },
            ModelInfo {
                id: "opus".into(),
                display_name: "Claude Opus 4.6".into(),
                description: Some("Highest capability for complex reasoning.".into()),
            },
            ModelInfo {
                id: "haiku".into(),
                display_name: "Claude Haiku 4.5".into(),
                description: Some("Fastest, lightweight tasks.".into()),
            },
        ]
    }

    fn default_model(&self) -> Option<&str> {
        Some("sonnet")
    }

    fn preserves_context_on_resume(&self) -> bool {
        // Claude replays the JSONL session file on `--resume`, preserving context.
        true
    }

    fn notifications_schema(&self) -> Vec<SettingField> {
        // Mirrors the legacy `ClaudeHooksGroup` React component:
        // status badge from check_claude_hooks + Install action that
        // calls install_claude_hooks. Lives in the global Notifications
        // tab so it sits next to desktop / sound / toast settings.
        vec![SettingField::Section {
            title: "Claude Code Notifications".into(),
            description: Some(
                "Get notified when Claude Code needs permission or finishes a task. \
                 Installs PostToolUse / Stop hooks into ~/.claude/settings.json that POST \
                 back to CodeTerm so notifications fire on the right pane."
                    .into(),
            ),
            fields: vec![
                SettingField::PathLink {
                    label: "Settings file".into(),
                    path: "~/.claude/settings.json".into(),
                    open: PathOpenKind::OpenFolder,
                },
                SettingField::Badge {
                    query_command: "check_claude_hooks".into(),
                    variants: vec![
                        BadgeVariant {
                            id: "installed".into(),
                            label: "installed".into(),
                            style: BadgeStyle::Green,
                        },
                        BadgeVariant {
                            id: "missing".into(),
                            label: "not installed".into(),
                            style: BadgeStyle::Gray,
                        },
                    ],
                },
                SettingField::Action {
                    id: "install_hooks".into(),
                    label: "Install hooks".into(),
                    command: "install_claude_hooks".into(),
                    state_query: Some("check_claude_hooks".into()),
                    confirm: None,
                    style: ActionStyle::Primary,
                },
            ],
        }]
    }

    fn settings_schema(&self) -> Vec<SettingField> {
        vec![
            // ── Section: Session Resume ──
            SettingField::Section {
                title: "Session Resume".into(),
                description: Some(
                    "Automatically resume Claude Code sessions when CodeTerm restarts.".into(),
                ),
                fields: vec![
                    SettingField::Toggle {
                        key: "resume_sessions".into(),
                        label: "Resume previous sessions".into(),
                        default: true,
                        description: None,
                    },
                    SettingField::Toggle {
                        key: "resume_skip_permissions".into(),
                        label: "Resume with --dangerously-skip-permissions".into(),
                        default: true,
                        description: None,
                    },
                    SettingField::When {
                        field: "resume_skip_permissions".into(),
                        equals: serde_json::Value::Bool(true),
                        then: vec![SettingField::Info {
                            title: None,
                            body: "⚠ Claude Code will run without asking for tool approvals. Use with caution.".into(),
                            level: NoteLevel::Warn,
                        }],
                    },
                ],
            },
            // ── Section: Model ──
            SettingField::Section {
                title: "Model".into(),
                description: Some("Default model for new Claude Code sessions.".into()),
                fields: vec![SettingField::Select {
                    key: "default_model".into(),
                    label: "Default model".into(),
                    options: self
                        .available_models()
                        .into_iter()
                        .map(|m| (m.id, m.display_name))
                        .collect(),
                    default: self.default_model().map(|s| s.to_string()),
                    description: None,
                }],
            },
            // ── Section: Rule (Claude-specific instruction file) ──
            SettingField::Section {
                title: "Rule".into(),
                description: Some(
                    "Teaches Claude Code when and how to use CodeTerm — auto-rename tabs, use workspaces instead of mkdir, open browser panel instead of external browser.".into(),
                ),
                fields: vec![
                    SettingField::PathLink {
                        label: "Rule file".into(),
                        path: "~/.claude/rules/codeterm.md".into(),
                        open: PathOpenKind::OpenFolder,
                    },
                    SettingField::Badge {
                        query_command: "check_rule_installed".into(),
                        variants: vec![
                            BadgeVariant {
                                id: "installed".into(),
                                label: "installed".into(),
                                style: BadgeStyle::Green,
                            },
                            BadgeVariant {
                                id: "update".into(),
                                label: "update available".into(),
                                style: BadgeStyle::Orange,
                            },
                            BadgeVariant {
                                id: "missing".into(),
                                label: "not installed".into(),
                                style: BadgeStyle::Gray,
                            },
                        ],
                    },
                    SettingField::Action {
                        id: "install_rule".into(),
                        label: "Install".into(),
                        command: "install_project_rules".into(),
                        state_query: Some("check_rule_installed".into()),
                        confirm: None,
                        style: ActionStyle::Primary,
                    },
                ],
            },
        ]
    }

    fn install_bridge_rule(&self, payload: &str) -> Result<(), String> {
        let rules_dir = dirs::home_dir()
            .ok_or_else(|| "no home directory".to_string())?
            .join(".claude")
            .join("rules");
        std::fs::create_dir_all(&rules_dir)
            .map_err(|e| format!("create {}: {}", rules_dir.display(), e))?;

        let final_path = rules_dir.join("codeterm.md");
        let tmp_path = rules_dir.join("codeterm.md.tmp");

        std::fs::write(&tmp_path, payload)
            .map_err(|e| format!("write {}: {}", tmp_path.display(), e))?;
        std::fs::rename(&tmp_path, &final_path)
            .map_err(|e| format!("rename → {}: {}", final_path.display(), e))?;

        log::info!(
            "[agent-claude] bridge rule installed to {}",
            final_path.display()
        );
        Ok(())
    }

    fn uninstall_bridge_rule(&self) -> Result<(), String> {
        let path = dirs::home_dir()
            .ok_or_else(|| "no home directory".to_string())?
            .join(".claude")
            .join("rules")
            .join("codeterm.md");
        if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|e| format!("remove {}: {}", path.display(), e))?;
            log::info!("[agent-claude] bridge rule removed from {}", path.display());
        }
        Ok(())
    }
}

/// Scan text for Claude-specific events using structural fingerprints.
/// Detects interactive prompts by structure (numbered options + keyboard footer)
/// and task completion ("Word for Xs" pattern unique to Claude's status line).
fn detect_claude_events(text: &str) -> Vec<crate::agent::detector::AgentEvent> {
    use crate::agent::detector::{detect_interactive_prompt, AgentEvent};
    let mut events = Vec::new();

    if detect_interactive_prompt(text) {
        events.push(AgentEvent::PermissionRequest);
    }

    if detect_task_completion(text) {
        events.push(AgentEvent::TaskComplete);
    }

    events
}

/// Structural fingerprint for Claude task completion.
/// Claude shows "<RandomVerb> for <duration>" when done thinking
/// (e.g. "Cogitated for 5s", "Brewed for 12s · $0.05 spent").
/// Pattern: capitalized word + " for " + digits + time unit (s/m/h).
fn detect_task_completion(text: &str) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(10);
    for line in &lines[start..] {
        let trimmed = line.trim();
        if let Some(pos) = trimmed.find(" for ") {
            let word = &trimmed[..pos];
            let after = &trimmed[pos + 5..];
            let is_single_cap_word = !word.is_empty()
                && word.chars().next().map_or(false, |c| c.is_uppercase())
                && !word.contains(' ')
                && word.chars().all(|c| c.is_alphabetic());
            let has_duration = after.starts_with(|c: char| c.is_ascii_digit())
                && (after.contains('s') || after.contains('m') || after.contains('h'));
            if is_single_cap_word && has_duration {
                return true;
            }
        }
    }
    false
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
    fn claude_auto_approve_flags_has_dangerously_skip() {
        let p = ClaudeProvider::new();
        assert_eq!(p.auto_approve_flags(), vec!["--dangerously-skip-permissions"]);
    }

    #[test]
    fn claude_compact_command() {
        let p = ClaudeProvider::new();
        assert_eq!(p.compact_command(), Some("/compact"));
    }

    #[test]
    fn claude_model_switch_command_formats_slash_model() {
        let p = ClaudeProvider::new();
        assert_eq!(p.model_switch_command("sonnet").as_deref(), Some("/model sonnet"));
    }

    #[test]
    fn claude_exit_command() {
        let p = ClaudeProvider::new();
        assert_eq!(p.exit_command(), Some("/exit"));
    }

    #[test]
    fn claude_supports_lazy_skills() {
        let p = ClaudeProvider::new();
        assert!(p.supports_lazy_skills());
    }

    #[test]
    fn claude_install_skills_default_noop_is_ok() {
        let p = ClaudeProvider::new();
        // Default implementation is a no-op that returns Ok(()) — ClaudeProvider
        // doesn't override it yet (real skill delivery comes with Phase 2).
        assert!(p.install_skills(&[]).is_ok());
        assert!(p.uninstall_skills(&[]).is_ok());
    }

    #[test]
    fn test_build_launch_no_task() {
        let p = ClaudeProvider::new();
        let params = AgentLaunchParams {
            agent_name: None,
            task: None,
            args: None,
            skip_permissions: true,
        };
        let cmd = p.build_launch_command(&params);
        if cfg!(target_os = "windows") {
            assert_eq!(
                cmd,
                "Remove-Item Env:CLAUDECODE -ErrorAction SilentlyContinue; claude --dangerously-skip-permissions"
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
            skip_permissions: true,
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
            skip_permissions: true,
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
        // Claude v2.1+ spinner titles
        assert!(p.detect_from_title("⠐ General coding session"));
        assert!(p.detect_from_title("⠂ General coding session"));
        assert!(p.detect_from_title("✳ Claude Code"));
        assert!(p.detect_from_title("✳ Fix bug in parser"));
        // Not Claude
        assert!(!p.detect_from_title("Aider"));
        assert!(!p.detect_from_title("C:\\WINDOWS\\System32\\powershell.exe"));
        assert!(!p.detect_from_title("PS C:\\Users\\User>"));
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

    // --- detect_claude_events tests (moved from agent/detector.rs) ---

    use crate::agent::detector::AgentEvent;

    #[test]
    fn test_detect_claude_permission_prompt() {
        let text = "Reading 1 file...\n\nRead file\n\nDo you want to proceed?\n  ❯ 1. Yes\n    2. Yes, allow reading during this session\n    3. No\n\nEsc to cancel · Tab to amend";
        let events = detect_claude_events(text);
        assert!(events.contains(&AgentEvent::PermissionRequest));
    }

    #[test]
    fn test_detect_prompt_without_selector() {
        let text = "Allow this?\n  1. Yes\n  2. No\n\nEsc to cancel";
        let events = detect_claude_events(text);
        assert!(events.contains(&AgentEvent::PermissionRequest));
    }

    #[test]
    fn test_detect_prompt_with_selector_no_footer() {
        let text = "Choose:\n  ❯ 1. Option A\n    2. Option B\n    3. Option C";
        let events = detect_claude_events(text);
        assert!(events.contains(&AgentEvent::PermissionRequest));
    }

    #[test]
    fn test_no_false_positive_source_code() {
        let text = r#"if text.contains("Do you want to proceed?") && text.contains("Esc to cancel") {
            events.push(PermissionRequest);
        }
        // Check for "1. Yes" and "2. No" patterns
        let options = vec!["1. Yes", "2. No"];"#;
        let events = detect_claude_events(text);
        assert!(!events.contains(&AgentEvent::PermissionRequest));
    }

    #[test]
    fn test_no_false_positive_normal_output() {
        let text = "Building project...\nStep 1. Compile\nStep 2. Link\nStep 3. Package\nDone!";
        let events = detect_claude_events(text);
        assert!(!events.contains(&AgentEvent::PermissionRequest));
    }

    #[test]
    fn test_no_false_positive_single_option() {
        let text = "1. Yes\nEsc to cancel";
        let events = detect_claude_events(text);
        assert!(!events.contains(&AgentEvent::PermissionRequest));
    }

    #[test]
    fn test_detect_task_complete_cogitated() {
        let text = "some output\nCogitated for 5s\n$0.03 spent";
        let events = detect_claude_events(text);
        assert!(events.contains(&AgentEvent::TaskComplete));
    }

    #[test]
    fn test_detect_task_complete_random_verb() {
        for verb in &["Pounced", "Brewed", "Reasoned", "Preambled", "Wandered", "Mused"] {
            let text = format!("output\n{} for 12s · $0.05 spent", verb);
            let events = detect_claude_events(&text);
            assert!(events.contains(&AgentEvent::TaskComplete), "Failed for verb: {}", verb);
        }
    }

    #[test]
    fn test_detect_task_complete_multiunit() {
        let text = "output\nContemplated for 3m 15s";
        let events = detect_claude_events(text);
        assert!(events.contains(&AgentEvent::TaskComplete));
    }

    #[test]
    fn test_no_false_completion_normal_text() {
        let text = "Waiting for response from server...\nCompiling for the first time";
        let events = detect_claude_events(text);
        assert!(!events.contains(&AgentEvent::TaskComplete));
    }

    #[test]
    fn test_detect_no_events_normal_output() {
        let text = "Building project...\nCompilation complete\n";
        let events = detect_claude_events(text);
        assert!(events.is_empty());
    }

    // --- Self-presentation metadata (B.2) ---

    #[test]
    fn claude_metadata_describes_three_models() {
        let p = ClaudeProvider::new();
        let models = p.available_models();
        assert_eq!(models.len(), 3);
        let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
        assert!(ids.contains(&"sonnet"));
        assert!(ids.contains(&"opus"));
        assert!(ids.contains(&"haiku"));
        assert_eq!(p.default_model(), Some("sonnet"));
    }

    #[test]
    fn claude_describe_assembles_full_descriptor() {
        let p = ClaudeProvider::new();
        let desc = p.describe();
        assert_eq!(desc.id, "claude");
        assert_eq!(desc.display_name, "Claude Code");
        assert_eq!(desc.icon_hint.as_deref(), Some("✳"));
        assert_eq!(desc.default_model.as_deref(), Some("sonnet"));
        assert!(desc.preserves_context_on_resume);
        assert!(desc.supports_lazy_skills);
        assert!(desc.has_hooks);
        assert!(!desc.settings_schema.is_empty());
    }

    #[test]
    fn claude_settings_schema_has_required_sections() {
        let p = ClaudeProvider::new();
        let schema = p.settings_schema();
        let section_titles: Vec<&str> = schema
            .iter()
            .filter_map(|f| match f {
                SettingField::Section { title, .. } => Some(title.as_str()),
                _ => None,
            })
            .collect();
        assert!(section_titles.contains(&"Session Resume"));
        assert!(section_titles.contains(&"Model"));
        assert!(section_titles.contains(&"Rule"));
    }

    #[test]
    fn claude_settings_schema_round_trips_as_json() {
        // Snapshot/golden — catches accidental schema breakage.
        let p = ClaudeProvider::new();
        let desc = p.describe();
        let json = serde_json::to_string(&desc).expect("serialize");
        let decoded: crate::agent::provider::ProviderDescriptor =
            serde_json::from_str(&json).expect("deserialize");
        assert_eq!(desc, decoded);
    }

    #[test]
    fn claude_notifications_schema_present() {
        let p = ClaudeProvider::new();
        let schema = p.notifications_schema();
        assert!(!schema.is_empty(), "Claude must contribute notifications");
        // The single top-level Section should be the hooks card.
        match &schema[0] {
            SettingField::Section { title, fields, .. } => {
                assert_eq!(title, "Claude Code Notifications");
                // Must contain a Badge wired to check_claude_hooks AND an
                // Action wired to install_claude_hooks — the two backend
                // commands the legacy ClaudeHooksGroup used.
                let mut saw_badge = false;
                let mut saw_action = false;
                for f in fields {
                    if let SettingField::Badge { query_command, .. } = f {
                        if query_command == "check_claude_hooks" {
                            saw_badge = true;
                        }
                    }
                    if let SettingField::Action { command, state_query, .. } = f {
                        if command == "install_claude_hooks"
                            && state_query.as_deref() == Some("check_claude_hooks")
                        {
                            saw_action = true;
                        }
                    }
                }
                assert!(saw_badge, "expected Badge wired to check_claude_hooks");
                assert!(saw_action, "expected Action wired to install_claude_hooks");
            }
            _ => panic!("expected Section as top-level notifications field"),
        }
    }

    #[test]
    fn claude_describe_includes_notifications_schema() {
        let p = ClaudeProvider::new();
        let desc = p.describe();
        assert!(!desc.notifications_schema.is_empty());
    }
}
