use std::collections::HashSet;
use std::path::Path;

use crate::html_preview::checked_path;
use codeterm_types::agent::chat_types::ChatMsg;
use codeterm_wire::envelope::{FrameKind, MessageSegment};

pub fn path_key(path: &Path) -> String {
    let value = path.to_string_lossy().replace('\\', "/");
    let key = value.strip_prefix("//?/").unwrap_or(&value).to_string();
    if cfg!(windows) {
        key.to_ascii_lowercase()
    } else {
        key
    }
}

fn insert(paths: &mut HashSet<String>, raw: &str, cwd: Option<&str>) {
    if raw != raw.trim() { return; }
    let path = if raw.to_ascii_lowercase().starts_with("file:") {
        let Ok(url) = url::Url::parse(raw) else {
            return;
        };
        if url.host_str().is_some_and(|host| host != "localhost") {
            return;
        }
        let Ok(path) = url.to_file_path() else {
            return;
        };
        path
    } else {
        let path = Path::new(raw);
        if path.is_absolute() {
            path.to_path_buf()
        } else if let Some(cwd) = cwd {
            Path::new(cwd).join(path)
        } else {
            return;
        }
    };
    if let Ok(path) = checked_path(&path.to_string_lossy()) {
        paths.insert(path_key(&path));
    }
}

fn untrusted_segments(segments: &[MessageSegment]) -> bool {
    segments.iter().any(|part| match part {
        MessageSegment::Frame { kind, segments, .. } => matches!(*kind, FrameKind::Neighbor | FrameKind::SystemPrompt) || untrusted_segments(segments),
        MessageSegment::Paste { .. } | MessageSegment::UnknownFrame { .. } | MessageSegment::PartialFrame { .. } => true,
        _ => false,
    })
}

pub fn referenced_paths(messages: &[ChatMsg], cwd: Option<&str>) -> HashSet<String> {
    let mut paths = HashSet::new();
    for message in messages {
        let trusted = message.inbound_kind.as_deref() != Some("neighbor")
            && !message.segments.as_deref().is_some_and(untrusted_segments);
        let scan = crate::artifact_directive::scan_artifacts(&message.content, trusted);
        for card in scan.cards {
            for path in card.paths { insert(&mut paths, &path, None); }
        }
        for path in scan.attachment_paths { insert(&mut paths, &path, None); }
        for attachment in message.attachments.iter().flatten() {
            if let Some(path) = &attachment.path {
                insert(&mut paths, path, None);
            }
        }
        for records in [&message.tool_edits, &message.file_diffs]
            .into_iter()
            .flatten()
        {
            for record in records.as_array().into_iter().flatten() {
                for field in ["path", "movePath", "move_path"] {
                    if let Some(path) = record.get(field).and_then(|value| value.as_str()) {
                        insert(&mut paths, path, cwd);
                    }
                }
            }
        }
        if let Some(result) = &message.tool_rich_result {
            if result.get("kind").and_then(|value| value.as_str()) == Some("image") {
                if let Some(path) = result.get("path").and_then(|value| value.as_str()) {
                    insert(&mut paths, path, None);
                }
            }
        }
    }
    paths
}
