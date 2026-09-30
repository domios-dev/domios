//! Claude session-file enumeration, parsing, and detection.
//!
//! Claude Code stores per-project sessions as JSONL files under
//! `~/.claude/projects/<encoded-cwd>/<uuid>.jsonl`. The encoding is lossy
//! (every non-alphanumeric collapses to `-`), so the source of truth for the
//! real cwd is the `cwd` field on every JSONL message line.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::provider::{self, SessionMessage, SessionMetadata};
use codeterm_types::agent::SessionModelTranscriptBoundary;

/// Encode a CWD path the same way Claude CLI does for project directories.
pub(super) fn encode_cwd(cwd: &str) -> String {
    cwd.chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect()
}

/// Get the sessions directory for a given CWD.
///
/// Claude canonicalizes the cwd (resolving symlinks) before encoding the
/// project dir name — on macOS `/var/...` becomes `/private/var/...`, so a
/// raw cwd would encode to a dir that doesn't exist. Canonicalize to match;
/// fall back to the raw cwd if the path can't be resolved (e.g. it was removed).
pub(super) fn sessions_dir(cwd: &str) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let encoded = encode_cwd(&codeterm_paths::canonical_cwd(cwd));
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
    // cwd appears on every message line but not on the permission-mode header, so bound the scan to the first ~20 lines.
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
    // Hook-reported evidence wins over id-based reconstruction; absence falls through.
    if let Some(stored) = crate::session_evidence::stored_transcript_path(session_id) {
        return Some(stored);
    }
    let dir = sessions_dir(cwd)?;
    Some(dir.join(format!("{}.jsonl", session_id)))
}

/// Build a structured pending prompt for this session from its OWN JSONL — the
/// live, unanswered AskUserQuestion if one is on the wire. Reads only a bounded
/// tail (the prompt and its eventual answer are always near the end) so it's
/// safe to call on the hot path's marker-gated branch. Returns `None` when no
/// structured question is pending. The permission picker has no structured
/// transcript handle, so it falls through to the native detector.
pub(super) fn structured_pending_prompt(
    cwd: &str,
    session_id: &str,
) -> Option<codeterm_types::pty::PendingPrompt> {
    const TAIL_BYTES: u64 = 256 * 1024;
    let path = session_file_path(cwd, session_id)?;
    let messages = read_rich_tail(&path, TAIL_BYTES);
    crate::structured_prompt::active_question_prompt(&messages)
}

/// Parse the tail of a claude session JSONL into enriched [`RichMessage`]s —
/// just enough structure (role + typed content blocks + `interactive`) for
/// [`crate::structured_prompt::active_question_prompt`]. Bounded read;
/// the first (possibly partial) line after a mid-file seek is dropped.
fn read_rich_tail(
    path: &Path,
    tail_bytes: u64,
) -> Vec<codeterm_types::agent::chat_types::RichMessage> {
    use codeterm_types::agent::chat_types::{MessageBlock, RichMessage};
    use std::io::{Read, Seek, SeekFrom};

    let Ok(mut f) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let Ok(len) = f.metadata().map(|m| m.len()) else {
        return Vec::new();
    };
    if len == 0 {
        return Vec::new();
    }
    let start = len.saturating_sub(tail_bytes);
    if f.seek(SeekFrom::Start(start)).is_err() {
        return Vec::new();
    }
    let mut buf = Vec::new();
    if f.read_to_end(&mut buf).is_err() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&buf);
    let mut lines: Vec<&str> = text.split('\n').collect();
    if start > 0 && !lines.is_empty() {
        // Drop the partial first line produced by the mid-file seek.
        lines.remove(0);
    }

    let mut out = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let role = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if role != "user" && role != "assistant" {
            continue;
        }
        let Some(serde_json::Value::Array(content)) =
            v.get("message").and_then(|m| m.get("content"))
        else {
            continue;
        };
        let blocks: Vec<MessageBlock> = content
            .iter()
            .filter_map(|b| {
                let kind = b.get("type").and_then(|t| t.as_str())?.to_string();
                let mut data = b.clone();
                if kind == "tool_use" {
                    add_canonical_tool_fields(&mut data);
                }
                Some(MessageBlock { kind, data })
            })
            .collect();
        let mut msg = RichMessage {
            role: role.to_string(),
            blocks,
            ..Default::default()
        };
        msg.interactive = super::interactive::parse_interactive_events(&msg);
        out.push(msg);
    }
    out
}

