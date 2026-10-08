use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use base64::Engine;

const INBOX_CAPACITY: usize = 64;
pub(crate) const EMPTY_INBOX: &str = "(no new annotations)";
const SEPARATOR: &str = "\n\n---\n\n";

#[derive(Debug, Clone, PartialEq, Eq)]
struct InboxEntry {
    id: String,
    markdown: String,
    delivered: bool,
    read: bool,
}

/// Annotations kept for `inbox`, the fallback when chat delivery to the owner did not happen.
#[derive(Debug, Default)]
pub(crate) struct AnnotationInbox {
    entries: VecDeque<InboxEntry>,
}

impl AnnotationInbox {
    pub(crate) fn push(&mut self, id: &str, markdown: &str) {
        self.entries.push_back(InboxEntry {
            id: id.to_string(),
            markdown: markdown.to_string(),
            delivered: false,
            read: false,
        });
        while self.entries.len() > INBOX_CAPACITY {
            self.entries.pop_front();
        }
    }

    pub(crate) fn mark_delivered(&mut self, id: &str) {
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) {
            entry.delivered = true;
        }
    }

    /// Unread undelivered annotations (`all`: every kept one), joined; all are read afterwards.
    pub(crate) fn take(&mut self, all: bool) -> String {
        let picked: Vec<&str> = self
            .entries
            .iter()
            .filter(|entry| all || !(entry.delivered || entry.read))
            .map(|entry| entry.markdown.as_str())
            .collect();
        let text = if picked.is_empty() {
            EMPTY_INBOX.to_string()
        } else {
            picked.join(SEPARATOR)
        };
        for entry in &mut self.entries {
            entry.read = true;
        }
        text
    }
}

/// One path component safe on every OS; `fallback` when nothing usable remains.
pub(crate) fn evidence_file_name(raw: &str, fallback: &str) -> String {
    let base = raw.rsplit(['/', '\\']).next().unwrap_or("");
    let name: String = base
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-') {
                c
            } else {
                '_'
            }
        })
        .take(120)
        .collect();
    let name = name.trim_start_matches('.').to_string();
    let name = if name.is_empty() {
        fallback.to_string()
    } else {
        name
    };
    if name.to_ascii_lowercase().ends_with(".png") {
        name
    } else {
        format!("{name}.png")
    }
}

/// Blocking: decode and write one PNG under `dir`.
pub(crate) fn write_evidence(dir: &Path, name: &str, base64: &str) -> Result<PathBuf, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(base64.as_bytes())
        .map_err(|error| format!("screenshot is not base64: {error}"))?;
    std::fs::create_dir_all(dir).map_err(|error| format!("create {}: {error}", dir.display()))?;
    let path = dir.join(name);
    std::fs::write(&path, bytes).map_err(|error| format!("write {}: {error}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_inbox_returns_only_undelivered_unread_annotations_once() {
        let mut inbox = AnnotationInbox::default();
        inbox.push("g1", "first");
        inbox.push("g2", "second");
        inbox.mark_delivered("g1");
        assert_eq!(inbox.take(false), "second");
        assert_eq!(inbox.take(false), EMPTY_INBOX);
        assert_eq!(inbox.take(true), format!("first{SEPARATOR}second"));
    }

    #[test]
    fn browser_inbox_is_bounded() {
        let mut inbox = AnnotationInbox::default();
        for index in 0..(INBOX_CAPACITY + 5) {
            inbox.push(&format!("g{index}"), "x");
        }
        assert_eq!(inbox.entries.len(), INBOX_CAPACITY);
        assert_eq!(inbox.entries.front().unwrap().id, "g5");
    }

    #[test]
    fn browser_evidence_file_name_is_one_safe_png_component() {
        assert_eq!(evidence_file_name("login.png", "shot"), "login.png");
        assert_eq!(evidence_file_name("../../etc/passwd", "shot"), "passwd.png");
        assert_eq!(evidence_file_name("C:\\x\\my shot", "shot"), "my_shot.png");
        assert_eq!(evidence_file_name("", "shot-1"), "shot-1.png");
        assert_eq!(evidence_file_name("..", "shot-1"), "shot-1.png");
    }

    #[test]
    fn browser_evidence_writes_decoded_png_bytes() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("shots");
        let path = write_evidence(&dir, "a.png", "aGVsbG8=").unwrap();
        assert_eq!(std::fs::read(path).unwrap(), b"hello");
        assert!(write_evidence(&dir, "b.png", "***").is_err());
    }
}
