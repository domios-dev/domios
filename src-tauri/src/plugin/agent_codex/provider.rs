use std::path::PathBuf;
use std::time::Duration;

use crate::agent::provider::{
    AgentLaunchParams, AgentProvider, ModelInfo, NoteLevel, ReadinessStrategy, SettingField,
    TaskDelivery,
};
use crate::shell::ShellSyntax;

/// OpenAI Codex CLI agent provider.
pub struct CodexProvider;

impl CodexProvider {
    pub fn new() -> Self {
        Self
    }

    /// Root sessions directory: ~/.codex/sessions/
    fn sessions_root() -> Option<PathBuf> {
        let home = dirs::home_dir()?;
        Some(home.join(".codex").join("sessions"))
    }
}

impl AgentProvider for CodexProvider {
    fn id(&self) -> &str {
        "codex"
    }

    fn display_name(&self) -> &str {
        "Codex"
    }

    fn build_launch_command(&self, params: &AgentLaunchParams) -> String {
        let mut parts: Vec<String> = Vec::new();

        if params.task.is_some() {
            // Non-interactive: codex exec
            parts.push("codex".to_string());
            parts.push("exec".to_string());
            if params.skip_permissions {
                parts.push("--full-auto".to_string());
            }
            if let Some(ref task) = params.task {
                parts.push("-p".to_string());
                parts.push(ShellSyntax::quote(task));
            }
        } else {
            // Interactive session
            parts.push("codex".to_string());
            if params.skip_permissions {
                parts.push("--dangerously-bypass-approvals-and-sandbox".to_string());
            }
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
        lower.contains("codex")
    }

    fn detect_from_output(&self, text: &str) -> bool {
        text.contains("Would you like to run the following command")
            || text.contains("Would you like to make the following edits")
            || text.contains("approve network access")
            || text.contains("Would you like to grant these permissions")
            || text.contains("codex>")
    }

    fn session_file_path(&self, _cwd: &str, session_id: &str) -> Option<PathBuf> {
        // session_id is a UUID — need to find the matching rollout-*-<uuid>.jsonl file
        let root = Self::sessions_root()?;
        find_session_file_by_uuid(&root, session_id)
    }

    fn detect_session_id(
        &self,
        _cwd: &str,
        exclude: &[String],
        max_age: Option<Duration>,
    ) -> Option<String> {
        let root = Self::sessions_root()?;
        if !root.exists() {
            return None;
        }

        let now = std::time::SystemTime::now();
        let recency_limit = max_age.unwrap_or_else(|| Duration::from_secs(300));
        let mut sessions: Vec<(String, std::time::SystemTime)> = Vec::new();

        // Walk YYYY/MM/DD directories — collects (stem, modified_time)
        collect_jsonl_files(&root, &mut sessions, &now, &recency_limit, exclude, 0);

        sessions.sort_by(|a, b| b.1.cmp(&a.1));
        // Return UUID extracted from the stem (codex resume expects UUID, not filename)
        sessions.first().and_then(|(stem, _)| extract_session_uuid(stem).map(|s| s.to_string()))
    }

    fn build_resume_command(&self, session_id: &str, skip_permissions: bool) -> Option<String> {
        let mut parts = vec!["codex".to_string(), "resume".to_string()];
        if skip_permissions {
            parts.push("--dangerously-bypass-approvals-and-sandbox".to_string());
        }
        // session_id should be a UUID; if it's a full stem, extract the UUID
        let uuid = extract_session_uuid(session_id).unwrap_or(session_id);
        parts.push(ShellSyntax::quote(uuid));
        Some(parts.join(" "))
    }

    fn has_hooks(&self) -> bool {
        false
    }

    fn status_bar_rows(&self) -> u16 {
        1
    }

    fn is_output_noise(&self, text: &str) -> bool {
        let t = text.trim();
        t.is_empty()
            || t.chars().all(|c| c == '─' || c == '━' || c == '═' || c.is_whitespace())
    }

    fn auto_approve_flags(&self) -> Vec<String> {
        vec!["--dangerously-bypass-approvals-and-sandbox".to_string()]
    }

    fn compact_command(&self) -> Option<&str> {
        Some("/compact")
    }

    fn exit_command(&self) -> Option<&str> {
        Some("/exit")
    }

    fn has_event_markers(&self, chunk: &str) -> bool {
        chunk.contains("Would you like")
            || chunk.contains("approve")
            || chunk.contains("run the following")
            || chunk.contains("make the following")
            || chunk.contains("grant these permissions")
    }

    fn detect_events(&self, text: &str) -> Vec<crate::agent::detector::AgentEvent> {
        let mut events = Vec::new();
        if text.contains("Would you like to run the following command")
            || text.contains("Would you like to make the following edits")
            || text.contains("Do you want to approve network access")
            || text.contains("Would you like to grant these permissions")
        {
            events.push(crate::agent::detector::AgentEvent::PermissionRequest);
        }
        events
    }

    // ── Self-presentation (Chunk B.2) ────────────────────────────

    fn icon_hint(&self) -> Option<&str> {
        Some("◆")
    }

    fn available_models(&self) -> Vec<ModelInfo> {
        vec![
            ModelInfo {
                id: "gpt-5".into(),
                display_name: "GPT-5".into(),
                description: Some("OpenAI's flagship coding model.".into()),
            },
            ModelInfo {
                id: "o1".into(),
                display_name: "o1".into(),
                description: Some("Reasoning-optimized model.".into()),
            },
        ]
    }

    fn default_model(&self) -> Option<&str> {
        Some("gpt-5")
    }

    fn preserves_context_on_resume(&self) -> bool {
        // Codex `resume` starts a fresh process and replays only the user
        // turns from the rollout file — context is partially reconstructed,
        // not preserved like Claude's JSONL replay.
        false
    }

    fn settings_schema(&self) -> Vec<SettingField> {
        vec![
            // ── Section: Session Resume ──
            SettingField::Section {
                title: "Session Resume".into(),
                description: Some(
                    "Automatically resume Codex sessions when CodeTerm restarts.".into(),
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
                        label: "Resume with --dangerously-bypass-approvals-and-sandbox".into(),
                        default: true,
                        description: None,
                    },
                    SettingField::When {
                        field: "resume_skip_permissions".into(),
                        equals: serde_json::Value::Bool(true),
                        then: vec![SettingField::Info {
                            title: None,
                            body: "⚠ Codex will run without asking for tool approvals. Use with caution.".into(),
                            level: NoteLevel::Warn,
                        }],
                    },
                ],
            },
            // ── Section: Model ──
            SettingField::Section {
                title: "Model".into(),
                description: Some("Default model for new Codex sessions.".into()),
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
            // ── Section: Requirements (read-only docs) ──
            SettingField::Section {
                title: "Requirements".into(),
                description: None,
                fields: vec![SettingField::Info {
                    title: None,
                    body: "**CLI:** `codex` — install with `npm i -g @openai/codex`\n\n**Auth:** set `OPENAI_API_KEY` or run `codex login`".into(),
                    level: NoteLevel::Info,
                }],
            },
        ]
    }
}

/// Extract UUID from a codex session filename stem.
/// Format: `rollout-YYYY-MM-DDTHH-MM-SS-<UUID>` → returns the UUID part.
/// Codex `resume` command expects the UUID, not the full filename.
fn extract_session_uuid(stem: &str) -> Option<&str> {
    // Pattern: rollout-YYYY-MM-DDTHH-MM-SS-UUID
    // The timestamp part is "YYYY-MM-DDTHH-MM-SS" (19 chars after "rollout-")
    let prefix = "rollout-";
    let rest = stem.strip_prefix(prefix)?;
    // Timestamp: 10 date chars + T + 8 time chars = 19
    // Then a dash separator before UUID
    if rest.len() > 20 && rest.as_bytes()[19] == b'-' {
        Some(&rest[20..])
    } else {
        None
    }
}

/// Recursively find JSONL session files in the Codex date-based directory tree.
fn collect_jsonl_files(
    dir: &std::path::Path,
    sessions: &mut Vec<(String, std::time::SystemTime)>,
    now: &std::time::SystemTime,
    recency_limit: &Duration,
    exclude: &[String],
    depth: u8,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.filter_map(|e| e.ok()) {
        let ft = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };

        if ft.is_dir() && depth < 4 {
            collect_jsonl_files(&entry.path(), sessions, now, recency_limit, exclude, depth + 1);
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".jsonl") {
            continue;
        }

        let stem = name.trim_end_matches(".jsonl");
        if exclude.contains(&stem.to_string()) {
            continue;
        }

        if let Ok(meta) = entry.metadata() {
            if let Ok(modified) = meta.modified() {
                if let Ok(age) = now.duration_since(modified) {
                    if age > *recency_limit {
                        continue;
                    }
                }
            }
            let sort_key = meta.created().or_else(|_| meta.modified());
            if let Ok(ts) = sort_key {
                sessions.push((stem.to_string(), ts));
            }
        }
    }
}

