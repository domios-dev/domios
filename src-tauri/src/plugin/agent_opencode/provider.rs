use std::path::{Path, PathBuf};
use std::time::Duration;

use super::usage as opencode_usage;
use crate::agent::chat_types::StructuredChat;
use crate::agent::provider::{
    self, AgentLaunchParams, AgentProvider, ModelInfo, NoteLevel, ReadinessStrategy,
    SessionMessage, SessionMetadata, SettingField, TaskDelivery, UsageSnapshot,
    VerifySubmitStrategy,
};
use crate::plugin::common::is_codeterm_injection;
use crate::shell::ShellSyntax;

#[path = "labels.rs"]
mod labels;
#[path = "models.rs"]
mod models;
use labels::opencode_short_label;
#[cfg(test)]
use models::prettify_model_id;
use models::{
    cached_opencode_models, fallback_models, fetch_opencode_models, store_opencode_models,
};

/// OpenCode (sst/opencode) agent provider.
///
/// projectID is opaque (sha1 of cwd in current opencode), but we don't have
/// to recompute it: the per-session JSON carries the authoritative `directory`
/// field, so enumeration / detection walks the session tree and filters by
/// that field instead.
pub struct OpenCodeProvider;

impl OpenCodeProvider {
    pub fn new() -> Self {
        // Register fallback models; fetch `opencode models` when no override exists.
        crate::agent::model_catalog::register(
            "opencode",
            crate::agent::model_catalog::RegisteredDefaults {
                models: fallback_models(),
                default: Some("opencode/big-pickle".into()),
            },
        );
        Self
    }

    /// Root sessions directory: ~/.local/share/opencode/storage/session/
    /// Used by older opencode installs that still wrote per-session JSON files
    /// here. Modern opencode (≥0.x with sqlite migration) keeps everything in
    /// `opencode.db` and only writes diff/cache artifacts under storage/.
    fn sessions_root() -> Option<PathBuf> {
        let base = data_dir()?;
        Some(base.join("opencode").join("storage").join("session"))
    }

    /// Root messages directory: ~/.local/share/opencode/storage/message/
    fn messages_root() -> Option<PathBuf> {
        let base = data_dir()?;
        Some(base.join("opencode").join("storage").join("message"))
    }

    /// Root parts directory: ~/.local/share/opencode/storage/part/
    fn parts_root() -> Option<PathBuf> {
        let base = data_dir()?;
        Some(base.join("opencode").join("storage").join("part"))
    }

    /// Path to the modern opencode SQLite store (`opencode.db`). Returns
    /// `None` when opencode hasn't been run yet on this machine.
    fn db_path() -> Option<PathBuf> {
        let base = data_dir()?;
        let p = base.join("opencode").join("opencode.db");
        if p.exists() {
            Some(p)
        } else {
            None
        }
    }

    /// modelID of the most recent assistant message for this pane's session.
    fn db_latest_assistant_model(session_id: &str) -> Option<String> {
        let path = Self::db_path()?;
        let conn = rusqlite::Connection::open_with_flags(
            &path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .ok()?;
        if session_id.is_empty() {
            return None;
        }
        Self::scan_messages(
            &conn,
            "SELECT data FROM message WHERE session_id = ?1 ORDER BY time_created DESC LIMIT 20",
            rusqlite::params![session_id],
        )
    }

    fn scan_messages(
        conn: &rusqlite::Connection,
        sql: &str,
        params: impl rusqlite::Params,
    ) -> Option<String> {
        let mut stmt = conn.prepare(sql).ok()?;
        let rows = stmt.query_map(params, |row| row.get::<_, String>(0)).ok()?;
        for raw in rows.flatten() {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
                continue;
            };
            if v.get("role").and_then(|r| r.as_str()) != Some("assistant") {
                continue;
            }
            if let Some(m) = v.get("modelID").and_then(|m| m.as_str()) {
                if !m.is_empty() {
                    return Some(m.to_string());
                }
            }
        }
        None
    }

