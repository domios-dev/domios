use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::agent::provider::{
    self, AgentLaunchParams, AgentProvider, ModelInfo, NoteLevel, ReadinessStrategy, SessionMessage,
    SessionMetadata, SettingField, TaskDelivery,
};
use crate::shell::ShellSyntax;

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
        Self
    }

    /// Root sessions directory: ~/.local/share/opencode/storage/session/
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
}

impl AgentProvider for OpenCodeProvider {
    fn id(&self) -> &str {
        "opencode"
    }

    fn display_name(&self) -> &str {
        "OpenCode"
    }

    fn build_launch_command(&self, params: &AgentLaunchParams) -> String {
        let mut parts: Vec<String> = Vec::new();

        if let Some(ref task) = params.task {
            // Non-interactive: opencode run [flags] -- "<task>"
            // --format json emits the full event stream so the watcher gets
            // continuous output and the idle counter never trips during normal
            // generation. Permission auto-approve only exists on `run`.
            parts.push("opencode".to_string());
            parts.push("run".to_string());
            if params.skip_permissions {
                parts.push("--dangerously-skip-permissions".to_string());
            }
            parts.push("--format".to_string());
            parts.push("json".to_string());
            parts.push(ShellSyntax::quote(task));
        } else {
            // Interactive TUI session.
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
        TaskDelivery::InlineFlag
    }

    fn detect_from_title(&self, title: &str) -> bool {
        let lower = title.to_lowercase();
        lower.contains("opencode")
    }

    fn detect_from_output(&self, text: &str) -> bool {
        // OpenCode TUI writes a distinctive ASCII banner at startup and
        // emits "permission" prompts on tool calls. Match the banner +
        // permission language.
        text.contains("opencode")
            && (text.contains("Permission required")
                || text.contains("approve")
                || text.contains("█▀▀█")
                || text.contains("▄"))
    }

    fn session_file_path(&self, _cwd: &str, session_id: &str) -> Option<PathBuf> {
        // session_id is `ses_XXXXXXXX...`. Walk projectID subdirs and find
        // <projectID>/<session_id>.json.
        let root = Self::sessions_root()?;
        let target = format!("{}.json", session_id);
        find_file_by_name(&root, &target, 0)
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
            // Windows install script lives at the same domain with .ps1 ext.
            "windows" => Some(
                "powershell -Command \"irm https://opencode.ai/install.ps1 | iex\"".into(),
            ),
            _ => None,
        }
    }