/// Find a session file by UUID anywhere in the date-based tree.
/// Matches files ending with `-<uuid>.jsonl`.
fn find_session_file_by_uuid(root: &std::path::Path, uuid: &str) -> Option<PathBuf> {
    let suffix = format!("-{}.jsonl", uuid);
    find_file_by_suffix(root, &suffix, 0)
}

fn find_file_by_suffix(dir: &std::path::Path, suffix: &str, depth: u8) -> Option<PathBuf> {
    if depth > 4 {
        return None;
    }
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.filter_map(|e| e.ok()) {
        let ft = entry.file_type().ok()?;
        if ft.is_file() && entry.file_name().to_string_lossy().ends_with(suffix) {
            return Some(entry.path());
        }
        if ft.is_dir() {
            if let Some(found) = find_file_by_suffix(&entry.path(), suffix, depth + 1) {
                return Some(found);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_provider_id() {
        let p = CodexProvider::new();
        assert_eq!(p.id(), "codex");
        assert_eq!(p.display_name(), "Codex");
    }

    #[test]
    fn codex_auto_approve_flags() {
        let p = CodexProvider::new();
        assert_eq!(
            p.auto_approve_flags(),
            vec!["--dangerously-bypass-approvals-and-sandbox"]
        );
    }

    #[test]
    fn codex_detect_from_title() {
        let p = CodexProvider::new();
        assert!(p.detect_from_title("codex"));
        assert!(p.detect_from_title("Codex - project"));
        assert!(p.detect_from_title("* My Codex Session"));
        assert!(!p.detect_from_title("claude"));
        assert!(!p.detect_from_title("random terminal"));
    }

    #[test]
    fn codex_detect_from_output() {
        let p = CodexProvider::new();
        assert!(p.detect_from_output("Would you like to run the following command?"));
        assert!(p.detect_from_output("Would you like to make the following edits?"));
        assert!(p.detect_from_output("Do you want to approve network access to \"api.example.com\"?"));
        assert!(!p.detect_from_output("Claude Code v2.0"));
    }

    #[test]
    fn codex_launch_interactive() {
        let p = CodexProvider::new();
        let cmd = p.build_launch_command(&AgentLaunchParams {
            agent_name: None,
            task: None,
            args: None,
            skip_permissions: false,
        });
        assert_eq!(cmd, "codex");
    }

    #[test]
    fn codex_launch_interactive_skip_perms() {
        let p = CodexProvider::new();
        let cmd = p.build_launch_command(&AgentLaunchParams {
            agent_name: None,
            task: None,
            args: None,
            skip_permissions: true,
        });
        assert_eq!(cmd, "codex --dangerously-bypass-approvals-and-sandbox");
    }

    #[test]
    fn codex_launch_with_task() {
        let p = CodexProvider::new();
        let cmd = p.build_launch_command(&AgentLaunchParams {
            agent_name: None,
            task: Some("fix the bug".to_string()),
            args: None,
            skip_permissions: true,
        });
        assert!(cmd.starts_with("codex exec --full-auto -p"));
        assert!(cmd.contains("fix the bug"));
    }

    #[test]
    fn codex_resume_command_with_uuid() {
        let p = CodexProvider::new();
        let cmd = p.build_resume_command("019d6240-e61f-7463-9d7d-6d5afa4e7f93", true).unwrap();
        assert!(cmd.contains("codex resume"));
        assert!(cmd.contains("--dangerously-bypass-approvals-and-sandbox"));
        assert!(cmd.contains("019d6240-e61f-7463-9d7d-6d5afa4e7f93"));
    }

    #[test]
    fn codex_resume_command_extracts_uuid_from_stem() {
        let p = CodexProvider::new();
        let cmd = p.build_resume_command("rollout-2026-04-06T13-05-13-019d6240-e61f-7463-9d7d-6d5afa4e7f93", true).unwrap();
        assert!(cmd.contains("019d6240-e61f-7463-9d7d-6d5afa4e7f93"));
        assert!(!cmd.contains("rollout-"));
    }

    #[test]
    fn extract_uuid_from_stem() {
        assert_eq!(
            extract_session_uuid("rollout-2026-04-06T13-05-13-019d6240-e61f-7463-9d7d-6d5afa4e7f93"),
            Some("019d6240-e61f-7463-9d7d-6d5afa4e7f93")
        );
        assert_eq!(extract_session_uuid("not-a-rollout"), None);
        assert_eq!(extract_session_uuid("rollout-short"), None);
    }

    #[test]
    fn codex_compact_and_exit() {
        let p = CodexProvider::new();
        assert_eq!(p.compact_command(), Some("/compact"));
        assert_eq!(p.exit_command(), Some("/exit"));
        assert_eq!(p.model_switch_command("gpt-5"), None);
    }

    #[test]
    fn codex_event_detection() {
        let p = CodexProvider::new();
        let events = p.detect_events("Would you like to run the following command?\n$ ls -la");
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], crate::agent::detector::AgentEvent::PermissionRequest));

        let events = p.detect_events("Hello, how can I help?");
        assert!(events.is_empty());
    }

    #[test]
    fn codex_noise_detection() {
        let p = CodexProvider::new();
        assert!(p.is_output_noise(""));
        assert!(p.is_output_noise("   "));
        assert!(p.is_output_noise("───────────"));
        assert!(!p.is_output_noise("Some actual output"));
    }

    #[test]
    fn codex_status_bar_rows() {
        let p = CodexProvider::new();
        assert_eq!(p.status_bar_rows(), 1);
    }

    // --- Self-presentation metadata (B.2) ---

    #[test]
    fn codex_metadata_describes_models() {
        let p = CodexProvider::new();
        let models = p.available_models();
        assert!(!models.is_empty());
        let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
        assert!(ids.contains(&"gpt-5"));
        assert_eq!(p.default_model(), Some("gpt-5"));
    }

    #[test]
    fn codex_describe_assembles_full_descriptor() {
        let p = CodexProvider::new();
        let desc = p.describe();
        assert_eq!(desc.id, "codex");
        assert_eq!(desc.display_name, "Codex");
        assert_eq!(desc.icon_hint.as_deref(), Some("◆"));
        assert!(!desc.preserves_context_on_resume); // Codex resumes fresh
        assert!(!desc.has_hooks);
        assert!(!desc.settings_schema.is_empty());
    }

    #[test]
    fn codex_settings_schema_has_required_sections() {
        let p = CodexProvider::new();
        let schema = p.settings_schema();
        let section_titles: Vec<&str> = schema
            .iter()
            .filter_map(|f| match f {
                crate::agent::provider::SettingField::Section { title, .. } => Some(title.as_str()),
                _ => None,
            })
            .collect();
        assert!(section_titles.contains(&"Session Resume"));
        assert!(section_titles.contains(&"Model"));
        assert!(section_titles.contains(&"Requirements"));
    }

    #[test]
    fn codex_settings_schema_round_trips_as_json() {
        let p = CodexProvider::new();
        let desc = p.describe();
        let json = serde_json::to_string(&desc).expect("serialize");
        let decoded: crate::agent::provider::ProviderDescriptor =
            serde_json::from_str(&json).expect("deserialize");
        assert_eq!(desc, decoded);
    }

    #[test]
    fn codex_has_no_notifications_contribution() {
        // Codex doesn't have a hook system like Claude — it contributes
        // an empty notifications_schema. The Notifications tab simply
        // skips it. When Codex grows a notification path, it adds a
        // schema here without any frontend changes.
        let p = CodexProvider::new();
        assert!(p.notifications_schema().is_empty());
        assert!(p.describe().notifications_schema.is_empty());
    }
}