    /// Pull every message row for one session out of the sqlite db,
    /// returning the raw `data` JSON of each. Returns None if the db
    /// is missing or the query fails (caller falls back to fs walk).
    fn db_messages(session_id: &str) -> Option<Vec<serde_json::Value>> {
        let path = Self::db_path()?;
        let conn = rusqlite::Connection::open_with_flags(
            &path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .ok()?;
        let mut stmt = conn
            .prepare("SELECT data FROM message WHERE session_id = ?1 ORDER BY time_created ASC")
            .ok()?;
        let rows = stmt
            .query_map(rusqlite::params![session_id], |row| row.get::<_, String>(0))
            .ok()?;
        let out: Vec<serde_json::Value> = rows
            .flatten()
            .filter_map(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .collect();
        if out.is_empty() {
            None
        } else {
            Some(out)
        }
    }

    /// Check the sqlite store for an exact session id. Used by
    /// `session_exists` to validate before resume — if the user wiped the
    /// db between runs the old id stops being resumable.
    fn db_session_exists(session_id: &str) -> bool {
        let Some(path) = Self::db_path() else {
            return false;
        };
        let Ok(conn) = rusqlite::Connection::open_with_flags(
            &path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        ) else {
            return false;
        };
        conn.query_row(
            "SELECT 1 FROM session WHERE id = ?1 LIMIT 1",
            rusqlite::params![session_id],
            |_| Ok(()),
        )
        .is_ok()
    }

    /// Read chat parts for a session from sqlite, newest cursor onward.
    /// Each renderable part (text/reasoning/tool) becomes one `ChatMsg`
    /// keyed by its stable part id. `since_ms` is inclusive (`>=`) so the
    /// still-streaming last message is re-read and its growth picked up.
    fn db_structured_chat(session_id: &str, since_ms: i64) -> StructuredChat {
        let Some(path) = Self::db_path() else {
            return StructuredChat::default();
        };
        Self::query_structured_chat(&path, session_id, since_ms)
    }

    fn query_structured_chat(db_path: &Path, session_id: &str, since_ms: i64) -> StructuredChat {
        let Ok(conn) = rusqlite::Connection::open_with_flags(
            db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        ) else {
            return StructuredChat::default();
        };
        let Ok(mut stmt) = conn.prepare(
            "SELECT p.id, p.time_created, p.data, m.data \
             FROM part p JOIN message m ON m.id = p.message_id \
             WHERE p.session_id = ?1 AND p.time_created >= ?2 \
             ORDER BY p.time_created ASC, p.id ASC",
        ) else {
            return StructuredChat::default();
        };
        let rows = stmt.query_map(rusqlite::params![session_id, since_ms], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        });
        let Ok(rows) = rows else {
            return StructuredChat::default();
        };

        let mut messages = Vec::new();
        let mut cursor = since_ms;
        for (part_id, ts_ms, part_json, msg_json) in rows.flatten() {
            cursor = cursor.max(ts_ms);
            let Ok(part) = serde_json::from_str::<serde_json::Value>(&part_json) else {
                continue;
            };
            let role = serde_json::from_str::<serde_json::Value>(&msg_json)
                .ok()
                .and_then(|m| m.get("role").and_then(|r| r.as_str()).map(String::from))
                .unwrap_or_else(|| "assistant".to_string());
            if let Some(msg) = part_to_chat_msg(&part, &role, part_id, ts_ms) {
                messages.push(msg);
            }
        }

        StructuredChat {
            messages,
            cursor: Some(cursor.to_string()),
        }
    }
}

/// Map one opencode `part` blob to a `ChatMsg`. Handles text (user/assistant
/// by role), reasoning (thinking) and tool (tool_call); other part types
/// (step-start/-finish, compaction, patch, file) are not rendered as chat.
fn part_to_chat_msg(
    part: &serde_json::Value,
    role: &str,
    id: String,
    ts_ms: i64,
) -> Option<crate::agent::chat_types::ChatMsg> {
    use crate::agent::chat_types::ChatMsg;
    let timestamp = Some(ts_ms.to_string());
    match part.get("type").and_then(|t| t.as_str())? {
        "text" => {
            let content = part.get("text").and_then(|t| t.as_str()).unwrap_or("");
            if content.trim().is_empty() {
                return None;
            }
            let kind = if role == "user" { "user" } else { "assistant" };
            Some(ChatMsg {
                id,
                kind: kind.to_string(),
                content: content.to_string(),
                timestamp,
                ..Default::default()
            })
        }
        "reasoning" => {
            let content = part.get("text").and_then(|t| t.as_str()).unwrap_or("");
            if content.trim().is_empty() {
                return None;
            }
            Some(ChatMsg {
                id,
                kind: "thinking".to_string(),
                content: content.to_string(),
                timestamp,
                ..Default::default()
            })
        }
        "tool" => {
            let tool = part.get("tool").and_then(|t| t.as_str()).unwrap_or("tool");
            let state = part.get("state");
            let tool_input = state.and_then(|s| s.get("input")).cloned();
            let tool_result = state
                .and_then(|s| s.get("output"))
                .and_then(|o| o.as_str())
                .map(String::from);
            Some(ChatMsg {
                id,
                kind: "tool_call".to_string(),
                content: String::new(),
                timestamp,
                tool_name: Some(tool.to_string()),
                tool_input,
                tool_result,
                ..Default::default()
            })
        }
        _ => None,
    }
}

impl AgentProvider for OpenCodeProvider {
    fn id(&self) -> &str {
        "opencode"
    }