    fn available_models(&self) -> Vec<ModelInfo> {
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

    fn default_model(&self) -> Option<&str> {
        Some("anthropic/claude-opus-4-7")
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
                self.default_model(),
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

// ── Live model discovery ────────────────────────────────────────────

/// 5 minute TTL — `opencode models` is fast (<200ms cold) but spawning
/// a subprocess on every settings render is silly.
const MODEL_CACHE_TTL: Duration = Duration::from_secs(300);

struct ModelCache {
    fetched_at: Instant,
    models: Vec<ModelInfo>,
}

fn model_cache() -> &'static Mutex<Option<ModelCache>> {
    static CACHE: OnceLock<Mutex<Option<ModelCache>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

fn cached_opencode_models() -> Option<Vec<ModelInfo>> {
    let guard = model_cache().lock();
    let entry = guard.as_ref()?;
    if entry.fetched_at.elapsed() < MODEL_CACHE_TTL {
        Some(entry.models.clone())
    } else {
        None
    }
}

fn store_opencode_models(models: Vec<ModelInfo>) {
    *model_cache().lock() = Some(ModelCache {
        fetched_at: Instant::now(),
        models,
    });
}

fn fetch_opencode_models() -> Result<Vec<ModelInfo>, String> {
    let output = Command::new("opencode")
        .arg("models")
        .output()
        .map_err(|e| format!("spawn opencode: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "opencode models exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let models: Vec<ModelInfo> = stdout
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && l.contains('/'))
        .map(|id| ModelInfo {
            id: id.to_string(),
            display_name: prettify_model_id(id),
            description: None,
        })
        .collect();

    Ok(models)
}

/// "anthropic/claude-opus-4-7" → "Anthropic · Claude Opus 4.7".
/// Drop in for a richer wire format if/when opencode adds JSON output.
fn prettify_model_id(id: &str) -> String {
    let (provider, model) = id.split_once('/').unwrap_or(("", id));
    let provider_label = match provider {
        "anthropic" => "Anthropic",
        "openai" => "OpenAI",
        "google" => "Google",
        "github-copilot" => "GitHub Copilot",
        "openrouter" => "OpenRouter",
        "opencode" => "OpenCode",
        other if !other.is_empty() => other,
        _ => "",
    };
    let model_label = model
        .split('-')
        .map(|part| {
            // Capitalise first char of each hyphen segment, leave numerics alone.
            let mut chars = part.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    if provider_label.is_empty() {
        model_label
    } else {
        format!("{} · {}", provider_label, model_label)
    }
}

/// Static seed used when the CLI is missing or the fetch fails. Keeps the
/// settings UI usable on a fresh install before `opencode providers login`.
fn fallback_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            id: "anthropic/claude-opus-4-7".into(),
            display_name: "Anthropic · Claude Opus 4.7".into(),
            description: Some("Anthropic frontier model — strongest coding intelligence.".into()),
        },
        ModelInfo {
            id: "anthropic/claude-sonnet-4-6".into(),
            display_name: "Anthropic · Claude Sonnet 4.6".into(),
            description: Some("Anthropic balanced model — fast + capable.".into()),
        },
        ModelInfo {
            id: "anthropic/claude-haiku-4-5".into(),
            display_name: "Anthropic · Claude Haiku 4.5".into(),
            description: Some("Anthropic small model — cheap, fast.".into()),
        },
        ModelInfo {
            id: "openai/gpt-5".into(),
            display_name: "OpenAI · GPT-5".into(),
            description: Some("OpenAI frontier model.".into()),
        },
        ModelInfo {
            id: "opencode/big-pickle".into(),
            display_name: "OpenCode · Big Pickle".into(),
            description: Some("OpenCode-hosted frontier coding model.".into()),
        },
    ]
}

/// CodeTerm injects a few system-shaped prompts as role=user at session
/// start. Mark them meta so cross-provider session search skips them.
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
    fn opencode_provider_id() {
        let p = OpenCodeProvider::new();
        assert_eq!(p.id(), "opencode");
        assert_eq!(p.display_name(), "OpenCode");
    }

    #[test]
    fn opencode_auto_approve_flags() {
        let p = OpenCodeProvider::new();
        assert_eq!(
            p.auto_approve_flags(),
            vec!["--dangerously-skip-permissions"]
        );
    }

    #[test]
    fn opencode_detect_from_title() {
        let p = OpenCodeProvider::new();
        assert!(p.detect_from_title("opencode"));
        assert!(p.detect_from_title("OpenCode - project"));
        assert!(p.detect_from_title("* My OpenCode Session"));
        assert!(!p.detect_from_title("claude"));
        assert!(!p.detect_from_title("codex"));
    }

    #[test]
    fn opencode_launch_interactive() {
        let p = OpenCodeProvider::new();
        let cmd = p.build_launch_command(&AgentLaunchParams {
            agent_name: None,
            task: None,
            args: None,
            skip_permissions: false,
            headless: false,
            session_id: None,
        });
        assert_eq!(cmd, "opencode");
    }

    #[test]
    fn opencode_launch_interactive_skip_perms_does_not_attach_flag() {
        // The --dangerously-skip-permissions flag is `run`-only. Interactive
        // TUI launches must NOT include it (opencode rejects unknown args).
        let p = OpenCodeProvider::new();
        let cmd = p.build_launch_command(&AgentLaunchParams {
            agent_name: None,
            task: None,
            args: None,
            skip_permissions: true,
            headless: false,
            session_id: None,
        });
        assert_eq!(cmd, "opencode");
        assert!(!cmd.contains("--dangerously-skip-permissions"));
    }

    #[test]
    fn opencode_launch_with_task_uses_run_subcommand() {
        let p = OpenCodeProvider::new();
        let cmd = p.build_launch_command(&AgentLaunchParams {
            agent_name: None,
            task: Some("fix the bug".to_string()),
            args: None,
            skip_permissions: true,
            headless: false,
            session_id: None,
        });
        assert!(cmd.starts_with("opencode run"));
        assert!(cmd.contains("--dangerously-skip-permissions"));
        assert!(cmd.contains("--format json"));
        assert!(cmd.contains("fix the bug"));
    }

    #[test]
    fn opencode_launch_with_task_no_skip() {
        let p = OpenCodeProvider::new();
        let cmd = p.build_launch_command(&AgentLaunchParams {
            agent_name: None,
            task: Some("fix the bug".to_string()),
            args: None,
            skip_permissions: false,
            headless: false,
            session_id: None,
        });
        assert!(cmd.starts_with("opencode run"));
        assert!(!cmd.contains("--dangerously-skip-permissions"));
        assert!(cmd.contains("--format json"));
    }

    #[test]
    fn opencode_launch_with_extra_args() {
        let p = OpenCodeProvider::new();
        let cmd = p.build_launch_command(&AgentLaunchParams {
            agent_name: None,
            task: Some("hello".to_string()),
            args: Some(vec!["-m".into(), "anthropic/claude-opus-4-7".into()]),
            skip_permissions: true,
            headless: false,
            session_id: None,
        });
        assert!(cmd.contains("-m anthropic/claude-opus-4-7"));
        assert!(cmd.contains("opencode run"));
    }

    #[test]
    fn opencode_resume_command_uses_session_flag() {
        let p = OpenCodeProvider::new();
        let cmd = p
            .build_resume_command("ses_3bc903c6bffeLofCwfS0EczYak", true)
            .unwrap();
        assert!(cmd.starts_with("opencode --session"));
        assert!(cmd.contains("ses_3bc903c6bffeLofCwfS0EczYak"));
        // TUI doesn't accept --dangerously-skip-permissions even when caller
        // asks for skip — provider must drop it silently.
        assert!(!cmd.contains("--dangerously-skip-permissions"));
    }

    #[test]
    fn opencode_compact_and_exit() {
        let p = OpenCodeProvider::new();
        assert_eq!(p.compact_command(), Some("/compact"));
        assert_eq!(p.exit_command(), Some("/exit"));
    }

    #[test]
    fn opencode_event_detection() {
        let p = OpenCodeProvider::new();
        let events = p.detect_events("Permission required: Bash(rm -rf /)");
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0],
            crate::agent::detector::AgentEvent::PermissionRequest
        ));

