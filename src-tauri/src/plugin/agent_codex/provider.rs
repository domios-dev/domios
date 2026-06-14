use std::path::PathBuf;
use std::time::Duration;

use super::status_tui::CodexStatusUsageParser;
use super::usage as codex_usage;
use crate::agent::chat_types::{MessageBlock, RichMessage, SessionDelta};
use crate::agent::provider::{
    self, AgentLaunchParams, AgentProvider, ModelInfo, NoteLevel, ReadinessStrategy,
    SessionMessage, SessionMetadata, SettingField, TaskDelivery, UsageSnapshot,
    VerifySubmitStrategy,
};
use crate::plugin::common::is_codeterm_injection;
use crate::shell::ShellSyntax;

static CODEX_STATUS_PARSER: CodexStatusUsageParser = CodexStatusUsageParser;

/// OpenAI Codex CLI agent provider.
pub struct CodexProvider;

impl CodexProvider {
    pub fn new() -> Self {
        crate::agent::model_catalog::register(
            "codex",
            crate::agent::model_catalog::RegisteredDefaults {
                models: builtin_models(),
                default: Some("gpt-5.5".into()),
            },
        );
        Self
    }

    /// Root sessions directory: ~/.codex/sessions/
    fn sessions_root() -> Option<PathBuf> {
        if let Ok(path) = std::env::var("CODETERM_CODEX_SESSIONS_ROOT") {
            if !path.is_empty() {
                return Some(PathBuf::from(path));
            }
        }
        let home = dirs::home_dir()?;
        Some(home.join(".codex").join("sessions"))
    }
}

fn builtin_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            id: "gpt-5.5".into(),
            display_name: "GPT-5.5".into(),
            description: Some(
                "Frontier model for complex coding, research, and real-world work.".into(),
            ),
        },
        ModelInfo {
            id: "gpt-5.4".into(),
            display_name: "GPT-5.4".into(),
            description: Some("Strong model for everyday coding.".into()),
        },
        ModelInfo {
            id: "gpt-5.4-mini".into(),
            display_name: "GPT-5.4 Mini".into(),
            description: Some(
                "Small, fast, and cost-efficient model for simpler coding tasks.".into(),
            ),
        },
        ModelInfo {
            id: "gpt-5.3-codex".into(),
            display_name: "GPT-5.3 Codex".into(),
            description: Some("Coding-optimized model.".into()),
        },
        ModelInfo {
            id: "gpt-5.2".into(),
            display_name: "GPT-5.2".into(),
            description: Some("Optimized for professional work and long-running agents.".into()),
        },
    ]
}

impl AgentProvider for CodexProvider {
    fn id(&self) -> &str {
        "codex"
    }

    fn supports_chat_ui(&self) -> bool {
        true
    }

    fn display_name(&self) -> &str {
        "Codex"
    }

    fn binary_name(&self) -> &str {
        "codex"
    }

    fn usage_parser(&self) -> Option<&dyn crate::agent::usage_parser::UsageParser> {
        Some(&CODEX_STATUS_PARSER)
    }

    fn usage_probe_command(&self) -> Option<&str> {
        Some("/status")
    }