    fn display_name(&self) -> &str {
        "OpenCode"
    }

    fn binary_name(&self) -> &str {
        "opencode"
    }

    fn build_launch_command(&self, params: &AgentLaunchParams) -> String {
        let mut parts: Vec<String> = Vec::new();
        parts.push("opencode".to_string());
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
        Duration::from_secs(5)
    }

    fn verify_submit_strategy(&self) -> VerifySubmitStrategy {
        VerifySubmitStrategy::TuiInputDrained {
            timeout: Duration::from_secs(15),
            idle_after_drain: Duration::from_millis(800),
        }
    }

    fn detect_from_title(&self, title: &str) -> bool {
        let lower = title.to_lowercase();
        lower.contains("opencode")
    }

    fn detect_from_output(&self, text: &str) -> bool {
        // Detect OpenCode from exact output markers.
        // Exclude generic "approve" prompts to avoid false positives from Claude/Codex.
        let has_marker =
            text.contains("opencode") || text.contains("OpenCode") || text.contains("OpenCode Zen");
        has_marker
            && (text.contains("Permission required")
                || text.contains("Allow this tool call")
                || text.contains("Approve this command")
                || text.contains("█▀▀█")
                || text.contains("Build · ")
                || text.contains("┃")
                || text.contains("╹"))
    }

    fn screen_has_tui(&self, screen_text: &str) -> bool {
        let lower = screen_text.to_ascii_lowercase();
        lower.contains("tab agents") || lower.contains("ctrl+p commands")
    }

    fn session_file_path(&self, _cwd: &str, session_id: &str) -> Option<PathBuf> {
        // session_id is `ses_...`; legacy path is `<project>/<session>.json`.
        // db-only installs return None here and read live DB data.
        let root = Self::sessions_root()?;
        let target = format!("{}.json", session_id);
        find_file_by_name(&root, &target, 0)
    }

    fn session_exists(&self, _cwd: &str, session_id: &str) -> bool {
        // Prefer db when available.
        if Self::db_session_exists(session_id) {
            return true;
        }
        // Legacy fallback for pre-sqlite installs.
        self.session_file_path(_cwd, session_id)
            .map(|p| p.exists())
            .unwrap_or(false)
    }

    fn detect_session_model(&self, _cwd: &str, session_id: &str) -> Option<String> {
        let id = Self::db_latest_assistant_model(session_id)?;
        Some(opencode_short_label(&id))
    }

    fn detect_active_session(&self, _cwd: &str, _current_sid: &str) -> Option<String> {
        None
    }