fn add_canonical_tool_fields(data: &mut serde_json::Value) {
    let name = data.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let input = data
        .get("input")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let (kind, summary, detail, edits) = canonical_tool_fields(name, &input);
    if let Some(object) = data.as_object_mut() {
        object.insert("tool_kind".into(), serde_json::Value::String(kind));
        object.insert("tool_summary".into(), serde_json::Value::String(summary));
        object.insert("tool_detail".into(), serde_json::Value::String(detail));
        object.insert("tool_edits".into(), edits);
    }
}

fn canonical_tool_fields(
    name: &str,
    input: &serde_json::Value,
) -> (String, String, String, serde_json::Value) {
    let tool = name.trim();
    let object = input.as_object();
    let text = |key: &str| {
        object
            .and_then(|value| value.get(key))
            .and_then(|value| value.as_str())
            .unwrap_or("")
    };
    let command = text("command");
    let path = [text("file_path"), text("filePath"), text("path")]
        .into_iter()
        .find(|value| !value.is_empty())
        .unwrap_or("");
    let pattern = text("pattern");
    let edits = extract_canonical_edits(object);
    let lower = tool.to_ascii_lowercase();
    if matches!(lower.as_str(), "bash" | "shell" | "exec") {
        return (
            "command".into(),
            clean_tool_text(command, "Command"),
            command.into(),
            serde_json::Value::Array(Vec::new()),
        );
    }
    if matches!(lower.as_str(), "edit" | "multiedit" | "write") {
        return (
            "edit".into(),
            if path.is_empty() { "File edit" } else { path }.into(),
            path.into(),
            edits,
        );
    }
    if lower == "read" {
        return (
            "read".into(),
            if path.is_empty() { "Read file" } else { path }.into(),
            path.into(),
            serde_json::Value::Array(Vec::new()),
        );
    }
    if matches!(lower.as_str(), "glob" | "grep" | "search") {
        let detail = if !pattern.is_empty() { pattern } else { path };
        return (
            "search".into(),
            clean_tool_text(detail, "Search"),
            detail.into(),
            serde_json::Value::Array(Vec::new()),
        );
    }
    (
        "generic".into(),
        clean_tool_text(tool, "Tool"),
        tool.into(),
        edits,
    )
}

fn extract_canonical_edits(
    object: Option<&serde_json::Map<String, serde_json::Value>>,
) -> serde_json::Value {
    let Some(object) = object else {
        return serde_json::Value::Array(Vec::new());
    };
    let parent_path = ["file_path", "filePath", "path"]
        .into_iter()
        .filter_map(|key| object.get(key).and_then(|value| value.as_str()))
        .find(|value| !value.is_empty())
        .unwrap_or("");
    let pair = |value: &serde_json::Value| {
        let item = value.as_object()?;
        let old = item
            .get("old_string")
            .or_else(|| item.get("oldString"))
            .and_then(|value| value.as_str());
        let new = item
            .get("new_string")
            .or_else(|| item.get("newString"))
            .and_then(|value| value.as_str());
        if old.is_none() && new.is_none() {
            return None;
        }
        let path = ["file_path", "filePath", "path"]
            .into_iter()
            .filter_map(|key| item.get(key).and_then(|value| value.as_str()))
            .find(|value| !value.is_empty())
            .unwrap_or(parent_path);
        Some(serde_json::json!({
            "path": path,
            "old": old.unwrap_or(""),
            "new": new.unwrap_or(""),
        }))
    };
    if let Some(edits) = object.get("edits").and_then(|value| value.as_array()) {
        return serde_json::Value::Array(edits.iter().filter_map(pair).collect());
    }
    pair(&serde_json::Value::Object(object.clone()))
        .map(|edit| serde_json::Value::Array(vec![edit]))
        .unwrap_or_else(|| serde_json::Value::Array(Vec::new()))
}

