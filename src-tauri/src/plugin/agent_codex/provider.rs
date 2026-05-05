use std::path::PathBuf;
use std::time::Duration;

use crate::agent::provider::{
    self, AgentLaunchParams, AgentProvider, ModelInfo, NoteLevel, ReadinessStrategy, SessionMessage,
    SessionMetadata, SettingField, TaskDelivery,
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

        if let Some(ref task) = params.task {
            // Non-interactive: codex exec <prompt>
            parts.push("codex".to_string());
            parts.push("exec".to_string());
            if params.skip_permissions {
                parts.push("--dangerously-bypass-approvals-and-sandbox".to_string());
            }
            // Inherit PTY env vars (CODETERM_SOCKET_PATH, etc.) into sandbox
            parts.push("-c".to_string());
            parts.push("'shell_environment_policy.inherit=all'".to_string());
            parts.push("--skip-git-repo-check".to_string());
            parts.push(ShellSyntax::quote(task));
        } else {
            // Interactive session. Codex CLI removed `--full-auto` in late
            // 2026; the surviving skip-approvals flag is the same one `codex
            // exec` takes — `--dangerously-bypass-approvals-and-sandbox`.
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

    fn enumerate_sessions(&self) -> Vec<SessionMetadata> {
        let Some(root) = Self::sessions_root() else { return Vec::new(); };
        let mut out = Vec::new();
        walk_jsonl_recursive(&root, 0, &mut |path| {
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { return };
            let fallback_uuid = extract_session_uuid(stem).map(String::from);

            // First line carries session_meta with the authoritative id + cwd.
            // Falls back to filename-derived uuid when the first line is not
            // session_meta (older codex rollouts, truncated files, etc.).
            let (session_id, cwd) = read_session_meta(path).unwrap_or((None, None));
            let session_id = session_id.or(fallback_uuid);
            let Some(session_id) = session_id else { return };

            let meta = std::fs::metadata(path).ok();
            let created_ts = meta.as_ref().and_then(|m| provider::fs_time_to_unix(m.created().ok()));
            let modified_ts = meta.as_ref().and_then(|m| provider::fs_time_to_unix(m.modified().ok()));

            out.push(SessionMetadata {
                session_id,
                provider: "codex".to_string(),
                project_path: cwd,
                // Codex has no sessions-index.json; titles/summaries would
                // need full-file parsing. Leave None for v1.
                title: None,
                summary: None,
                file_path: path.to_string_lossy().to_string(),
                created_ts,
                modified_ts,
            });
        });
        out
    }

    fn parse_session_messages(&self, file_path: &std::path::Path) -> Vec<SessionMessage> {
        use std::io::{BufRead, BufReader};
        let Ok(f) = std::fs::File::open(file_path) else { return Vec::new() };
        let reader = BufReader::new(f);
        let mut out = Vec::new();
        for line in reader.lines().map_while(Result::ok) {
            if line.is_empty() {
                continue;
            }
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else { continue };
            if v.get("type").and_then(|t| t.as_str()) != Some("response_item") {
                continue;
            }
            let payload = match v.get("payload") {
                Some(p) => p,
                None => continue,
            };
            if payload.get("type").and_then(|t| t.as_str()) != Some("message") {
                continue;
            }
            let role = match payload.get("role").and_then(|r| r.as_str()) {
                Some(r) if r == "user" || r == "assistant" => r.to_string(),
                _ => continue, // skip "developer" and unknown roles
            };

            let text = payload
                .get("content")
                .and_then(|c| c.as_array())
                .map(|blocks| {
                    blocks
                        .iter()
                        .filter_map(|b| {
                            let kind = b.get("type").and_then(|t| t.as_str()).unwrap_or("");
                            if matches!(kind, "input_text" | "output_text" | "text") {
                                b.get("text").and_then(|t| t.as_str()).map(String::from)
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default();

            if text.trim().is_empty() {
                continue;
            }

            // CodeTerm injects a few system-shaped prompts as role=user at
            // session start (AGENTS.md, worker bootstraps, environment
            // context). Mark them meta so cross-provider search skips them.
            let is_meta = role == "user" && is_codeterm_injection(&text);

            let ts = v
                .get("timestamp")
                .and_then(|t| t.as_str())
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.timestamp());

            out.push(SessionMessage { role, text, ts, is_meta });
        }
        out
    }

    fn detect_session_id(
        &self,
        cwd: &str,
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
        // The cwd filter is applied lazily during the walk so we don't read
        // every JSONL header in the tree; only candidates that pass age +
        // exclude get their first line parsed.
        collect_jsonl_files(&root, &mut sessions, &now, &recency_limit, exclude, cwd, 0);

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

    fn model_launch_args(&self, model: &str) -> Vec<String> {
        if model.is_empty() {
            return Vec::new();
        }
        vec!["-m".into(), model.into()]
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

    fn icon_svg(&self) -> Option<&str> {
        Some("<svg viewBox=\"0 0 24 24\" fill=\"currentColor\" fill-rule=\"evenodd\" xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M9.205 8.658v-2.26c0-.19.072-.333.238-.428l4.543-2.616c.619-.357 1.356-.523 2.117-.523 2.854 0 4.662 2.212 4.662 4.566 0 .167 0 .357-.024.547l-4.71-2.759a.797.797 0 00-.856 0l-5.97 3.473zm10.609 8.8V12.06c0-.333-.143-.57-.429-.737l-5.97-3.473 1.95-1.118a.433.433 0 01.476 0l4.543 2.617c1.309.76 2.189 2.378 2.189 3.948 0 1.808-1.07 3.473-2.76 4.163zM7.802 12.703l-1.95-1.142c-.167-.095-.239-.238-.239-.428V5.899c0-2.545 1.95-4.472 4.591-4.472 1 0 1.927.333 2.712.928L8.23 5.067c-.285.166-.428.404-.428.737v6.898zM12 15.128l-2.795-1.57v-3.33L12 8.658l2.795 1.57v3.33L12 15.128zm1.796 7.23c-1 0-1.927-.332-2.712-.927l4.686-2.712c.285-.166.428-.404.428-.737v-6.898l1.974 1.142c.167.095.238.238.238.428v5.233c0 2.545-1.974 4.472-4.614 4.472zm-5.637-5.303l-4.544-2.617c-1.308-.761-2.188-2.378-2.188-3.948A4.482 4.482 0 014.21 6.327v5.423c0 .333.143.571.428.738l5.947 3.449-1.95 1.118a.432.432 0 01-.476 0zm-.262 3.9c-2.688 0-4.662-2.021-4.662-4.519 0-.19.024-.38.047-.57l4.686 2.71c.286.167.571.167.856 0l5.97-3.448v2.26c0 .19-.07.333-.237.428l-4.543 2.616c-.619.357-1.356.523-2.117.523zm5.899 2.83a5.947 5.947 0 005.827-4.756C22.287 18.339 24 15.84 24 13.296c0-1.665-.713-3.282-1.998-4.448.119-.5.19-.999.19-1.498 0-3.401-2.759-5.947-5.946-5.947-.642 0-1.26.095-1.88.31A5.962 5.962 0 0010.205 0a5.947 5.947 0 00-5.827 4.757C1.713 5.447 0 7.945 0 10.49c0 1.666.713 3.283 1.998 4.448-.119.5-.19 1-.19 1.499 0 3.401 2.759 5.946 5.946 5.946.642 0 1.26-.095 1.88-.309a5.96 5.96 0 004.162 1.713z\"/></svg>")
    }

    fn icon_color(&self) -> Option<&str> {
        Some("var(--ct-fg)")
    }

    fn cli_binary(&self) -> Option<&str> {
        Some("codex")
    }

    fn install_command(&self, platform: &str) -> Option<String> {
        match platform {
            "macos" | "linux" | "windows" => Some("npm install -g @openai/codex".into()),
            _ => None,
        }
    }

    fn available_models(&self) -> Vec<ModelInfo> {
        vec![
            ModelInfo {
                id: "gpt-5.4".into(),
                display_name: "GPT-5.4".into(),
                description: Some("Latest frontier agentic coding model.".into()),
            },
            ModelInfo {
                id: "gpt-5.4-mini".into(),
                display_name: "GPT-5.4 Mini".into(),
                description: Some("Smaller frontier agentic model.".into()),
            },
            ModelInfo {
                id: "gpt-5.3-codex".into(),
                display_name: "GPT-5.3 Codex".into(),
                description: Some("Frontier Codex-optimized agentic coding model.".into()),
            },
            ModelInfo {
                id: "gpt-5.2".into(),
                display_name: "GPT-5.2".into(),
                description: Some("Optimized for professional work and long-running agents.".into()),
            },
        ]
    }

    fn default_model(&self) -> Option<&str> {
        Some("gpt-5.4")
    }

    fn preserves_context_on_resume(&self) -> bool {
        // Codex `resume` starts a fresh process and replays only the user
        // turns from the rollout file — context is partially reconstructed,
        // not preserved like Claude's JSONL replay.
        false
    }

    fn settings_schema(&self) -> Vec<SettingField> {
        vec![
            provider::session_resume_section(
                "Automatically resume Codex sessions when CodeTerm restarts.",
                "--dangerously-bypass-approvals-and-sandbox",
            ),
            provider::model_select_section(
                "Default model for new Codex sessions.",
                self.available_models(),
                self.default_model(),
            ),
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
    cwd_filter: &str,
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
            collect_jsonl_files(&entry.path(), sessions, now, recency_limit, exclude, cwd_filter, depth + 1);
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
            // cwd filter — read only the session_meta header (first line) and
            // match `payload.cwd`. Without this, parallel codex panes in the
            // same workspace see candidates from sibling cwds and pick the
            // wrong rollout, then loop forever in the atomic-claim retry path.
            if !cwd_filter.is_empty() && !rollout_cwd_matches(&entry.path(), cwd_filter) {
                continue;
            }
            let sort_key = meta.created().or_else(|_| meta.modified());
            if let Ok(ts) = sort_key {
                sessions.push((stem.to_string(), ts));
            }
        }
    }
}

fn rollout_cwd_matches(path: &std::path::Path, cwd: &str) -> bool {
    use std::io::{BufRead, BufReader};
    let Ok(file) = std::fs::File::open(path) else { return false };
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    if reader.read_line(&mut line).ok().filter(|n| *n > 0).is_none() {
        return false;
    }
    let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) else { return false };
    let kind = val.get("type").and_then(|v| v.as_str()).unwrap_or_default();
    if kind != "session_meta" {
        return false;
    }
    let recorded = match val.get("payload").and_then(|p| p.get("cwd")).and_then(|c| c.as_str()) {
        Some(s) => s,
        None => return false,
    };
    paths_equivalent(recorded, cwd)
}

fn paths_equivalent(a: &str, b: &str) -> bool {
    let norm = |s: &str| -> String {
        let trimmed = s.trim_end_matches('/');
        let canon = std::fs::canonicalize(trimmed)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| trimmed.to_string());
        if cfg!(any(target_os = "macos", target_os = "windows")) {
            canon.to_lowercase()
        } else {
            canon
        }
    };
    norm(a) == norm(b)
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

/// Recursively visit every `*.jsonl` file under the codex sessions tree
/// (which is YYYY/MM/DD deep). Mirror of `collect_jsonl_files` but without
/// the exclude/recency filtering — session search wants them all.
fn walk_jsonl_recursive(dir: &std::path::Path, depth: u8, visit: &mut dyn FnMut(&std::path::Path)) {
    if depth > 5 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.filter_map(|e| e.ok()) {
        let Ok(ft) = entry.file_type() else { continue };
        let path = entry.path();
        if ft.is_dir() {
            walk_jsonl_recursive(&path, depth + 1, visit);
        } else if ft.is_file()
            && path.extension().and_then(|e| e.to_str()) == Some("jsonl")
        {
            visit(&path);
        }
    }
}

/// Read just the first line of a rollout file and extract `(session_id, cwd)`
/// from its `session_meta` payload. Cheap — one line per file, no full parse.
fn read_session_meta(path: &std::path::Path) -> Option<(Option<String>, Option<String>)> {
    use std::io::{BufRead, BufReader};
    let f = std::fs::File::open(path).ok()?;
    let mut reader = BufReader::new(f);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    if v.get("type").and_then(|t| t.as_str()) != Some("session_meta") {
        return None;
    }
    let payload = v.get("payload")?;
    let id = payload.get("id").and_then(|s| s.as_str()).map(String::from);
    let cwd = payload.get("cwd").and_then(|s| s.as_str()).map(String::from);
    Some((id, cwd))
}

/// Filter CodeTerm's own boot-time system messages that codex records as
/// `role: user`. These start with one of a short, explicit set of markers —
/// no regex guessing; unknown injections will show up in search until added.
fn is_codeterm_injection(text: &str) -> bool {
    let t = text.trim_start();
    t.starts_with("# AGENTS.md instructions")
        || t.starts_with("<INSTRUCTIONS>")
        || t.starts_with("<environment_context>")
        || t.starts_with("<permissions instructions>")
        || t.starts_with("You are a Graph Worker")
        || t.starts_with("You are the General Agent")
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
            headless: false,
            session_id: None,
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
            headless: false,
            session_id: None,
        });
        // Codex CLI removed --full-auto; the surviving skip-approvals flag is
        // --dangerously-bypass-approvals-and-sandbox (same one codex exec uses).
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
            headless: false,
            session_id: None,
        });
        assert!(cmd.contains("codex exec --dangerously-bypass-approvals-and-sandbox"));
        assert!(cmd.contains("shell_environment_policy.inherit=all"));
        assert!(cmd.contains("--skip-git-repo-check"));
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
        assert_eq!(models.len(), 4);
        let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
        assert!(ids.contains(&"gpt-5.4"));
        assert!(ids.contains(&"gpt-5.4-mini"));
        assert!(ids.contains(&"gpt-5.3-codex"));
        assert!(ids.contains(&"gpt-5.2"));
        assert_eq!(p.default_model(), Some("gpt-5.4"));
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

    // Meta filter — these strings are injections CodeTerm writes to a codex
    // session as role=user. Session search must mark them is_meta so they
    // don't pollute results with our own bootstrap noise.
    #[test]
    fn is_codeterm_injection_matches_known_boot_prompts() {
        assert!(is_codeterm_injection("# AGENTS.md instructions for /Users/x/proj"));
        assert!(is_codeterm_injection("<INSTRUCTIONS>\nYou are..."));
        assert!(is_codeterm_injection("<environment_context>\n  <cwd>/x</cwd>\n</environment_context>"));
        assert!(is_codeterm_injection("<permissions instructions>\nFilesystem sandboxing..."));
        assert!(is_codeterm_injection("You are a Graph Worker for the CodeTerm memory system."));
        assert!(is_codeterm_injection("You are the General Agent — the always-on system brain"));
        // Leading whitespace is ok — real codex payloads don't trim.
        assert!(is_codeterm_injection("   \n<INSTRUCTIONS>payload"));
    }

    #[test]
    fn is_codeterm_injection_leaves_real_user_messages_alone() {
        assert!(!is_codeterm_injection("fix the bug in parser.rs"));
        assert!(!is_codeterm_injection("why did the deploy fail?"));
        assert!(!is_codeterm_injection("# TODO list\n- ship v2"));
        // User could paste the literal strings inside a question — that's
        // legitimate. Strict `starts_with` keeps false positives low.
        assert!(!is_codeterm_injection("looking at <INSTRUCTIONS> — what does it mean"));
    }

    #[test]
    fn extract_session_uuid_strips_rollout_timestamp_prefix() {
        // Real rollout stem layout: rollout-YYYY-MM-DDTHH-MM-SS-<uuid>
        let stem = "rollout-2026-04-10T14-50-25-019d773a-a5e8-7a02-9295-a40f9f9016ea";
        assert_eq!(
            extract_session_uuid(stem),
            Some("019d773a-a5e8-7a02-9295-a40f9f9016ea"),
        );
        // Malformed input must return None, not a garbled slice.
        assert_eq!(extract_session_uuid("not-a-rollout-file"), None);
        assert_eq!(extract_session_uuid("rollout-short"), None);
    }
}