    fn read_usage(&self, _cwd: &str, session_id: &str) -> Option<UsageSnapshot> {
        if let Some(rows) = Self::db_messages(session_id) {
            if let Some(snap) = opencode_usage::accumulate(rows.into_iter()) {
                return Some(snap);
            }
        }
        let messages_root = Self::messages_root()?;
        let dir = messages_root.join(session_id);
        opencode_usage::read_from_path(&dir)
    }

    fn enumerate_sessions(&self) -> Vec<SessionMetadata> {
        let Some(root) = Self::sessions_root() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let Ok(project_dirs) = std::fs::read_dir(&root) else {
            return out;
        };

        for entry in project_dirs.filter_map(|e| e.ok()) {
            let Ok(ft) = entry.file_type() else { continue };
            if !ft.is_dir() {
                continue;
            }
            let Ok(files) = std::fs::read_dir(entry.path()) else {
                continue;
            };
            for f in files.filter_map(|e| e.ok()) {
                let path = f.path();
                let name = match path.file_name().and_then(|s| s.to_str()) {
                    Some(n) => n.to_string(),
                    None => continue,
                };
                if !name.starts_with("ses_") || !name.ends_with(".json") {
                    continue;
                }
                let session_id = name.trim_end_matches(".json").to_string();

                let info = match read_session_info(&path) {
                    Some(i) => i,
                    None => continue,
                };

                let meta = std::fs::metadata(&path).ok();
                let created_ts = info.created_ms.map(|ms| ms / 1000).or_else(|| {
                    meta.as_ref()
                        .and_then(|m| provider::fs_time_to_unix(m.created().ok()))
                });
                let modified_ts = info.updated_ms.map(|ms| ms / 1000).or_else(|| {
                    meta.as_ref()
                        .and_then(|m| provider::fs_time_to_unix(m.modified().ok()))
                });

                out.push(SessionMetadata {
                    session_id,
                    provider: "opencode".to_string(),
                    project_path: info.directory,
                    title: info.title,
                    summary: None,
                    file_path: path.to_string_lossy().to_string(),
                    created_ts,
                    modified_ts,
                });
            }
        }
        out
    }

    fn uses_structured_chat(&self) -> bool {
        true
    }

    fn read_structured_chat(
        &self,
        _cwd: &str,
        session_id: &str,
        cursor: Option<&str>,
    ) -> StructuredChat {
        let since_ms = cursor.and_then(|c| c.parse::<i64>().ok()).unwrap_or(0);
        Self::db_structured_chat(session_id, since_ms)
    }

    fn parse_session_messages(&self, file_path: &Path) -> Vec<SessionMessage> {
        // file_path points to session metadata; session ID comes from file stem.
        // Messages live under message/<session_id>/part/*.
        let session_id = match file_path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s,
            None => return Vec::new(),
        };

        let Some(messages_root) = Self::messages_root() else {
            return Vec::new();
        };
        let Some(parts_root) = Self::parts_root() else {
            return Vec::new();
        };

        let msg_dir = messages_root.join(session_id);
        let Ok(entries) = std::fs::read_dir(&msg_dir) else {
            return Vec::new();
        };

        let mut out: Vec<(SessionMessage, Option<i64>)> = Vec::new();
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            let name = match path.file_name().and_then(|s| s.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };
            if !name.starts_with("msg_") || !name.ends_with(".json") {
                continue;
            }
            let msg_id = name.trim_end_matches(".json").to_string();

            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(info) = serde_json::from_str::<serde_json::Value>(&text) else {
                continue;
            };

            let role = match info.get("role").and_then(|r| r.as_str()) {
                Some("user") => "user".to_string(),
                Some("assistant") => "assistant".to_string(),
                _ => continue,
            };