fn clean_tool_text(value: &str, fallback: &str) -> String {
    let text = value
        .chars()
        .map(|c| {
            if matches!(c, '\r' | '\n' | '\t') {
                ' '
            } else {
                c
            }
        })
        .collect::<String>()
        .trim()
        .to_string();
    if text.is_empty() {
        fallback.into()
    } else if text.chars().count() > 120 {
        format!("{}...", text.chars().take(117).collect::<String>())
    } else {
        text
    }
}

pub(super) fn detect_session_model(cwd: &str, session_id: &str) -> Option<String> {
    let path = session_file_path(cwd, session_id)?;
    // hundreds of KB of file-backup/tool-call metadata trailing the last message. 4MB is plenty without hitting the disk hard.
    scan_tail(&path, 64 * 1024)
        .or_else(|| scan_tail(&path, 512 * 1024))
        .or_else(|| scan_tail(&path, 4 * 1024 * 1024))
}

pub(super) fn session_model_transcript_boundary(
    cwd: &str,
    session_id: &str,
) -> Option<SessionModelTranscriptBoundary> {
    let path = session_file_path(cwd, session_id)?;
    let contents = read_model_tail(&path)?;
    transcript_model_boundary(&contents)
}

pub(super) fn detect_session_model_after_boundary(
    cwd: &str,
    session_id: &str,
    boundary: &SessionModelTranscriptBoundary,
) -> Option<String> {
    let path = session_file_path(cwd, session_id)?;
    let contents = read_model_tail(&path)?;
    transcript_model_after_boundary(&contents, boundary)
}

fn transcript_model_boundary(contents: &str) -> Option<SessionModelTranscriptBoundary> {
    if contents.is_empty() {
        return Some(SessionModelTranscriptBoundary {
            last_entry_uuid: None,
            last_entry_timestamp: None,
            last_entry_type: None,
            boundary_found: true,
        });
    }
    if contents.lines().all(|line| line.trim().is_empty()) {
        return Some(SessionModelTranscriptBoundary {
            last_entry_uuid: None,
            last_entry_timestamp: None,
            last_entry_type: None,
            boundary_found: true,
        });
    }
    for line in contents.lines().rev().filter(|line| !line.trim().is_empty()) {
        let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if let Some(uuid) = entry
            .get("uuid")
            .and_then(serde_json::Value::as_str)
            .filter(|uuid| !uuid.is_empty())
        {
            return Some(SessionModelTranscriptBoundary {
                last_entry_uuid: Some(uuid.to_string()),
                last_entry_timestamp: None,
                last_entry_type: None,
                boundary_found: true,
            });
        }
        let entry_type = entry.get("type").and_then(serde_json::Value::as_str);
        if matches!(entry_type, Some("user" | "assistant")) {
            let timestamp = entry
                .get("timestamp")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|timestamp| {
                    !timestamp.is_empty()
                        && !timestamp
                            .chars()
                            .any(|character| character <= '\u{1f}' || character == '\u{7f}')
                });
            let Some(timestamp) = timestamp else {
                return None;
            };
            return Some(SessionModelTranscriptBoundary {
                last_entry_uuid: None,
                last_entry_timestamp: Some(timestamp.to_string()),
                last_entry_type: entry_type.map(str::to_string),
                boundary_found: true,
            });
        }
    }
    None
}

