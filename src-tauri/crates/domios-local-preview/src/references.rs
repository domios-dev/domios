use std::collections::HashSet;
use std::path::Path;

use crate::html_preview::checked_path;
use codeterm_types::agent::chat_types::ChatMsg;

pub(super) fn path_key(path: &Path) -> String {
    let value = path.to_string_lossy().replace('\\', "/");
    value.strip_prefix("//?/").unwrap_or(&value).to_string()
}

fn insert(paths: &mut HashSet<String>, raw: &str, cwd: Option<&str>) {
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
        let path = Path::new(raw.trim());
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

fn directive(line: &str) -> Option<&str> {
    let (tag, value) = line.trim().split_once(char::is_whitespace)?;
    if ![
        "@ds-page",
        "@ds-image",
        "@ds-document",
        "@ct-page",
        "@ct-image",
        "@ct-document",
        "@file",
    ]
    .iter()
    .any(|prefix| tag.eq_ignore_ascii_case(prefix))
    {
        return None;
    }
    let value = value.trim();
    if value.starts_with(['\'', '"']) {
        let quote = value.chars().next()?;
        value.strip_prefix(quote)?.strip_suffix(quote)
    } else {
        Some(value)
    }
}

fn marker_paths(line: &str) -> Vec<String> {
    static TAG: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r#"(?i)<domios\b([^<>]*?)/?>"#).unwrap());
    static ATTRIBUTE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r#"(?i)([a-z]+)="([^"]*)""#).unwrap());
    TAG.captures_iter(line)
        .filter_map(|tag| {
            let attributes: std::collections::HashMap<_, _> = ATTRIBUTE
                .captures_iter(&tag[1])
                .map(|value| (value[1].to_ascii_lowercase(), value[2].to_string()))
                .collect();
            if attributes.get("type")? != "attachment" {
                return None;
            }
            Some(
                attributes
                    .get("path")?
                    .replace("&quot;", "\"")
                    .replace("&lt;", "<")
                    .replace("&gt;", ">")
                    .replace("&amp;", "&"),
            )
        })
        .collect()
}

pub fn referenced_paths(messages: &[ChatMsg], cwd: Option<&str>) -> HashSet<String> {
    let mut paths = HashSet::new();
    for message in messages {
        let mut fence: Option<(char, usize)> = None;
        for line in message.content.lines() {
            let trimmed = line.trim_start();
            if let Some(marker @ ('`' | '~')) = trimmed.chars().next() {
                let count = trimmed.chars().take_while(|c| *c == marker).count();
                if count >= 3 {
                    if fence.is_none() {
                        fence = Some((marker, count));
                    } else if fence.is_some_and(|(open, width)| open == marker && count >= width) {
                        fence = None;
                    }
                    continue;
                }
            }
            if fence.is_some() {
                continue;
            }
            for path in marker_paths(line) {
                insert(&mut paths, &path, None);
            }
            if let Some(path) = directive(line) {
                insert(&mut paths, path, None);
            }
        }
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