            // Concat text parts in part/<msg_id>/.
            let parts_dir = parts_root.join(&msg_id);
            let mut chunks: Vec<(String, Option<String>)> = Vec::new();
            if let Ok(part_entries) = std::fs::read_dir(&parts_dir) {
                for pe in part_entries.filter_map(|e| e.ok()) {
                    let pp = pe.path();
                    let pname = match pp.file_name().and_then(|s| s.to_str()) {
                        Some(n) => n.to_string(),
                        None => continue,
                    };
                    if !pname.starts_with("prt_") || !pname.ends_with(".json") {
                        continue;
                    }
                    let Ok(pt) = std::fs::read_to_string(&pp) else {
                        continue;
                    };
                    let Ok(pv) = serde_json::from_str::<serde_json::Value>(&pt) else {
                        continue;
                    };
                    if pv.get("type").and_then(|t| t.as_str()) != Some("text") {
                        continue;
                    }
                    let body = pv
                        .get("text")
                        .and_then(|t| t.as_str())
                        .unwrap_or("")
                        .to_string();
                    if body.trim().is_empty() {
                        continue;
                    }
                    chunks.push((pname, Some(body)));
                }
            }
            if chunks.is_empty() {
                continue;
            }
            // Sort parts by filename; part IDs are already timestamp-ordered.
            chunks.sort_by(|a, b| a.0.cmp(&b.0));
            let body: String = chunks
                .into_iter()
                .filter_map(|(_, b)| b)
                .collect::<Vec<_>>()
                .join(" ");

            // Treat boot-time injections as meta to avoid polluting session search.
            let is_meta = role == "user" && is_codeterm_injection(&body);

            let ts = info
                .get("time")
                .and_then(|t| t.get("created"))
                .and_then(|c| c.as_i64())
                .map(|ms| ms / 1000);

            out.push((
                SessionMessage {
                    role,
                    text: body,
                    ts,
                    is_meta,
                },
                ts,
            ));
        }

        // Stable ordering: timestamp asc, then filename.
        out.sort_by(|a, b| a.1.unwrap_or(0).cmp(&b.1.unwrap_or(0)));
        out.into_iter().map(|(m, _)| m).collect()
    }

    fn detect_session_id(
        &self,
        _cwd: &str,
        _exclude: &[String],
        _max_age: Option<Duration>,
    ) -> Option<String> {
        None
    }

    fn build_resume_command(&self, session_id: &str, _skip_permissions: bool) -> Option<String> {
        // Resume is TUI-only; never forward `--dangerously-skip-permissions`.
        let parts = vec![
            "opencode".to_string(),
            "--session".to_string(),
            ShellSyntax::quote(session_id),
        ];
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
        2
    }

    fn is_output_noise(&self, text: &str) -> bool {
        let t = text.trim();
        t.is_empty()
            || t.chars().all(|c| {
                c == '─'
                    || c == '━'
                    || c == '═'
                    || c == '█'
                    || c == '▀'
                    || c == '▄'
                    || c == '▌'
                    || c == '▐'
                    || c.is_whitespace()
            })
    }

    fn auto_approve_flags(&self) -> Vec<String> {
        vec!["--dangerously-skip-permissions".to_string()]
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
        chunk.contains("Permission required") || chunk.contains("approve")
    }

    fn detect_events(&self, text: &str) -> Vec<crate::agent::detector::AgentEvent> {
        let mut events = Vec::new();
        if text.contains("Permission required")
            || text.contains("Allow this tool call")
            || text.contains("Approve this command")
        {
            events.push(crate::agent::detector::AgentEvent::PermissionRequest);
        }
        events
    }

    fn icon_hint(&self) -> Option<&str> {
        Some("◇")
    }

    fn icon_svg(&self) -> Option<&str> {
        Some("<svg viewBox=\"0 0 24 24\" fill=\"currentColor\" fill-rule=\"evenodd\" xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M5 3 H19 V21 H5 Z M9 7 H15 V17 H9 Z\"/></svg>")
    }

    fn icon_color(&self) -> Option<&str> {
        Some("var(--ct-fg)")
    }

    fn cli_binary(&self) -> Option<&str> {
        Some("opencode")
    }

    fn install_command(&self, platform: &str) -> Option<String> {
        match platform {
            // macOS/Linux: use upstream script (`curl ... | bash`).
            "macos" | "linux" => Some("curl -fsSL https://opencode.ai/install | bash".into()),
            // Windows: upstream docs list Chocolatey, Scoop, npm, Mise and Docker. NPM is the least opinionated single-command default.
            "windows" => Some("npm install -g opencode-ai".into()),
            _ => None,
        }
    }

    fn available_models(&self) -> Vec<ModelInfo> {
        // Use user override model list first.
        if let Some(o) = crate::agent::model_catalog::try_override("opencode") {
            return o;
        }
        // Read live model list from `opencode models` so it matches current backend auth.
        // Cache models for 5 minutes; fallback to static defaults on error.
        if let Some(cached) = cached_opencode_models() {
            return cached;
        }
        match fetch_opencode_models() {
            Ok(list) if !list.is_empty() => {
                store_opencode_models(list.clone());
                list
            }
            _ => fallback_models(),
        }
    }

    fn default_model(&self) -> Option<String> {
        crate::agent::model_catalog::try_override_default("opencode")
            .or_else(|| self.available_models().first().map(|m| m.id.clone()))
    }

    fn preserves_context_on_resume(&self) -> bool {
        // `--session` replays full session storage (messages + parts) on resume.
        true
    }

    fn settings_schema(&self) -> Vec<SettingField> {
        vec![
            provider::session_resume_section(
                "Automatically resume OpenCode sessions when CodeTerm restarts.",
                "--dangerously-skip-permissions",
            ),
            provider::model_select_section(
                "Default model for new OpenCode sessions.",
                self.available_models(),
                self.default_model().as_deref(),
            ),
            SettingField::Section {
                title: "Requirements".into(),
                description: None,
                fields: vec![SettingField::Info {
                    title: None,
                    body: "**CLI:** `opencode` — macOS/Linux: `curl -fsSL https://opencode.ai/install | bash`; Windows: `npm install -g opencode-ai` (Chocolatey/Scoop also work)\n\n**Auth:** run `opencode providers login` to connect Anthropic, OpenAI, GitHub Copilot, etc.".into(),
                    level: NoteLevel::Info,
                }],
            },
        ]
    }

    // Opencode notifications are already PTY-driven; extra notifications UI isn't needed.
}

