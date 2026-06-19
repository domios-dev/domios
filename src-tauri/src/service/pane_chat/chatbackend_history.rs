//! Durable host-side history for chatBackend (shell) panes.
//!
//! A chatBackend pane's transcript otherwise lives ONLY in the plugin VM's
//! in-memory sessions Map, so a refresh / VM reload / daemon restart loses it.
//! We persist it as JSONL — one line per message, UPSERT by id — under
//! `~/.codeterm[-dev]/chatbackend-sessions/<plugin_id>/<session_id>.jsonl`.
//!
//! A streaming assistant message grows in place across polls (same id), so we
//! key by id and rewrite the growing message's line instead of appending N
//! duplicates: the file holds exactly one line per message.
//!
//! The optional first line `{"_meta": <openSessionConfig>}` carries the
//! systemPrompt / preset / model so a pane restored after a daemon restart can
//! lazily re-open its plugin session and continue the conversation.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::agent::chat_types::ChatMsg;

const SESSIONS_SUBDIR: &str = "chatbackend-sessions";
const META_KEY: &str = "_meta";

/// Filesystem-safe path component: keep `[A-Za-z0-9._-]`, map anything else to
/// `_` so a plugin-chosen id with slashes/colons can't escape the sessions dir.
fn sanitize(component: &str) -> String {
    component
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// `~/.codeterm[-dev]/chatbackend-sessions/<plugin_id>/`.
pub fn session_dir(plugin_id: &str) -> PathBuf {
    crate::paths::data_dir()
        .join(SESSIONS_SUBDIR)
        .join(sanitize(plugin_id))
}

/// JSONL path for one session.
pub fn session_path(plugin_id: &str, session_id: &str) -> PathBuf {
    session_dir(plugin_id).join(format!("{}.jsonl", sanitize(session_id)))
}

/// Read the persisted transcript (durable truth), skipping the `_meta` header.
/// Missing / unreadable file ⇒ empty. Unparseable lines (e.g. a half-written
/// final line after a crash) are skipped, never fatal.
pub fn read_messages(path: &Path) -> Vec<ChatMsg> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    content
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || is_meta_line(line) {
                return None;
            }
            serde_json::from_str::<ChatMsg>(line).ok()
        })
        .collect()
}

/// Read the persisted openSession config from the `_meta` header line.
pub fn read_meta(path: &Path) -> Option<serde_json::Value> {
    let content = std::fs::read_to_string(path).ok()?;
    let first = content.lines().next()?.trim();
    let v: serde_json::Value = serde_json::from_str(first).ok()?;
    v.get(META_KEY).cloned()
}

fn is_meta_line(line: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(line)
        .ok()
        .and_then(|v| v.get(META_KEY).cloned())
        .is_some()
}

/// Move a session's JSONL from one id to another (best-effort). Used when a
/// lazily re-opened session is assigned a fresh id by the plugin so history
/// stays continuous. No-op if ids match, source is missing, or dest exists.
pub fn migrate(plugin_id: &str, from_session: &str, to_session: &str) {
    if from_session == to_session {
        return;
    }
    let from = session_path(plugin_id, from_session);
    let to = session_path(plugin_id, to_session);
    if from.exists() && !to.exists() {
        if let Some(parent) = to.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::rename(&from, &to);
    }
}

/// In-memory mirror of one session's JSONL. UPSERTs messages by id and rewrites
/// the file only when content actually changed (the poll-loop dedup means this
/// is rare). Best-effort, no fsync on the hot path.
pub struct ChatBackendHistory {
    path: PathBuf,
    meta: Option<serde_json::Value>,
    order: Vec<String>,
    by_id: HashMap<String, ChatMsg>,
}

impl ChatBackendHistory {
    pub fn open(plugin_id: &str, session_id: &str) -> Self {
        Self::open_at(session_path(plugin_id, session_id))
    }

    pub fn open_at(path: PathBuf) -> Self {
        let meta = read_meta(&path);
        let mut order = Vec::new();
        let mut by_id = HashMap::new();
        for m in read_messages(&path) {
            if !by_id.contains_key(&m.id) {
                order.push(m.id.clone());
            }
            by_id.insert(m.id.clone(), m);
        }
        Self {
            path,
            meta,
            order,
            by_id,
        }
    }

    /// Seed the openSession config as the `_meta` header if not already present.
    pub fn ensure_meta(&mut self, config: &serde_json::Value) {
        if self.meta.is_none() && !config.is_null() {
            self.meta = Some(config.clone());
            self.flush();
        }
    }

    /// UPSERT messages by id. Returns whether anything changed (and was flushed).
    pub fn upsert(&mut self, messages: &[ChatMsg]) -> bool {
        let mut changed = false;
        for m in messages {
            match self.by_id.get(&m.id) {
                Some(existing) if existing == m => continue,
                Some(_) => {}
                None => self.order.push(m.id.clone()),
            }
            self.by_id.insert(m.id.clone(), m.clone());
            changed = true;
        }
        if changed {
            self.flush();
        }
        changed
    }