fn transcript_model_after_boundary(
    contents: &str,
    boundary: &SessionModelTranscriptBoundary,
) -> Option<String> {
    if !boundary.boundary_found {
        return None;
    }
    let entries = contents
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .collect::<Vec<_>>();
    let start_index = match boundary.last_entry_uuid.as_deref() {
        Some(uuid) => entries
            .iter()
            .rposition(|entry| entry.get("uuid").and_then(serde_json::Value::as_str) == Some(uuid))?
            .saturating_add(1),
        None => match (
            boundary.last_entry_type.as_deref(),
            boundary.last_entry_timestamp.as_deref(),
        ) {
            (Some(entry_type), Some(timestamp)) => entries
                .iter()
                .rposition(|entry| {
                    entry.get("type").and_then(serde_json::Value::as_str) == Some(entry_type)
                        && entry
                            .get("timestamp")
                            .and_then(serde_json::Value::as_str)
                            .map(str::trim)
                            == Some(timestamp)
                })?
                .saturating_add(1),
            _ => 0,
        },
    };
    let entries = &entries[start_index..];
    entries.iter().rev().find_map(|entry| {
        if entry.get("type").and_then(serde_json::Value::as_str) != Some("assistant") {
            return None;
        }
        let model = entry
            .get("message")
            .and_then(|message| message.get("model"))
            .and_then(serde_json::Value::as_str)?;
        (!model.is_empty() && model != "<synthetic>").then(|| family_from_model_id(model))
    })
}

fn read_model_tail(path: &Path) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};

    const TAIL_BYTES: u64 = 4 * 1024 * 1024;
    let mut file = std::fs::File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    let start = length.saturating_sub(TAIL_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::with_capacity((length - start) as usize);
    file.read_to_end(&mut bytes).ok()?;
    if start > 0 {
        if let Some(line_end) = bytes.iter().position(|byte| *byte == b'\n') {
            bytes.drain(..=line_end);
        }
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

pub(super) fn detect_session_reasoning_effort(cwd: &str, session_id: &str) -> Option<String> {
    let path = session_file_path(cwd, session_id)?;
    scan_effort_tail(&path, 64 * 1024)
        .or_else(|| scan_effort_tail(&path, 512 * 1024))
        .or_else(|| scan_effort_tail(&path, 4 * 1024 * 1024))
}

fn scan_effort_tail(path: &Path, tail_bytes: u64) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    if len == 0 {
        return None;
    }
    let start = len.saturating_sub(tail_bytes);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::with_capacity((len - start) as usize);
    file.read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let mut lines: Vec<&str> = text.split('\n').collect();
    if start > 0 && !lines.is_empty() {
        lines.remove(0);
    }
    lines.iter().rev().find_map(|line| {
        let value = serde_json::from_str::<serde_json::Value>(line).ok()?;
        session_entry_reasoning_effort(&value)
    })
}

fn session_entry_reasoning_effort(value: &serde_json::Value) -> Option<String> {
    let candidates = [
        value.get("effort"),
        value.get("reasoning_effort"),
        value.get("reasoningEffort"),
        value
            .get("message")
            .and_then(|message| message.get("effort")),
        value
            .get("message")
            .and_then(|message| message.get("reasoning_effort")),
        value
            .get("message")
            .and_then(|message| message.get("metadata"))
            .and_then(|metadata| metadata.get("effort")),
    ];
    candidates
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(str::trim)
        .find(|effort| matches!(*effort, "low" | "medium" | "high" | "xhigh" | "max"))
        .map(str::to_string)
}

fn scan_tail(path: &Path, tail_bytes: u64) -> Option<String> {
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
        // /model X writes "Set model to X" before the next assistant turn.
        if let Some(content) = v.get("message").and_then(|m| m.get("content")) {
            let raw = content_to_text(content);
            if let Some(fam) = extract_set_model_family(&raw) {
                return Some(fam);
            }
        }
        // Last assistant turn carries the authoritative model id.
        if let Some(m) = v
            .get("message")
            .and_then(|m| m.get("model"))
            .and_then(|m| m.as_str())
        {
            if !m.is_empty() && m != "<synthetic>" {
                return Some(family_from_model_id(m));
            }
        }
    }
    None
}

