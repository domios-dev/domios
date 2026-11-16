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
fn item_path(value: &Value, index: usize, ids: &mut HashSet<String>) -> Option<Option<String>> {
    let record = value.as_object()?;
    let kind = record.get("kind")?.as_str()?;
    let content = content_field(kind)?;
    if !record
        .keys()
        .all(|key| FRAME_FIELDS.contains(&key.as_str()) || key == content)
    {
        return None;
    }
    if !ids.insert(item_id(record.get("id"), index)?) {
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
    absolute(body).then(|| Some(body.to_string()))
}

fn valid_view(value: &Value, names: &mut HashSet<String>) -> bool {
    let Some(record) = value.as_object() else {
        return false;
    };
    let Some(name) = record.get("name").and_then(Value::as_str) else {
        return false;
    };
    only(record, &["name", "x", "y", "w", "h"])
        && !name.is_empty()
        && name.len() <= VIEW_NAME_BYTES
        && names.insert(name.to_string())
        && position(record.get("x"))
        && position(record.get("y"))
        && extent(record.get("w"))
        && extent(record.get("h"))
}

/// Mirrors `parseDsCanvas` in chat-core: the file paths of a canvas body that renders.
pub(super) fn canvas_paths(body: &str) -> Option<Vec<String>> {
    if body.len() > BODY_LIMIT {
        return None;
    }
    let document: Value = serde_json::from_str(body).ok()?;
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
    let mut paths = Vec::new();
    for (index, item) in items.iter().enumerate() {
        if let Some(path) = item_path(item, index, &mut ids)? {
            paths.push(path);
        }
    }
    let mut names = HashSet::new();
    (paths.len() <= FILE_LIMIT && views.iter().all(|view| valid_view(view, &mut names)))
        .then_some(paths)
}