        let events = p.detect_events("Hello, how can I help?");
        assert!(events.is_empty());
    }

    #[test]
    fn opencode_noise_detection() {
        let p = OpenCodeProvider::new();
        assert!(p.is_output_noise(""));
        assert!(p.is_output_noise("   "));
        assert!(p.is_output_noise("───────────"));
        assert!(p.is_output_noise("█▀▀█ █▀▀█ █▀▀█"));
        assert!(!p.is_output_noise("Some actual output"));
    }

    #[test]
    fn opencode_status_bar_rows() {
        let p = OpenCodeProvider::new();
        assert_eq!(p.status_bar_rows(), 2);
    }

    #[test]
    fn opencode_describe_assembles_full_descriptor() {
        let p = OpenCodeProvider::new();
        let desc = p.describe();
        assert_eq!(desc.id, "opencode");
        assert_eq!(desc.display_name, "OpenCode");
        assert_eq!(desc.icon_hint.as_deref(), Some("◇"));
        assert_eq!(desc.default_model.as_deref(), Some("anthropic/claude-opus-4-7"));
        assert!(desc.preserves_context_on_resume);
        assert!(!desc.has_hooks);
        assert!(!desc.settings_schema.is_empty());
    }

    #[test]
    fn opencode_settings_schema_has_required_sections() {
        let p = OpenCodeProvider::new();
        let schema = p.settings_schema();
        let titles: Vec<&str> = schema
            .iter()
            .filter_map(|f| match f {
                crate::agent::provider::SettingField::Section { title, .. } => Some(title.as_str()),
                _ => None,
            })
            .collect();
        assert!(titles.contains(&"Session Resume"));
        assert!(titles.contains(&"Model"));
        assert!(titles.contains(&"Requirements"));
    }

    #[test]
    fn opencode_settings_schema_round_trips_as_json() {
        let p = OpenCodeProvider::new();
        let desc = p.describe();
        let json = serde_json::to_string(&desc).expect("serialize");
        let decoded: crate::agent::provider::ProviderDescriptor =
            serde_json::from_str(&json).expect("deserialize");
        assert_eq!(desc, decoded);
    }

    #[test]
    fn opencode_has_no_notifications_contribution() {
        let p = OpenCodeProvider::new();
        assert!(p.notifications_schema().is_empty());
        assert!(p.describe().notifications_schema.is_empty());
    }

    #[test]
    fn opencode_default_model_is_set() {
        let p = OpenCodeProvider::new();
        assert_eq!(p.default_model(), Some("anthropic/claude-opus-4-7"));
    }

    #[test]
    fn opencode_models_returns_non_empty_list_via_live_or_fallback() {
        // Either the live `opencode models` succeeds and we get >0 entries,
        // or the fetch fails and the fallback seed list kicks in. Both paths
        // must produce a non-empty list — settings UI relies on it.
        let p = OpenCodeProvider::new();
        let models = p.available_models();
        assert!(!models.is_empty(), "available_models must never be empty");
        // Every entry must be a proper provider/model pair.
        for m in &models {
            assert!(m.id.contains('/'), "model id without provider prefix: {}", m.id);
        }
    }

    #[test]
    fn opencode_fallback_seed_is_stable() {
        let seed = fallback_models();
        let ids: Vec<&str> = seed.iter().map(|m| m.id.as_str()).collect();
        assert!(ids.contains(&"anthropic/claude-opus-4-7"));
        assert!(ids.contains(&"anthropic/claude-sonnet-4-6"));
        assert!(ids.contains(&"opencode/big-pickle"));
    }

    #[test]
    fn prettify_model_id_humanises_known_providers() {
        assert_eq!(
            prettify_model_id("anthropic/claude-opus-4-7"),
            "Anthropic · Claude Opus 4 7"
        );
        assert_eq!(
            prettify_model_id("github-copilot/gpt-5-mini"),
            "GitHub Copilot · Gpt 5 Mini"
        );
        assert_eq!(prettify_model_id("opencode/big-pickle"), "OpenCode · Big Pickle");
        // Unknown provider — keep the raw segment so we don't lose info.
        assert_eq!(prettify_model_id("zoinks/super-fast"), "zoinks · Super Fast");
        // Bare id without slash — return as-is, capitalised.
        assert_eq!(prettify_model_id("just-a-name"), "Just A Name");
    }

    #[test]
    fn opencode_meta_filter_known_injections() {
        assert!(is_codeterm_injection("# AGENTS.md instructions for /Users/x"));
        assert!(is_codeterm_injection("<INSTRUCTIONS>\nYou are..."));
        assert!(is_codeterm_injection("<environment_context>\n  <cwd>/x</cwd>"));
        assert!(is_codeterm_injection("You are the General Agent — the always-on system brain"));
        assert!(!is_codeterm_injection("fix the bug in parser.rs"));
        assert!(!is_codeterm_injection("# TODO\n- ship v2"));
    }

    #[test]
    fn opencode_session_id_injection_default_none() {
        // OpenCode generates session ids itself (`ses_*`) — no pre-launch
        // injection like Claude's --session-id flag.
        let p = OpenCodeProvider::new();
        assert_eq!(
            p.session_id_injection(),
            crate::agent::provider::SessionIdInjection::None
        );
    }
}