fn content_to_text(c: &serde_json::Value) -> String {
    match c {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(arr) => arr
            .iter()
            .filter_map(|c| c.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    }
}

fn extract_set_model_family(raw: &str) -> Option<String> {
    let idx = raw.find("Set model to ")?;
    let after = &raw[idx + "Set model to ".len()..];
    let cleaned = strip_ansi(after);
    let family: String = cleaned
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect();
    if family.is_empty() {
        return None;
    }
    Some(capitalize_family(&family))
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            while let Some(c) = chars.next() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn family_from_model_id(id: &str) -> String {
    let m = id.to_ascii_lowercase();
    for fam in ["opus", "sonnet", "haiku"] {
        if m.contains(fam) {
            return capitalize_family(fam);
        }
    }
    let body = m.strip_prefix("claude-").unwrap_or(&m);
    let head = body.split('-').next().unwrap_or(body);
    capitalize_family(head)
}

fn capitalize_family(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_ascii_uppercase().to_string() + &chars.as_str().to_ascii_lowercase(),
        None => String::new(),
    }
}

#[cfg(test)]
mod model_label_tests;

type SessionsIndex = std::collections::HashMap<String, serde_json::Value>;

fn claude_projects_root() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".claude").join("projects"))
}

fn is_session_stem(stem: &str) -> bool {
    stem.len() >= 32 && stem.contains('-')
}

pub(super) fn enumerate_sessions() -> Vec<SessionMetadata> {
    claude_projects_root()
        .map(|root| enumerate_sessions_under(&root))
        .unwrap_or_default()
}

pub(crate) fn enumerate_sessions_under(projects_root: &Path) -> Vec<SessionMetadata> {
    let Ok(entries) = std::fs::read_dir(projects_root) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.filter_map(|e| e.ok()) {
        if !entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
            continue;
        }
        let dir_path = entry.path();
        let index = read_sessions_index(&dir_path);
        let Ok(files) = std::fs::read_dir(&dir_path) else {
            continue;
        };
        for f in files.filter_map(|e| e.ok()) {
            let name = f.file_name().to_string_lossy().to_string();
            let Some(stem) = name.strip_suffix(".jsonl") else {
                continue;
            };
            if is_session_stem(stem) {
                out.push(session_metadata(stem, &f.path(), &index));
            }
        }
    }
    out
}

pub(super) fn find_session(session_id: &str) -> Option<SessionMetadata> {
    find_session_under(&claude_projects_root()?, session_id)
}

/// Probes `<project>/<id>.jsonl` once per project dir, so the cost follows the
/// project count and never the number of stored sessions.
pub(crate) fn find_session_under(
    projects_root: &Path,
    session_id: &str,
) -> Option<SessionMetadata> {
    if !crate::session_evidence::valid_session_id(session_id) || !is_session_stem(session_id) {
        return None;
    }
    let file_name = format!("{session_id}.jsonl");
    std::fs::read_dir(projects_root)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
        .find_map(|e| {
            let dir_path = e.path();
            let path = dir_path.join(&file_name);
            path.is_file()
                .then(|| session_metadata(session_id, &path, &read_sessions_index(&dir_path)))
        })
}

fn read_sessions_index(dir_path: &Path) -> SessionsIndex {
    std::fs::read_to_string(dir_path.join("sessions-index.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| {
            v.get("entries").and_then(|e| e.as_array()).map(|arr| {
                arr.iter()
                    .filter_map(|it| {
                        it.get("sessionId")
                            .and_then(|s| s.as_str())
                            .map(|sid| (sid.to_string(), it.clone()))
                    })
                    .collect()
            })
        })
        .unwrap_or_default()
}

