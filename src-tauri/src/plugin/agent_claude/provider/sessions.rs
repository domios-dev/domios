//! Claude session-file enumeration, parsing, and detection.
//!
//! Claude Code stores per-project sessions as JSONL files under
//! `~/.claude/projects/<encoded-cwd>/<uuid>.jsonl`. The encoding is lossy
//! (every non-alphanumeric collapses to `-`), so the source of truth for the
//! real cwd is the `cwd` field on every JSONL message line.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::agent::provider::{self, SessionMessage, SessionMetadata};

/// Encode a CWD path the same way Claude CLI does for project directories.
pub(super) fn encode_cwd(cwd: &str) -> String {
    cwd.chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect()
}

/// Get the sessions directory for a given CWD.
pub(super) fn sessions_dir(cwd: &str) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let encoded = encode_cwd(cwd);
    Some(home.join(".claude").join("projects").join(encoded))
}

/// Read the real cwd out of a Claude session JSONL. Claude's directory
/// encoding collapses both `/` and `_` (and other non-alphanumerics)
/// into `-`, so the dir name like `-Users-x-eco-iq` ambiguously maps
/// back to several real paths (`/Users/x/eco/iq`, `/Users/x/eco_iq`,
/// `/Users/x/e-co/iq`, …). Pick up the authoritative value from the
/// first few JSONL lines, where every message carries a `cwd` field.
fn read_session_cwd(file_path: &Path) -> Option<String> {
    use std::io::{BufRead, BufReader};
    let f = std::fs::File::open(file_path).ok()?;
    let reader = BufReader::new(f);
    // cwd appears on every message line but not on the permission-mode
    // header, so bound the scan to the first ~20 lines.
    for line in reader.lines().take(20).map_while(Result::ok) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
            if let Some(cwd) = v.get("cwd").and_then(|c| c.as_str()) {
                return Some(cwd.to_string());
            }
        }
    }
    None
}

pub(super) fn session_file_path(cwd: &str, session_id: &str) -> Option<PathBuf> {
    let dir = sessions_dir(cwd)?;
    Some(dir.join(format!("{}.jsonl", session_id)))
}

/// Scan a session file from the bottom for the most recent
/// `message.model` value. Claude logs the model on every message, so a
/// mid-session `/model opus` is observable within a tick of the poller.
pub(super) fn detect_session_model(cwd: &str, session_id: &str) -> Option<provider::SessionModel> {
    let path = session_file_path(cwd, session_id)?;
    let text = std::fs::read_to_string(&path).ok()?;
    for line in text.lines().rev() {
        if line.is_empty() { continue; }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            if let Some(m) = v
                .get("message")
                .and_then(|m| m.get("model"))
                .and_then(|m| m.as_str())
            {
                if !m.is_empty() && m != "<synthetic>" {
                    return Some(provider::SessionModel {
                        id: m.to_string(),
                        label: claude_model_short_label(m),
                    });
                }
            }
        }
    }
    None
}

/// Map a raw Claude model id to a short display label. Pure string
/// transform — no IO, no lookups against the registered model catalog
/// (that would couple display logic to runtime registration order).
fn claude_model_short_label(model_id: &str) -> String {
    let m = model_id.to_ascii_lowercase();
    if m.contains("opus") { return "Opus".into(); }
    if m.contains("sonnet") { return "Sonnet".into(); }
    if m.contains("haiku") { return "Haiku".into(); }
    // Strip "claude-" prefix and trailing dashed version, capitalize.
    let body = m.strip_prefix("claude-").unwrap_or(&m);
    let name = body.split('-').next().unwrap_or(body);
    let mut chars = name.chars();
    match chars.next() {
        Some(c) => c.to_ascii_uppercase().to_string() + chars.as_str(),
        None => model_id.to_string(),
    }
}

#[cfg(test)]
mod model_label_tests {
    use super::*;

    #[test]
    fn claude_short_labels_canonical() {
        assert_eq!(claude_model_short_label("claude-opus-4-7"), "Opus");
        assert_eq!(claude_model_short_label("claude-sonnet-4-6"), "Sonnet");
        assert_eq!(claude_model_short_label("claude-haiku-4-5-20251001"), "Haiku");
    }

    #[test]
    fn claude_short_label_unknown_family_falls_back_to_capitalized_root() {
        assert_eq!(claude_model_short_label("claude-uranium-5-0"), "Uranium");
    }
}