// Internal helpers.

/// Collapse common model family names for compact tab badges.
/// Resolve the XDG data directory the same way opencode does:
///   $XDG_DATA_HOME or fallback to $HOME/.local/share
fn data_dir() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("XDG_DATA_HOME") {
        if !p.is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    let home = dirs::home_dir()?;
    Some(home.join(".local").join("share"))
}

/// Subset of the per-session JSON we actually use.
struct SessionInfo {
    directory: Option<String>,
    title: Option<String>,
    created_ms: Option<i64>,
    updated_ms: Option<i64>,
}

fn read_session_info(path: &Path) -> Option<SessionInfo> {
    let text = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let directory = v
        .get("directory")
        .and_then(|d| d.as_str())
        .map(String::from);
    let title = v.get("title").and_then(|t| t.as_str()).map(String::from);
    let created_ms = v
        .get("time")
        .and_then(|t| t.get("created"))
        .and_then(|c| c.as_i64());
    let updated_ms = v
        .get("time")
        .and_then(|t| t.get("updated"))
        .and_then(|c| c.as_i64());
    Some(SessionInfo {
        directory,
        title,
        created_ms,
        updated_ms,
    })
}

fn find_file_by_name(dir: &Path, target: &str, depth: u8) -> Option<PathBuf> {
    if depth > 3 {
        return None;
    }
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.filter_map(|e| e.ok()) {
        let ft = entry.file_type().ok()?;
        if ft.is_file() && entry.file_name().to_string_lossy() == target {
            return Some(entry.path());
        }
        if ft.is_dir() {
            if let Some(found) = find_file_by_name(&entry.path(), target, depth + 1) {
                return Some(found);
            }
        }
    }
    None
}

#[cfg(test)]
#[path = "provider_tests.rs"]
mod tests;