fn session_metadata(stem: &str, file_path: &Path, index: &SessionsIndex) -> SessionMetadata {
    let meta = std::fs::metadata(file_path).ok();
    let entry_meta = index.get(stem);
    let index_field = |field: &str| {
        entry_meta
            .and_then(|v| v.get(field).and_then(|s| s.as_str()))
            .map(String::from)
    };
    SessionMetadata {
        session_id: stem.to_string(),
        provider: "claude".to_string(),
        project_path: read_session_cwd(file_path),
        title: index_field("title"),
        summary: index_field("summary"),
        file_path: file_path.to_string_lossy().to_string(),
        transcript_path: crate::session_evidence::stored_transcript_path(stem)
            .map(|p| p.to_string_lossy().to_string()),
        created_ts: meta
            .as_ref()
            .and_then(|m| provider::fs_time_to_unix(m.created().ok())),
        modified_ts: meta
            .as_ref()
            .and_then(|m| provider::fs_time_to_unix(m.modified().ok())),
    }
}

pub(super) fn parse_session_messages(file_path: &Path) -> Vec<SessionMessage> {
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
        let msg_type = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if msg_type != "user" && msg_type != "assistant" {
            continue;
        }
        let is_meta = v.get("isMeta").and_then(|b| b.as_bool()).unwrap_or(false);

        // Claude JSONL nests the real content under `message.content`, which can a string or an array of typed blocks.
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

        let ts = v
            .get("timestamp")
            .and_then(|t| t.as_str())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.timestamp());

        out.push(SessionMessage {
            role: msg_type.to_string(),
            text,
            ts,
            is_meta,
        });
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
    let recency_limit = max_age.unwrap_or_else(|| Duration::from_secs(300));
    let mut sessions: Vec<(String, std::time::SystemTime)> = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&sessions_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".jsonl") {
                let stem = name.trim_end_matches(".jsonl");
                if stem.len() >= 32 && stem.contains('-') && !exclude.contains(&stem.to_string()) {
                    if let Ok(meta) = entry.metadata() {
                        // Recency check uses modification time — a resumed session has an old birth time but recent writes.
                        if let Ok(modified) = meta.modified() {
                            if let Ok(age) = now.duration_since(modified) {
                                if age > recency_limit {
                                    continue;
                                }
                            }
                        }
                        // Sort key uses creation time (birth) when available — a new session creates a new file, so the newest-created file is the current session.
                        // Falls back to modified time (Linux without birth time, or resumed sessions).
                        let sort_key = meta.created().or_else(|_| meta.modified());
                        if let Ok(ts) = sort_key {
                            sessions.push((stem.to_string(), ts));
                        }
                    }
                }
            }
        }
    }

    match sessions.as_slice() {
        [(id, _)] => Some(id.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::AgentProvider;

    #[test]
    fn find_session_under_returns_the_enumerated_record_for_exactly_that_id() {
        let tmp = tempfile::tempdir().unwrap();
        let mut env = codeterm_util::test_env::EnvGuard::lock();
        env.set(
            crate::session_evidence::EVIDENCE_DIR_ENV,
            tmp.path().join("store"),
        );
        let root = tmp.path().join("projects");
        let owned = "cccc3333-find-4ccc-8ccc-cccccccccccc";
        let sibling = "dddd4444-find-4ddd-8ddd-dddddddddddd";
        let other_dir = root.join("-other-project");
        let owned_dir = root.join("-owned-project");
        std::fs::create_dir_all(&other_dir).unwrap();
        std::fs::create_dir_all(&owned_dir).unwrap();
        std::fs::write(
            other_dir.join(format!("{sibling}.jsonl")),
            "{\"cwd\":\"/other\"}\n",
        )
        .unwrap();
        std::fs::write(
            owned_dir.join(format!("{owned}.jsonl")),
            "{\"cwd\":\"/owned\"}\n",
        )
        .unwrap();
        std::fs::write(
            owned_dir.join("sessions-index.json"),
            format!(r#"{{"entries":[{{"sessionId":"{owned}","title":"Owned","summary":"S"}}]}}"#),
        )
        .unwrap();

        let found = find_session_under(&root, owned).expect("owned session resolves by id");
        let enumerated = enumerate_sessions_under(&root)
            .into_iter()
            .find(|s| s.session_id == owned)
            .expect("fixture session is enumerated");
        assert_eq!(
            serde_json::to_value(&found).unwrap(),
            serde_json::to_value(&enumerated).unwrap()
        );
        assert_eq!(found.project_path.as_deref(), Some("/owned"));
        assert_eq!(found.title.as_deref(), Some("Owned"));

        assert!(find_session_under(&root, "eeee5555-none-4eee-8eee-eeeeeeeeeeee").is_none());
        assert!(find_session_under(
            &root,
            "../-owned-project/cccc3333-find-4ccc-8ccc-cccccccccccc"
        )
        .is_none());
        assert!(find_session_under(&root, "").is_none());
    }

    #[test]
    fn test_encode_cwd() {
        assert_eq!(encode_cwd("/home/user/project"), "-home-user-project");
        assert_eq!(encode_cwd("C:\\Users\\test"), "C--Users-test");
    }

    #[test]
    fn encode_cwd_matches_real_claude_project_dirs() {
        let cases = [
            ("/Users/user", "-Users-user"),
            // /Users/user/.codeterm-dev/general : the '/' before '.codeterm' AND the '.'
            // each become a dash -> the `--codeterm` double-dash. Real dir on disk.
            (
                "/Users/user/.codeterm-dev/general",
                "-Users-user--codeterm-dev-general",
            ),
            (
                "/Users/user/Developer/wk-cb-p1",
                "-Users-user-Developer-wk-cb-p1",
            ),
            // Windows native path: drive colon + backslashes all collapse to dashes.
            ("C:\\Users\\User\\proj", "C--Users-User-proj"),
            // encoder is separator-agnostic, so a normalized cwd binds the same dir.
            ("C:/Users/User/proj", "C--Users-User-proj"),
        ];
        for (cwd, want) in cases {
            assert_eq!(encode_cwd(cwd), want, "encode_cwd({cwd:?})");
        }
    }

    #[test]
    fn detect_session_id_refuses_to_guess_when_ambiguous_in_shared_cwd() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path().to_string_lossy().to_string();
        let s1 = "aaaa1111-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
        let s2 = "bbbb2222-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
        let Some(p1) = session_file_path(&cwd, s1) else {
            return;
        };
        let Some(dir) = p1.parent().map(|d| d.to_path_buf()) else {
            return;
        };
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let p2 = dir.join(format!("{s2}.jsonl"));
        if std::fs::write(&p1, "{}\n").is_err() || std::fs::write(&p2, "{}\n").is_err() {
            return;
        }

        // Two recent sibling sessions, nothing excluded → ambiguous → no guess.
        assert_eq!(
            detect_session_id(&cwd, &[], Some(Duration::from_secs(300))),
            None,
            "must refuse to guess when 2+ sibling sessions are viable candidates",
        );

        // Excluding the sibling disambiguates → the unique candidate binds.
        let got = detect_session_id(&cwd, &[s2.to_string()], Some(Duration::from_secs(300)));
        assert_eq!(
            got.as_deref(),
            Some(s1),
            "with the sibling excluded there is exactly one candidate → bind it",
        );

        let _ = std::fs::remove_file(&p1);
        let _ = std::fs::remove_file(&p2);
    }

    #[test]
    fn detect_active_session_never_grabs_a_sibling_session_in_same_cwd() {
        let p = super::super::ClaudeProvider::new();
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path().to_string_lossy().to_string();
        let my_sid = "mine-0000-0000-0000-000000000000"; // tab's own id (no file yet)
        let sibling_sid = "sibl-1111-1111-1111-111111111111";
        let Some(sib_path) = p.session_file_path(&cwd, sibling_sid) else {
            return;
        };
        let Some(dir) = sib_path.parent().map(|d| d.to_path_buf()) else {
            return;
        };
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        // Materialize ONLY the sibling's session in this shared cwd.
        if std::fs::write(&sib_path, "{\"cwd\":\"x\"}\n").is_err() {
            return;
        }
        assert_eq!(
            p.detect_active_session(&cwd, my_sid),
            None,
            "must never roll a tab onto a sibling's session (no newest-for-cwd)",
        );
        let _ = std::fs::remove_file(&sib_path);
    }

    #[test]
    fn test_verbatim_cwd_encodes_like_plain_cwd() {
        // SAME claude project dir as the plain cwd — otherwise resume never matches.
        let plain = encode_cwd(r"C:\Users\User\.codeterm-dev\general");
        let verbatim = encode_cwd(&codeterm_paths::strip_verbatim_prefix(
            r"\\?\C:\Users\User\.codeterm-dev\general",
        ));
        assert_eq!(plain, verbatim);
        assert_eq!(plain, "C--Users-User--codeterm-dev-general");
    }

    #[test]
    fn test_sessions_dir_real_cwd_has_no_verbatim_dashes() {
        let cwd = std::env::temp_dir().to_string_lossy().to_string();
        let dir = sessions_dir(&cwd).unwrap();
        let leaf = dir.file_name().unwrap().to_string_lossy().to_string();
        assert!(
            !leaf.starts_with("----"),
            "verbatim prefix leaked into encoded project dir: {leaf}"
        );
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
    fn session_file_path_prefers_stored_transcript_path_over_reconstruction() {
        let tmp = tempfile::tempdir().unwrap();
        let mut env = codeterm_util::test_env::EnvGuard::lock();
        env.set(
            crate::session_evidence::EVIDENCE_DIR_ENV,
            tmp.path().join("store"),
        );

        // Stored transcript whose filename differs from the session id; the
        // reconstructed <sid>.jsonl is never created — reads must not need it.
        let sid = "aaaa1111-read-pref-4aaa-8aaa-aaaaaaaaaaaa";
        let stored = tmp.path().join("vendor-drifted-uuid.jsonl");
        std::fs::write(&stored, "{\"cwd\":\"x\"}\n").unwrap();
        assert!(crate::session_evidence::record(
            sid,
            stored.to_str().unwrap()
        ));

        let cwd = tmp.path().to_string_lossy().to_string();
        let resolved = session_file_path(&cwd, sid).expect("stored path resolves");
        assert_eq!(resolved, stored, "stored path wins over reconstruction");
        let reconstructed = sessions_dir(&cwd).unwrap().join(format!("{sid}.jsonl"));
        assert!(
            !reconstructed.exists(),
            "fixture layout: the reconstructed path must not exist"
        );
        assert!(
            std::fs::read_to_string(&resolved).is_ok(),
            "the stored file itself is readable through the preferred path"
        );
    }

    #[test]
    fn session_file_path_without_transcript_path_evidence_reconstructs_as_today() {
        let tmp = tempfile::tempdir().unwrap();
        let mut env = codeterm_util::test_env::EnvGuard::lock();
        env.set(
            crate::session_evidence::EVIDENCE_DIR_ENV,
            tmp.path().join("empty-store"),
        );

        let cwd = tmp.path().to_string_lossy().to_string();
        let sid = "bbbb2222-no-evidence-4bbb-8bbb-bbbbbbbbbbbb";
        let resolved = session_file_path(&cwd, sid).expect("reconstruction resolves");
        let reconstructed = sessions_dir(&cwd).unwrap().join(format!("{sid}.jsonl"));
        assert_eq!(
            resolved, reconstructed,
            "absence of evidence keeps today's reconstruction byte-for-byte"
        );
    }

    #[test]
    fn test_session_exists_default_via_file_path() {
        let p = super::super::ClaudeProvider::new();
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path().to_string_lossy().to_string();

        // find it. Skips the test if HOME isn't writable.
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

        assert!(
            p.session_exists(&cwd, "session-real"),
            "existing JSONL should be detected"
        );
        assert!(
            !p.session_exists(&cwd, "session-bogus-uuid-xxx"),
            "missing JSONL must report false"
        );

        let _ = std::fs::remove_file(&real_path);
    }
}