    fn build_launch_command(&self, params: &AgentLaunchParams) -> String {
        let mut parts: Vec<String> = Vec::new();
        if params.headless && params.task.is_some() {
            parts.push("codex".to_string());
            parts.push("exec".to_string());
            parts.push("--dangerously-bypass-hook-trust".to_string());
            if params.skip_permissions {
                parts.push("--dangerously-bypass-approvals-and-sandbox".to_string());
            }
            parts.push("-c".to_string());
            parts.push("'shell_environment_policy.inherit=all'".to_string());
            parts.push("--skip-git-repo-check".to_string());
            parts.push(ShellSyntax::quote(params.task.as_ref().unwrap()));
        } else {
            parts.push("codex".to_string());
            parts.push("--dangerously-bypass-hook-trust".to_string());
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
        TaskDelivery::PtyInput
    }

    fn task_input_warmup(&self) -> Duration {
        Duration::from_secs(3)
    }

    fn verify_submit_strategy(&self) -> VerifySubmitStrategy {
        VerifySubmitStrategy::TuiInputDrained {
            timeout: Duration::from_secs(15),
            idle_after_drain: Duration::from_millis(800),
        }
    }

    fn detect_from_title(&self, title: &str) -> bool {
        let lower = title.to_lowercase();
        lower.contains("codex")
    }

    fn detect_from_output(&self, text: &str) -> bool {
        // Startup banner — fires reliably the moment codex launches, even before any permission prompt. Used when the user runs `codex` manually
        // inside a shell pane that was previously labelled with another provider.
        text.contains("OpenAI Codex")
            || text.contains("permissions: YOLO mode")
            || text.contains("permissions: read-only")
            // Permission-prompt patterns (existing) — keep for in-session detection
            || text.contains("Would you like to run the following command")
            || text.contains("Would you like to make the following edits")
            || text.contains("approve network access")
            || text.contains("Would you like to grant these permissions")
            || text.contains("codex>")
    }

    fn screen_has_tui(&self, screen_text: &str) -> bool {
        // Banner is always visible at the top of any running Codex session.
        screen_text.contains("OpenAI Codex")
            || screen_text
                .to_ascii_lowercase()
                .contains("ctrl+c to cancel")
    }

    fn session_file_path(&self, _cwd: &str, session_id: &str) -> Option<PathBuf> {
        // session_id is a UUID — need to find the matching rollout-*-<uuid>.jsonl file
        let root = Self::sessions_root()?;
        find_session_file_by_uuid(&root, session_id)
    }

    fn session_jsonl_path(&self, cwd: &str, session_id: &str) -> Option<PathBuf> {
        self.session_file_path(cwd, session_id)
    }

    fn read_usage(&self, cwd: &str, session_id: &str) -> Option<UsageSnapshot> {
        let path = self.session_file_path(cwd, session_id)?;
        codex_usage::read_from_path(&path)
    }

    fn detect_session_model(&self, cwd: &str, session_id: &str) -> Option<String> {
        let path = self.session_file_path(cwd, session_id)?;
        let model = scan_tail_for_model(&path, 64 * 1024)
            .or_else(|| scan_tail_for_model(&path, 512 * 1024))?;
        Some(codex_short_label(&model))
    }

    fn enumerate_sessions(&self) -> Vec<SessionMetadata> {
        let Some(root) = Self::sessions_root() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        walk_jsonl_recursive(&root, 0, &mut |path| {
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                return;
            };
            let fallback_uuid = extract_session_uuid(stem).map(String::from);

            // First line carries session_meta with the authoritative id + cwd. Falls back to filename-derived uuid when the first line is not session_meta
            // (older codex rollouts, truncated files, etc.).
            let (session_id, cwd) = read_session_meta(path).unwrap_or((None, None));
            let session_id = session_id.or(fallback_uuid);
            let Some(session_id) = session_id else { return };

            let meta = std::fs::metadata(path).ok();
            let created_ts = meta
                .as_ref()
                .and_then(|m| provider::fs_time_to_unix(m.created().ok()));
            let modified_ts = meta
                .as_ref()
                .and_then(|m| provider::fs_time_to_unix(m.modified().ok()));

            out.push(SessionMetadata {
                session_id,
                provider: "codex".to_string(),
                project_path: cwd,
                // Codex has no sessions-index.json; titles/summaries would need full-file parsing. Leave None for v1.
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
        let Ok(f) = std::fs::File::open(file_path) else {
            return Vec::new();
        };
        let reader = BufReader::new(f);
        let mut out = Vec::new();
        for line in reader.lines().map_while(Result::ok) {
            if line.is_empty() {
                continue;
            }
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
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

            // CodeTerm injects a few system-shaped prompts as role=user at session start (AGENTS.md, worker bootstraps, environment context). Mark them meta so cross-provider search skips them.
            let is_meta = role == "user" && is_codeterm_injection(&text);

            let ts = v
                .get("timestamp")
                .and_then(|t| t.as_str())
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.timestamp());

            out.push(SessionMessage {
                role,
                text,
                ts,
                is_meta,
            });
        }
        out
    }

    fn parse_session_delta(&self, path: &std::path::Path, from_offset: u64) -> SessionDelta {
        parse_codex_session_delta(path, from_offset)
    }

    fn detect_session_id(
        &self,
        _cwd: &str,
        _exclude: &[String],
        _max_age: Option<Duration>,
    ) -> Option<String> {
        None
    }

    fn detect_launched_session_id(
        &self,
        cwd: &str,
        _exclude: &[String],
        _max_age: Option<Duration>,
        injected_session_id: Option<&str>,
    ) -> Option<String> {
        if let Some(session_id) = injected_session_id {
            if self.session_exists(cwd, session_id) {
                return Some(session_id.to_string());
            }
        }
        None
    }

    fn detect_started_session(
        &self,
        cwd: &str,
        exclude: &[String],
        _max_age: Option<Duration>,
        injected_session_id: Option<&str>,
        screen_text: Option<&str>,
    ) -> Option<String> {
        if let Some(session_id) = injected_session_id {
            if self.session_exists(cwd, session_id) {
                return Some(session_id.to_string());
            }
        }

        let sid = screen_text.and_then(extract_visible_session_uuid)?;
        if exclude.iter().any(|claimed| claimed == &sid) {
            return None;
        }
        if self.session_exists(cwd, &sid) {
            Some(sid)
        } else {
            None
        }
    }

    fn detect_active_session(&self, _cwd: &str, _current_sid: &str) -> Option<String> {
        None
    }

    fn build_resume_command(&self, session_id: &str, skip_permissions: bool) -> Option<String> {
        let mut parts = vec!["codex".to_string(), "resume".to_string()];
        parts.push("--dangerously-bypass-hook-trust".to_string());
        if skip_permissions {
            parts.push("--dangerously-bypass-approvals-and-sandbox".to_string());
        }
        // session_id should be a UUID; if it's a full stem, extract the UUID
        let uuid = extract_session_uuid(session_id).unwrap_or(session_id);
        parts.push(ShellSyntax::quote(uuid));
        Some(parts.join(" "))
    }

    fn has_hooks(&self) -> bool {
        true
    }

    fn check_integration(&self) -> serde_json::Value {
        super::integration::check_integration()
    }

    fn install_integration(&self, api_port: u16) -> Result<(), String> {
        super::integration::install_integration(api_port)
    }

    fn uninstall_integration(&self) -> Result<(), String> {
        super::integration::uninstall_integration()
    }

    fn ensure_integration_on_startup(&self, api_port: u16) {
        super::integration::ensure_integration_on_startup(api_port);
    }

    fn status_bar_rows(&self) -> u16 {
        1
    }

    fn is_output_noise(&self, text: &str) -> bool {
        let t = text.trim();
        t.is_empty()
            || t.chars()
                .all(|c| c == '─' || c == '━' || c == '═' || c.is_whitespace())
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

    // Codex /model is an interactive picker, not arg-based — model_switch_command stays None.

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
        crate::agent::model_catalog::resolve_models("codex")
    }

    fn default_model(&self) -> Option<String> {
        crate::agent::model_catalog::resolve_default("codex")
    }

    fn preserves_context_on_resume(&self) -> bool {
        // Codex `resume` starts a fresh process and replays only the user turns from the rollout file — context is partially reconstructed, not preserved like Claude's JSONL replay.
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
                self.default_model().as_deref(),
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

    // No notifications_schema — Codex notifications fire via the PTY event pipeline without any per-user configuration, so surfacing a status card was just noise.
}

/// Extract UUID from a codex session filename stem.
/// Format: `rollout-YYYY-MM-DDTHH-MM-SS-<UUID>` → returns the UUID part.
/// Codex `resume` command expects the UUID, not the full filename.
fn extract_session_uuid(stem: &str) -> Option<&str> {
    // Pattern: rollout-YYYY-MM-DDTHH-MM-SS-UUID The timestamp part is "YYYY-MM-DDTHH-MM-SS" (19 chars after "rollout-")
    let prefix = "rollout-";
    let rest = stem.strip_prefix(prefix)?;
    // Timestamp: 10 date chars + T + 8 time chars = 19 Then a dash separator before UUID
    if rest.len() > 20 && rest.as_bytes()[19] == b'-' {
        Some(&rest[20..])
    } else {
        None
    }
}

fn extract_visible_session_uuid(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let labels = ["session id", "session_id", "session-id"];
    for label in labels {
        let mut start = 0;
        while let Some(pos) = lower[start..].find(label) {
            let offset = start + pos + label.len();
            if let Some(uuid) = first_uuid_like(&text[offset..]) {
                return Some(uuid.to_string());
            }
            start = offset;
        }
    }
    None
}

fn first_uuid_like(text: &str) -> Option<&str> {
    text.split(|c: char| !(c.is_ascii_hexdigit() || c == '-'))
        .find(|&token| is_uuid_like(token))
}

fn is_uuid_like(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && [8, 13, 18, 23].iter().all(|i| bytes[*i] == b'-')
        && bytes
            .iter()
            .enumerate()
            .all(|(i, b)| [8, 13, 18, 23].contains(&i) || b.is_ascii_hexdigit())
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
/// (which is YYYY/MM/DD deep).
fn walk_jsonl_recursive(dir: &std::path::Path, depth: u8, visit: &mut dyn FnMut(&std::path::Path)) {
    if depth > 5 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let Ok(ft) = entry.file_type() else { continue };
        let path = entry.path();
        if ft.is_dir() {
            walk_jsonl_recursive(&path, depth + 1, visit);
        } else if ft.is_file() && path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
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
    let cwd = payload
        .get("cwd")
        .and_then(|s| s.as_str())
        .map(String::from);
    Some((id, cwd))
}

fn parse_codex_session_delta(path: &std::path::Path, from_offset: u64) -> SessionDelta {
    use std::io::{Read, Seek, SeekFrom};

    let Ok(mut f) = std::fs::File::open(path) else {
        return SessionDelta {
            new_offset: from_offset,
            messages: vec![],
        };
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = from_offset.min(len);
    if f.seek(SeekFrom::Start(start)).is_err() {
        return SessionDelta {
            new_offset: start,
            messages: vec![],
        };
    }
    let mut bytes = Vec::new();
    if f.read_to_end(&mut bytes).is_err() || bytes.is_empty() {
        return SessionDelta {
            new_offset: start,
            messages: vec![],
        };
    }

    let complete_len = if bytes.ends_with(b"\n") {
        bytes.len()
    } else {
        match bytes.iter().rposition(|b| *b == b'\n') {
            Some(pos) => pos + 1,
            None => 0,
        }
    };
    if complete_len == 0 {
        return SessionDelta {
            new_offset: start,
            messages: vec![],
        };
    }

    let text = String::from_utf8_lossy(&bytes[..complete_len]);
    let mut messages = Vec::new();
    for (line_idx, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        if let Some(msg) = parse_codex_rich_message(line, start, line_idx) {
            messages.push(msg);
        }
    }

    SessionDelta {
        new_offset: start + complete_len as u64,
        messages,
    }
}

fn parse_codex_rich_message(line: &str, start: u64, line_idx: usize) -> Option<RichMessage> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    if v.get("type").and_then(|t| t.as_str()) != Some("response_item") {
        return None;
    }
    let payload = v.get("payload")?;
    let timestamp = v
        .get("timestamp")
        .and_then(|t| t.as_str())
        .map(String::from);
    let ts = timestamp
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.timestamp());
    let uuid = payload
        .get("id")
        .or_else(|| payload.get("call_id"))
        .and_then(|id| id.as_str())
        .map(String::from)
        .or_else(|| Some(format!("codex-{start}-{line_idx}")));

    match payload.get("type").and_then(|t| t.as_str())? {
        "message" => parse_codex_message_payload(payload, timestamp, ts, uuid),
        "reasoning" => parse_codex_reasoning_payload(payload, timestamp, ts, uuid),
        "function_call" => parse_codex_function_call_payload(payload, timestamp, ts, uuid),
        "function_call_output" => {
            parse_codex_function_call_output_payload(payload, timestamp, ts, uuid)
        }
        _ => None,
    }
}

fn parse_codex_message_payload(
    payload: &serde_json::Value,
    timestamp: Option<String>,
    ts: Option<i64>,
    uuid: Option<String>,
) -> Option<RichMessage> {
    let role = match payload.get("role").and_then(|r| r.as_str()) {
        Some("developer") | Some("system") => "system",
        Some("user") => "user",
        Some("assistant") => "assistant",
        _ => return None,
    };
    let blocks = payload
        .get("content")
        .and_then(|c| c.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let kind = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    if matches!(kind, "input_text" | "output_text" | "text") {
                        item.get("text")
                            .and_then(|t| t.as_str())
                            .map(|text| MessageBlock {
                                kind: "text".into(),
                                data: serde_json::json!({ "text": text }),
                            })
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if blocks.is_empty() {
        return None;
    }
    let first_text = blocks
        .iter()
        .filter_map(|b| b.data.get("text").and_then(|t| t.as_str()))
        .collect::<Vec<_>>()
        .join("\n");
    Some(RichMessage {
        role: role.into(),
        blocks,
        ts,
        uuid,
        is_meta: Some(role == "system" || (role == "user" && is_codeterm_injection(&first_text))),
        timestamp,
        interactive: vec![],
    })
}

fn parse_codex_reasoning_payload(
    payload: &serde_json::Value,
    timestamp: Option<String>,
    ts: Option<i64>,
    uuid: Option<String>,
) -> Option<RichMessage> {
    let text = payload
        .get("summary")
        .and_then(|s| s.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    item.get("text")
                        .or_else(|| item.get("summary"))
                        .and_then(|t| t.as_str())
                        .map(String::from)
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    if text.trim().is_empty() {
        return None;
    }
    Some(RichMessage {
        role: "assistant".into(),
        blocks: vec![MessageBlock {
            kind: "thinking".into(),
            data: serde_json::json!({ "text": text }),
        }],
        ts,
        uuid,
        is_meta: None,
        timestamp,
        interactive: vec![],
    })
}

fn parse_codex_function_call_payload(
    payload: &serde_json::Value,
    timestamp: Option<String>,
    ts: Option<i64>,
    uuid: Option<String>,
) -> Option<RichMessage> {
    let id = payload
        .get("call_id")
        .or_else(|| payload.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let name = payload.get("name").and_then(|v| v.as_str()).unwrap_or("");
    if id.is_empty() && name.is_empty() {
        return None;
    }
    let input = payload
        .get("arguments")
        .and_then(|v| v.as_str())
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .or_else(|| payload.get("arguments").cloned())
        .unwrap_or_else(|| serde_json::json!({}));
    Some(RichMessage {
        role: "assistant".into(),
        blocks: vec![MessageBlock {
            kind: "tool_use".into(),
            data: serde_json::json!({
                "id": id,
                "name": name,
                "input": input,
            }),
        }],
        ts,
        uuid,
        is_meta: None,
        timestamp,
        interactive: vec![],
    })
}

fn parse_codex_function_call_output_payload(
    payload: &serde_json::Value,
    timestamp: Option<String>,
    ts: Option<i64>,
    uuid: Option<String>,
) -> Option<RichMessage> {
    let call_id = payload
        .get("call_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if call_id.is_empty() {
        return None;
    }
    let output = payload
        .get("output")
        .or_else(|| payload.get("content"))
        .cloned()
        .unwrap_or_else(|| serde_json::Value::String(String::new()));
    Some(RichMessage {
        role: "user".into(),
        blocks: vec![MessageBlock {
            kind: "tool_result".into(),
            data: serde_json::json!({
                "tool_use_id": call_id,
                "content": output,
                "is_error": false,
            }),
        }],
        ts,
        uuid,
        is_meta: None,
        timestamp,
        interactive: vec![],
    })
}

/// Tail-scan a codex jsonl for the latest `turn_context.payload.model`.
/// Codex writes a fresh turn_context every interaction, so the last one is
/// the active model — including post-`/model` swap.
fn scan_tail_for_model(path: &std::path::Path, tail_bytes: u64) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    if len == 0 {
        return None;
    }
    let start = len.saturating_sub(tail_bytes);
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::with_capacity((len - start) as usize);
    f.read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf);
    let mut lines: Vec<&str> = text.split('\n').collect();
    if start > 0 && !lines.is_empty() {
        lines.remove(0);
    }
    for line in lines.iter().rev() {
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if v.get("type").and_then(|t| t.as_str()) != Some("turn_context") {
            continue;
        }
        let m = v
            .get("payload")
            .and_then(|p| p.get("model"))
            .and_then(|m| m.as_str())?;
        if !m.is_empty() {
            return Some(m.to_string());
        }
    }
    None
}

/// Short tab-badge label for a codex model id ("gpt-5-codex" → "Codex",
/// "gpt-5" → "Gpt", "o3-mini" → "O3"). Known families collapse; anything
/// else keeps the first dash-segment so the badge stays readable.
fn codex_short_label(model_id: &str) -> String {
    let m = model_id.to_ascii_lowercase();
    if m.contains("codex") {
        return "Codex".into();
    }
    for fam in ["opus", "sonnet", "haiku", "gemini"] {
        if m.contains(fam) {
            let mut c = fam.chars();
            return c.next().unwrap().to_ascii_uppercase().to_string() + c.as_str();
        }
    }
    // GPT family: keep version + variant suffix so the badge can distinguish gpt-5 from gpt-4o / gpt-5-mini / etc.
    if m.starts_with("gpt") {
        let mut parts = m.split('-').take(3).collect::<Vec<_>>();
        while parts.last() == Some(&"") {
            parts.pop();
        }
        let head = parts.join("-");
        return format!("GPT{}", head.trim_start_matches("gpt"));
    }
    if let Some(first) = m.split('-').next() {
        let mut c = first.chars();
        return c
            .next()
            .map(|x| x.to_ascii_uppercase())
            .into_iter()
            .collect::<String>()
            + c.as_str();
    }
    model_id.to_string()
}

#[cfg(test)]
#[path = "provider_tests.rs"]
mod tests;
