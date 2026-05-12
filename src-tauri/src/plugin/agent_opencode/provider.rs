use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::agent::provider::{
    self, AgentLaunchParams, AgentProvider, ModelInfo, NoteLevel, ReadinessStrategy, SessionMessage,
    SessionMetadata, SettingField, TaskDelivery, UsageSnapshot, VerifySubmitStrategy,
};
use super::usage as opencode_usage;
use crate::shell::ShellSyntax;
use crate::plugin::common::is_codeterm_injection;

#[path = "models.rs"]
mod models;
use models::{cached_opencode_models, fallback_models, fetch_opencode_models, store_opencode_models};
#[cfg(test)]
use models::prettify_model_id;

/// OpenCode (sst/opencode) agent provider.
///
/// Session storage layout (XDG_DATA_HOME, defaults to ~/.local/share):
///   ~/.local/share/opencode/storage/session/<projectID>/<sessionID>.json   (metadata)
///   ~/.local/share/opencode/storage/message/<sessionID>/<messageID>.json   (per-message header)
///   ~/.local/share/opencode/storage/part/<messageID>/<partID>.json         (text parts)
///   ~/.local/share/opencode/storage/project/<projectID>.json               (project worktree)
///
/// projectID is opaque (sha1 of cwd in current opencode), but we don't have
/// to recompute it: the per-session JSON carries the authoritative `directory`
/// field, so enumeration / detection walks the session tree and filters by
/// that field instead.
pub struct OpenCodeProvider;

