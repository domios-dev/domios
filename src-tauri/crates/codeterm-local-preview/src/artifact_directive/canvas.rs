use super::absolute;
use serde_json::{Map, Value};
use std::collections::HashSet;

const BODY_LIMIT: usize = 200_000;
const ITEM_LIMIT: usize = 64;
const FILE_LIMIT: usize = 32;
const VIEW_LIMIT: usize = 16;
const POSITION_LIMIT: f64 = 1_000_000.0;
const SIZE_LIMIT: f64 = 100_000.0;
const VIEW_NAME_BYTES: usize = 80;
const FRAME_FIELDS: &[&str] = &["id", "kind", "x", "y", "w", "h", "title"];

fn content_field(kind: &str) -> Option<&'static str> {
    match kind {
        "image" | "page" | "document" => Some("src"),
        "html" => Some("html"),
        "note" => Some("markdown"),
        _ => None,
    }
}

fn only(record: &Map<String, Value>, allowed: &[&str]) -> bool {
    record.keys().all(|key| allowed.contains(&key.as_str()))
}

fn number(value: Option<&Value>, valid: impl Fn(f64) -> bool) -> bool {
    value.is_some_and(|value| value.as_f64().is_some_and(|n| n.is_finite() && valid(n)))
}

fn position(value: Option<&Value>) -> bool {
    number(value, |n| n.abs() <= POSITION_LIMIT)
}

fn extent(value: Option<&Value>) -> bool {
    number(value, |n| n > 0.0 && n <= SIZE_LIMIT)
}

fn item_id(value: Option<&Value>, index: usize) -> Option<String> {
    match value {
        None => Some(format!("item-{}", index + 1)),
        Some(Value::String(id))
            if (1..=64).contains(&id.len())
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-')) =>
        {
            Some(id.clone())
        }
        Some(_) => None,
    }
}

/// `None` rejects the canvas; `Some(None)` is a valid item without a file.
/// A file item of a canvas, as the board shows it: its shown id, its kind and its path.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CanvasFileItem {
    pub id: String,
    pub kind: String,
    pub src: String,
}

fn item_path(
    value: &Value,
    index: usize,
    ids: &mut HashSet<String>,
) -> Option<Option<CanvasFileItem>> {
    let record = value.as_object()?;
    let kind = record.get("kind")?.as_str()?;
    let content = content_field(kind)?;
    if !record
        .keys()
        .all(|key| FRAME_FIELDS.contains(&key.as_str()) || key == content)
    {
        return None;
    }
    let id = item_id(record.get("id"), index)?;
    if !ids.insert(id.clone()) {
        return None;
    }
    let frame_valid = ["x", "y"]
        .iter()
        .all(|field| record.get(*field).is_none_or(|value| position(Some(value))))
        && ["w", "h"]
            .iter()
            .all(|field| record.get(*field).is_none_or(|value| extent(Some(value))))
        && record.get("title").is_none_or(Value::is_string);
    if !frame_valid {
        return None;
    }
    let body = record.get(content)?.as_str()?;
    if content != "src" {
        return Some(None);
    }
    absolute(body).then(|| {
        Some(CanvasFileItem {
            id,
            kind: kind.to_string(),
            src: body.to_string(),
        })
    })
}

fn valid_view(value: &Value, names: &mut HashSet<String>) -> bool {
    let Some(record) = value.as_object() else {
        return false;
    };
    let Some(name) = record.get("name").and_then(Value::as_str) else {
        return false;
    };
    only(record, &["name", "x", "y", "w", "h"])
        && valid_view_name(name)
        && names.insert(name.to_string())
        && position(record.get("x"))
        && position(record.get("y"))
        && extent(record.get("w"))
        && extent(record.get("h"))
}

/// A `view` name in a canvas file or in a `<ds-canvas src view>` tag.
pub fn valid_view_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= VIEW_NAME_BYTES
}

/// An owner edit may move, resize, rewrite or remove items and add notes, but never adds a file:
/// the files a canvas previews stay those its author referenced.
/// `restorable` holds file items that owner edits removed from this file; undo may put back
/// exactly those, with the same id, kind and path, and nothing else.
pub fn canvas_write_permitted(
    current: &str,
    next: &str,
    restorable: &HashSet<CanvasFileItem>,
) -> Result<(), &'static str> {
    let next = canvas_file_items(next).ok_or("Canvas edit is not a valid canvas")?;
    let current: HashSet<String> = canvas_file_items(current)
        .ok_or("Canvas file is not a valid canvas")?
        .into_iter()
        .map(|item| item.src)
        .collect();
    if next
        .iter()
        .all(|item| current.contains(&item.src) || restorable.contains(item))
    {
        Ok(())
    } else {
        Err("Canvas edits cannot add files")
    }
}

fn sources(items: Vec<CanvasFileItem>) -> Vec<String> {
    items.into_iter().map(|item| item.src).collect()
}

/// The header that opens every canvas file.
pub const CANVAS_FILE_FORMAT: &str = "domios.canvas/1";

fn format_first(text: &str) -> bool {
    let space = |c: char| matches!(c, ' ' | '\t' | '\n' | '\r');
    text.trim_start_matches(space)
        .strip_prefix('{')
        .is_some_and(|rest| rest.trim_start_matches(space).starts_with("\"format\""))
}

fn supported_format(format: &str) -> bool {
    let Some(version) = format.strip_prefix("domios.canvas/") else {
        return false;
    };
    let (major, minor) = version.split_once('.').unwrap_or((version, "0"));
    major == "1" && !minor.is_empty() && minor.bytes().all(|b| b.is_ascii_digit())
}

/// Mirrors `parseDsCanvasFile` in chat-core: a `.canvas.json` opens with `"format"` as its first
/// field, names major version 1, and otherwise holds the board fields.
pub fn canvas_file_paths(text: &str) -> Option<Vec<String>> {
    canvas_file_items(text).map(sources)
}

/// The file items of a canvas file that renders, in item order.
pub fn canvas_file_items(text: &str) -> Option<Vec<CanvasFileItem>> {
    if text.len() > BODY_LIMIT || !format_first(text) {
        return None;
    }
    let mut document: Value = serde_json::from_str(text).ok()?;
    let format = document.as_object_mut()?.remove("format")?;
    supported_format(format.as_str()?)
        .then(|| canvas_value_items(&document))
        .flatten()
}

fn canvas_value_items(document: &Value) -> Option<Vec<CanvasFileItem>> {
    let record = document.as_object()?;
    if !only(record, &["title", "items", "views"])
        || !record.get("title").is_none_or(Value::is_string)
    {
        return None;
    }
    let items = record.get("items")?.as_array()?;
    if items.is_empty() || items.len() > ITEM_LIMIT {
        return None;
    }
    let views = match record.get("views") {
        None => [].as_slice(),
        Some(Value::Array(views)) if views.len() <= VIEW_LIMIT => views.as_slice(),
        Some(_) => return None,
    };
    let mut ids = HashSet::new();
    let mut files = Vec::new();
    for (index, item) in items.iter().enumerate() {
        if let Some(file) = item_path(item, index, &mut ids)? {
            files.push(file);
        }
    }
    let mut names = HashSet::new();
    (files.len() <= FILE_LIMIT && views.iter().all(|view| valid_view(view, &mut names)))
        .then_some(files)
}