    fn flush(&self) {
        let mut out = String::new();
        if let Some(meta) = &self.meta {
            if let Ok(line) = serde_json::to_string(&serde_json::json!({ META_KEY: meta })) {
                out.push_str(&line);
                out.push('\n');
            }
        }
        for id in &self.order {
            if let Some(m) = self.by_id.get(id) {
                if let Ok(line) = serde_json::to_string(m) {
                    out.push_str(&line);
                    out.push('\n');
                }
            }
        }
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&self.path, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn msg(id: &str, content: &str) -> ChatMsg {
        ChatMsg {
            id: id.into(),
            kind: "assistant".into(),
            content: content.into(),
            ..Default::default()
        }
    }

    #[test]
    fn writing_a_message_lands_in_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.jsonl");
        let mut h = ChatBackendHistory::open_at(path.clone());
        assert!(h.upsert(&[msg("1", "hello")]), "first write changes file");

        let read = read_messages(&path);
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].id, "1");
        assert_eq!(read[0].content, "hello");
    }

    #[test]
    fn streaming_message_yields_one_final_line_not_n() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.jsonl");
        let mut h = ChatBackendHistory::open_at(path.clone());
        // Same id grows across ticks (streaming tokens).
        h.upsert(&[msg("live", "He")]);
        h.upsert(&[msg("live", "Hello")]);
        h.upsert(&[msg("live", "Hello world")]);

        // Exactly ONE physical line for the streaming message, holding the
        // final content — not three appended snapshots.
        let raw = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = raw.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(lines.len(), 1, "one line per message id, not per tick");
        let read = read_messages(&path);
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].content, "Hello world");
    }

    #[test]
    fn unchanged_upsert_does_not_rewrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.jsonl");
        let mut h = ChatBackendHistory::open_at(path);
        assert!(h.upsert(&[msg("1", "a")]));
        assert!(
            !h.upsert(&[msg("1", "a")]),
            "byte-identical upsert is a no-op"
        );
    }

    #[test]
    fn multiple_messages_keep_insertion_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.jsonl");
        let mut h = ChatBackendHistory::open_at(path.clone());
        h.upsert(&[msg("a", "first"), msg("b", "second")]);
        h.upsert(&[msg("a", "first-edited")]);
        h.upsert(&[msg("c", "third")]);

        let read = read_messages(&path);
        let ids: Vec<&str> = read.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["a", "b", "c"], "stable order, edit in place");
        assert_eq!(read[0].content, "first-edited");
    }

    #[test]
    fn reopen_loads_existing_transcript() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.jsonl");
        {
            let mut h = ChatBackendHistory::open_at(path.clone());
            h.upsert(&[msg("1", "a"), msg("2", "b")]);
        }
        // Fresh instance must see prior content and append, not clobber.
        let mut h2 = ChatBackendHistory::open_at(path.clone());
        h2.upsert(&[msg("3", "c")]);
        let read = read_messages(&path);
        assert_eq!(read.len(), 3);
    }

    #[test]
    fn meta_header_is_first_line_and_excluded_from_messages() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.jsonl");
        let mut h = ChatBackendHistory::open_at(path.clone());
        h.ensure_meta(&json!({ "systemPrompt": "You are X", "preset": "p" }));
        h.upsert(&[msg("1", "hi")]);

        let read = read_messages(&path);
        assert_eq!(read.len(), 1, "meta line is not a message");
        let meta = read_meta(&path).expect("meta persisted");
        assert_eq!(meta["systemPrompt"], "You are X");
        assert_eq!(meta["preset"], "p");

        // First physical line is the meta header.
        let raw = std::fs::read_to_string(&path).unwrap();
        let first = raw.lines().next().unwrap();
        assert!(first.contains("_meta"));
    }

    #[test]
    fn ensure_meta_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.jsonl");
        let mut h = ChatBackendHistory::open_at(path.clone());
        h.ensure_meta(&json!({ "preset": "first" }));
        h.ensure_meta(&json!({ "preset": "second" }));
        let meta = read_meta(&path).unwrap();
        assert_eq!(meta["preset"], "first", "first meta wins, not overwritten");
    }

    #[test]
    fn sanitize_blocks_path_escape() {
        let p = session_path("plug/in", "../../etc/passwd");
        // Separators are mapped to `_`, so no component can climb out of the
        // sessions dir — every component sits directly under it.
        assert!(
            p.starts_with(crate::paths::data_dir().join(SESSIONS_SUBDIR)),
            "stays under sessions dir: {}",
            p.display()
        );
        assert!(
            p.components()
                .all(|c| c.as_os_str() != std::ffi::OsStr::new("..")),
            "no parent-dir component: {}",
            p.display()
        );
        assert!(p.to_string_lossy().ends_with(".jsonl"));
    }

    #[test]
    fn migrate_renames_session_file() {
        // Hermetic: drive open_at directly on a temp path, then exercise the
        // rename helper on data_dir-rooted paths through a unique plugin id.
        let plugin = "test-migrate-plugin-unique";
        let from = session_path(plugin, "old-sid");
        let to = session_path(plugin, "new-sid");
        let _ = std::fs::remove_file(&from);
        let _ = std::fs::remove_file(&to);

        let mut h = ChatBackendHistory::open_at(from.clone());
        h.upsert(&[msg("1", "carry me")]);
        assert!(from.exists());

        migrate(plugin, "old-sid", "new-sid");
        assert!(!from.exists(), "source moved");
        assert!(to.exists(), "dest created");
        assert_eq!(read_messages(&to)[0].content, "carry me");

        let _ = std::fs::remove_file(&to);
    }

    #[test]
    fn unparseable_trailing_line_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.jsonl");
        std::fs::write(
            &path,
            "{\"id\":\"1\",\"type\":\"assistant\",\"content\":\"ok\"}\n{half-written",
        )
        .unwrap();
        let read = read_messages(&path);
        assert_eq!(read.len(), 1, "corrupt last line skipped, not fatal");
        assert_eq!(read[0].content, "ok");
    }
}