impl OpenCodeProvider {
    pub fn new() -> Self {
        // Register fallback list as the "built-in" — actual list is fetched
        // dynamically from `opencode models` when no override file exists.
        crate::agent::model_catalog::register(
            "opencode",
            crate::agent::model_catalog::RegisteredDefaults {
                models: fallback_models(),
                default: Some("anthropic/claude-opus-4-7".into()),
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
        if p.exists() { Some(p) } else { None }
    }

    /// Query the sqlite store for the most recent non-archived session
    /// rooted at `cwd`, skipping ids in `exclude`. `max_age` filters by
    /// `time_updated` (epoch ms in the schema). Returns Some(session_id)
    /// or None when nothing matches / db missing.
    fn db_latest_session(
        cwd: &str,
        exclude: &[String],
        max_age: Option<Duration>,
    ) -> Option<String> {
        let path = Self::db_path()?;
        // Read-only — opencode is the writer; we just observe.
        let conn = rusqlite::Connection::open_with_flags(
            &path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .ok()?;
        // Be liberal with errors — schema drift in newer opencode versions
        // shouldn't blow up the codeterm daemon. If the query fails, fall
        // through to the legacy filesystem walk.
        // Filter on `time_created` (not `time_updated`) so that resuming an
        // older session — which bumps `time_updated` to now — doesn't make it
        // re-eligible for the next fresh pane in the same directory. Each
        // newly-spawned opencode pane should pick up only sessions that were
        // *created* during this detection window. Order by `time_created DESC`
        // for the same reason.
        let mut stmt = conn
            .prepare(
                "SELECT id, time_created FROM session \
                 WHERE directory = ?1 AND time_archived IS NULL \
                 ORDER BY time_created DESC LIMIT 50",
            )
            .ok()?;
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let cutoff_ms = max_age.map(|d| now_ms - d.as_millis() as i64);
        let rows = stmt
            .query_map(rusqlite::params![cwd], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .ok()?;
        for row in rows.flatten() {
            let (id, created_ms) = row;
            if exclude.contains(&id) {
                continue;
            }
            if let Some(cut) = cutoff_ms {
                if created_ms < cut {
                    continue;
                }
            }
            return Some(id);
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
        if out.is_empty() { None } else { Some(out) }
    }

    /// Check the sqlite store for an exact session id. Used by
    /// `session_exists` to validate before resume — if the user wiped the
    /// db between runs the old id stops being resumable.
    fn db_session_exists(session_id: &str) -> bool {
        let Some(path) = Self::db_path() else { return false };
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
        if params.headless && params.task.is_some() {
            parts.push("opencode".to_string());
            parts.push("run".to_string());
            if params.skip_permissions {
                parts.push("--dangerously-skip-permissions".to_string());
            }
            parts.push("--format".to_string());
            parts.push("json".to_string());
            parts.push(ShellSyntax::quote(params.task.as_ref().unwrap()));
        } else {
            parts.push("opencode".to_string());
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
        // OpenCode TUI writes a distinctive box-drawn banner at startup and
        // emits "Permission required" on tool calls. Require literal
        // "opencode" or the TUI's box characters to avoid cross-firing with
        // claude. "approve" is excluded — overlaps codex's "approve network
        // access" prompts.
        let has_marker = text.contains("opencode")
            || text.contains("OpenCode")
            || text.contains("OpenCode Zen");
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
        // session_id is `ses_XXXXXXXX...`. Walk projectID subdirs and find
        // <projectID>/<session_id>.json. Returns None on modern opencode
        // installs where sessions live only in `opencode.db` — the runtime
        // never calls `session_file_path` for those (we override
        // `session_exists` to check the db directly).
        let root = Self::sessions_root()?;
        let target = format!("{}.json", session_id);
        find_file_by_name(&root, &target, 0)
    }

    fn session_exists(&self, _cwd: &str, session_id: &str) -> bool {
        // Modern opencode: db is authoritative.
        if Self::db_session_exists(session_id) {
            return true;
        }
        // Legacy fallback for pre-sqlite installs.
        self.session_file_path(_cwd, session_id)
            .map(|p| p.exists())
            .unwrap_or(false)
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
        let Some(root) = Self::sessions_root() else { return Vec::new(); };
        let mut out = Vec::new();
        let Ok(project_dirs) = std::fs::read_dir(&root) else { return out };

        for entry in project_dirs.filter_map(|e| e.ok()) {
            let Ok(ft) = entry.file_type() else { continue };
            if !ft.is_dir() {
                continue;
            }
            let Ok(files) = std::fs::read_dir(entry.path()) else { continue };
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
                let created_ts = info
                    .created_ms
                    .map(|ms| ms / 1000)
                    .or_else(|| meta.as_ref().and_then(|m| provider::fs_time_to_unix(m.created().ok())));
                let modified_ts = info
                    .updated_ms
                    .map(|ms| ms / 1000)
                    .or_else(|| meta.as_ref().and_then(|m| provider::fs_time_to_unix(m.modified().ok())));

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

    fn parse_session_messages(&self, file_path: &Path) -> Vec<SessionMessage> {
        // file_path points at session/<projectID>/<sessionID>.json — but the
        // actual conversation lives in message/<sessionID>/ + part/<messageID>/.
        // Recover sessionID from the file stem.
        let session_id = match file_path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s,
            None => return Vec::new(),
        };

        let Some(messages_root) = Self::messages_root() else { return Vec::new() };
        let Some(parts_root) = Self::parts_root() else { return Vec::new() };

        let msg_dir = messages_root.join(session_id);
        let Ok(entries) = std::fs::read_dir(&msg_dir) else { return Vec::new() };

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

            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            let Ok(info) = serde_json::from_str::<serde_json::Value>(&text) else { continue };

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
                    let Ok(pt) = std::fs::read_to_string(&pp) else { continue };
                    let Ok(pv) = serde_json::from_str::<serde_json::Value>(&pt) else { continue };
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
            // Sort parts by filename — opencode part ids carry timestamp
            // monotonically so lexical sort = chronological.
            chunks.sort_by(|a, b| a.0.cmp(&b.0));
            let body: String = chunks
                .into_iter()
                .filter_map(|(_, b)| b)
                .collect::<Vec<_>>()
                .join(" ");

            // Treat CodeTerm boot-time injections as meta so they don't
            // pollute cross-provider session search.
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

        // Stable order: by timestamp asc, then by filename order via the
        // initial walk (already deterministic per-fs).
        out.sort_by(|a, b| a.1.unwrap_or(0).cmp(&b.1.unwrap_or(0)));
        out.into_iter().map(|(m, _)| m).collect()
    }

    fn detect_session_id(
        &self,
        cwd: &str,
        exclude: &[String],
        max_age: Option<Duration>,
    ) -> Option<String> {
        // Modern opencode (≥0.x with sqlite migration) keeps sessions in
        // `opencode.db`. Try that first — when the db exists it's the
        // authoritative store; the fs walk below is dead data on those
        // installs.
        if let Some(id) = Self::db_latest_session(cwd, exclude, max_age) {
            return Some(id);
        }

        let root = Self::sessions_root()?;
        if !root.exists() {
            return None;
        }

        let now = std::time::SystemTime::now();
        let recency_limit = max_age.unwrap_or_else(|| Duration::from_secs(300));
        let mut candidates: Vec<(String, std::time::SystemTime)> = Vec::new();

        let Ok(project_dirs) = std::fs::read_dir(&root) else { return None };
        for entry in project_dirs.filter_map(|e| e.ok()) {
            let Ok(ft) = entry.file_type() else { continue };
            if !ft.is_dir() {
                continue;
            }
            let Ok(files) = std::fs::read_dir(entry.path()) else { continue };
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
                if exclude.contains(&session_id) {
                    continue;
                }

                let info = match read_session_info(&path) {
                    Some(i) => i,
                    None => continue,
                };
                // Only consider sessions whose authoritative cwd matches.
                if info.directory.as_deref() != Some(cwd) {
                    continue;
                }

                let Ok(meta) = path.metadata() else { continue };
                let Ok(modified) = meta.modified() else { continue };
                if let Ok(age) = now.duration_since(modified) {
                    if age > recency_limit {
                        continue;
                    }
                }
                let sort_key = meta.created().unwrap_or(modified);
                candidates.push((session_id, sort_key));
            }
        }

        candidates.sort_by(|a, b| b.1.cmp(&a.1));
        candidates.first().map(|(id, _)| id.clone())
    }

    fn build_resume_command(&self, session_id: &str, _skip_permissions: bool) -> Option<String> {
        // OpenCode resume goes through the TUI, which doesn't accept
        // --dangerously-skip-permissions; per-session permission rules
        // are persisted on the session JSON instead. Keep the flag off.
        let parts = vec![
            "opencode".to_string(),
            "--session".to_string(),
            ShellSyntax::quote(session_id),
        ];
        Some(parts.join(" "))
    }

    fn has_hooks(&self) -> bool {
        false
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
            // The official one-liner from opencode.ai/install — works on macOS
            // and Linux. Pulls a static binary into ~/.opencode/bin/.
            "macos" | "linux" => {
                Some("curl -fsSL https://opencode.ai/install | bash".into())
            }
            // Upstream removed the .ps1 installer; the canonical npm package
            // works on stock Windows + Node and matches the claude/codex flow.
            "windows" => Some("npm install -g opencode-ai".into()),
            _ => None,
        }
    }

    fn available_models(&self) -> Vec<ModelInfo> {
        // User override wins outright (skip the dynamic CLI fetch).
        if let Some(o) = crate::agent::model_catalog::try_override("opencode") {
            return o;
        }
        // OpenCode auths into many backends (Anthropic, OpenAI, GitHub Copilot,
        // OpenRouter, OpenCode-hosted, …) — the live list depends on what the
        // user actually logged into. Don't hardcode: shell out to
        // `opencode models`, parse plain stdout (one `provider/model` id per
        // line), cache for 5 minutes. Fall back to a static seed list if the
        // CLI is missing or fails so settings_schema still has options to show.
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
            .or_else(|| Some("anthropic/claude-opus-4-7".into()))
    }

    fn preserves_context_on_resume(&self) -> bool {
        // OpenCode `--session` replays the full session storage (messages
        // + parts) into the new TUI, similar to Claude's JSONL replay.
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
                    body: "**CLI:** `opencode` — install with `npm i -g opencode-ai` or `curl -fsSL https://opencode.ai/install | bash`\n\n**Auth:** run `opencode providers login` to connect Anthropic, OpenAI, GitHub Copilot, etc.".into(),
                    level: NoteLevel::Info,
                }],
            },
        ]
    }

    fn notifications_schema(&self) -> Vec<SettingField> {
        use crate::agent::provider::{BadgeStyle, BadgeVariant};
        vec![SettingField::Section {
            title: "Opencode Notifications".into(),
            description: Some(
                "Notifications fire when Opencode needs your input or finishes a task. \
                 Powered by CodeTerm's pane-output detection — no setup required."
                    .into(),
            ),
            fields: vec![SettingField::Badge {
                query_command: "check_pty_status_available".into(),
                variants: vec![BadgeVariant {
                    id: "ok".into(),
                    label: "auto-enabled".into(),
                    style: BadgeStyle::Green,
                }],
            }],
        }]
    }
}

// ── Internal helpers ────────────────────────────────────────────────

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
        if ft.is_file()
            && entry.file_name().to_string_lossy() == target
        {
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