pub(super) fn enumerate_sessions() -> Vec<SessionMetadata> {
    let Some(home) = dirs::home_dir() else { return Vec::new() };
    let projects_root = home.join(".claude").join("projects");
    let Ok(entries) = std::fs::read_dir(&projects_root) else { return Vec::new() };

    let mut out = Vec::new();
    for entry in entries.filter_map(|e| e.ok()) {
        if !entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
            continue;
        }
        let dir_path = entry.path();

        // Also look for a sessions-index.json to pick up titles/summaries.
        let index: std::collections::HashMap<String, serde_json::Value> =
            std::fs::read_to_string(dir_path.join("sessions-index.json"))
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                .and_then(|v| {
                    v.get("entries").and_then(|e| e.as_array()).map(|arr| {
                        arr.iter()
                            .filter_map(|it| {
                                it.get("sessionId").and_then(|s| s.as_str()).map(|sid| {
                                    (sid.to_string(), it.clone())
                                })
                            })
                            .collect()
                    })
                })
                .unwrap_or_default();

        let Ok(files) = std::fs::read_dir(&dir_path) else { continue };
        for f in files.filter_map(|e| e.ok()) {
            let name = f.file_name().to_string_lossy().to_string();
            let Some(stem) = name.strip_suffix(".jsonl") else { continue };
            if stem.len() < 32 || !stem.contains('-') {
                continue;
            }
            let file_path_buf = f.path();
            let meta = f.metadata().ok();
            let created_ts = meta.as_ref().and_then(|m| provider::fs_time_to_unix(m.created().ok()));
            let modified_ts = meta.as_ref().and_then(|m| provider::fs_time_to_unix(m.modified().ok()));

            let entry_meta = index.get(stem);
            let title = entry_meta
                .and_then(|v| v.get("title").and_then(|s| s.as_str()))
                .map(String::from);
            let summary = entry_meta
                .and_then(|v| v.get("summary").and_then(|s| s.as_str()))
                .map(String::from);
            let project_path = read_session_cwd(&file_path_buf);

            out.push(SessionMetadata {
                session_id: stem.to_string(),
                provider: "claude".to_string(),
                project_path,
                title,
                summary,
                file_path: file_path_buf.to_string_lossy().to_string(),
                created_ts,
                modified_ts,
            });
        }
    }
    out
}

pub(super) fn parse_session_messages(file_path: &Path) -> Vec<SessionMessage> {
    use std::io::{BufRead, BufReader};
    let Ok(f) = std::fs::File::open(file_path) else { return Vec::new() };
    let reader = BufReader::new(f);
    let mut out = Vec::new();
    for line in reader.lines().map_while(Result::ok) {
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else { continue };
        let msg_type = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if msg_type != "user" && msg_type != "assistant" {
            continue;
        }
        let is_meta = v.get("isMeta").and_then(|b| b.as_bool()).unwrap_or(false);

        // Claude JSONL nests the real content under `message.content`,
        // which can be a string or an array of typed blocks.
        let content = v.get("message").and_then(|m| m.get("content"));
        let text = match content {
            Some(serde_json::Value::String(s)) => s.clone(),
            Some(serde_json::Value::Array(blocks)) => blocks
                .iter()
                .filter_map(|b| {
                    if b.get("type").and_then(|t| t.as_str()) == Some("text") {
                        b.get("text").and_then(|t| t.as_str()).map(String::from)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join(" "),
            _ => continue,
        };
        if text.trim().is_empty() {
            continue;
        }

        let ts = v.get("timestamp")
            .and_then(|t| t.as_str())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.timestamp());

        out.push(SessionMessage { role: msg_type.to_string(), text, ts, is_meta });
    }
    out
}

pub(super) fn detect_session_id(
    cwd: &str,
    exclude: &[String],
    max_age: Option<Duration>,
) -> Option<String> {
    let sessions_dir = sessions_dir(cwd)?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::provider::AgentProvider;

    #[test]
    fn test_encode_cwd() {
        assert_eq!(encode_cwd("/home/user/project"), "-home-user-project");
        assert_eq!(encode_cwd("C:\\Users\\test"), "C--Users-test");
    }

    #[test]
    fn test_session_file_path() {
        let path = session_file_path("/home/user/project", "abc-def-123-456").unwrap();
        let path_str = path.to_string_lossy();
        assert!(path_str.contains("abc-def-123-456.jsonl"));
        assert!(path_str.contains(".claude"));
        assert!(path_str.contains("projects"));
        assert!(path_str.contains("-home-user-project"));
    }

    #[test]
    fn test_session_exists_default_via_file_path() {
        let p = super::super::ClaudeProvider::new();
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path().to_string_lossy().to_string();

        // Build the path the provider expects, then materialize it so the
        // default `session_exists` (which checks `session_file_path().exists()`)
        // can find it. Skips the test if HOME isn't writable.
        let real_path = match p.session_file_path(&cwd, "session-real") {
            Some(p) => p,
            None => return,
        };
        if let Some(parent) = real_path.parent() {
            if std::fs::create_dir_all(parent).is_err() {
                return;
            }
        }
        if std::fs::write(&real_path, "{}\n").is_err() {
            return;
        }

        assert!(p.session_exists(&cwd, "session-real"), "existing JSONL should be detected");
        assert!(!p.session_exists(&cwd, "session-bogus-uuid-xxx"), "missing JSONL must report false");

        let _ = std::fs::remove_file(&real_path);
    }
}
