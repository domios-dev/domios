use serde::Serialize;
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};

static COLLECTION: AtomicBool = AtomicBool::new(false);

static VERBOSE: AtomicBool = AtomicBool::new(false);

pub fn set_verbose(verbose: bool) {
    VERBOSE.store(verbose, Ordering::Relaxed);
}

pub fn verbose() -> bool {
    VERBOSE.load(Ordering::Relaxed)
}

const COLLECTION_KEYS: &[&str] = &[
    "hits",
    "results",
    "tasks",
    "plans",
    "tabs",
    "blueprints",
    "items",
    "skills",
    "sessions",
    "workspaces",
    "teams",
    "backups",
    "providers",
    "models",
    "services",
    "flows",
    "history",
    "entries",
    "jobs",
    "offers",
    "data",
    "docs",
    "matches",
    "notifications",
    "plugins",
    "channels",
    "messages",
    "themes",
    "endpoints",
    "presets",
    "kinds",
    "areas",
];

pub fn configure(args: &[String]) {
    use clap::CommandFactory;
    if let Ok(matches) = crate::Cli::command().try_get_matches_from(args) {
        let mut cursor = &matches;
        let mut leaf = "";
        while let Some((name, child)) = cursor.subcommand() {
            leaf = name;
            cursor = child;
        }
        COLLECTION.store(
            matches!(
                leaf,
                "list"
                    | "search"
                    | "providers"
                    | "models"
                    | "recent"
                    | "history"
                    | "tags"
                    | "offers"
            ),
            Ordering::Relaxed,
        );
    }
}

pub fn is_collection() -> bool {
    COLLECTION.load(Ordering::Relaxed)
}

#[cfg(test)]
pub(crate) fn collection_envelope(value: &Value) -> Value {
    envelope(value, true)
}

pub fn json_envelope(value: &Value) -> Value {
    envelope(value, COLLECTION.load(Ordering::Relaxed))
}

fn envelope(value: &Value, collection: bool) -> Value {
    if value.get("ok").and_then(Value::as_bool) == Some(false) {
        let mut error = value.clone();
        if let Some(object) = error.as_object_mut() {
            let payload = object
                .remove("data")
                .or_else(|| object.remove("result"))
                .unwrap_or(Value::Null);
            object.insert("result".into(), payload);
        }
        return error;
    }
    let mut payload = value.clone();
    let mut envelope = serde_json::Map::new();
    if let Some(object) = payload.as_object_mut() {
        for key in ["next", "warnings"] {
            if key == "next" && !collection {
                continue;
            }
            if let Some(value) = object.remove(key) {
                envelope.insert(key.into(), value);
            }
        }
        let checklist_rows = if collection {
            object
                .get_mut("checklist")
                .and_then(Value::as_object_mut)
                .and_then(|checklist| checklist.remove("items"))
        } else {
            None
        };
        if let Some(rows) = checklist_rows {
            envelope.insert("meta".into(), Value::Object(std::mem::take(object)));
            payload = rows;
        } else if let Some(key) = collection
            .then(|| {
                COLLECTION_KEYS
                    .iter()
                    .find(|key| object.get(**key).is_some_and(Value::is_array))
            })
            .flatten()
        {
            let rows = object.remove(*key).unwrap_or(Value::Null);
            if !object.is_empty() {
                envelope.insert("meta".into(), Value::Object(std::mem::take(object)));
            }
            payload = rows;
        }
    }
    envelope.insert(
        "result".into(),
        crate::cmd::print::uniform_id_value(&payload),
    );
    Value::Object(envelope)
}

pub fn encode<T: Serialize + ?Sized>(value: &T) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(&json_envelope(&serde_json::to_value(value)?))
}

fn field<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(Value::as_str)
            .filter(|text| !text.trim().is_empty())
    })
}

fn single_line(text: &str, limit: usize) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = normalized.chars();
    let mut out: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        out.push('…');
    }
    out
}

pub fn has_collection(value: &Value) -> bool {
    envelope(value, true)["result"].is_array()
}

pub fn compact_row(row: &Value, current_tab: Option<&str>) -> String {
    if let Some(text) = row.as_str() {
        let text = single_line(text, 160);
        return format!("{text}  {text} — \n");
    }
    let item = row.get("item").or_else(|| row.get("meta")).unwrap_or(row);
    let id = field(
        item,
        &[
            "id",
            "task_id",
            "plan_id",
            "tab_id",
            "session_id",
            "workspace_id",
            "unit_id",
            "name",
        ],
    )
    .or_else(|| field(row, &["session_id", "pointer", "doc"]))
    .unwrap_or("?");
    let title = field(item, &["title", "name", "display_name", "section"])
        .or_else(|| field(row, &["section", "title", "doc"]))
        .unwrap_or("untitled");
    let snippet = field(
        item,
        &[
            "summary",
            "description",
            "body",
            "excerpt",
            "goal",
            "status",
            "state",
        ],
    )
    .or_else(|| field(row, &["excerpt", "pointer"]))
    .or_else(|| row.pointer("/snippets/0/text").and_then(Value::as_str))
    .unwrap_or("");
    let marker = if current_tab == Some(id) {
        " (self)"
    } else {
        ""
    };
    format!(
        "{}  {}{marker} — {}\n",
        single_line(id, 160),
        single_line(title, 160),
        single_line(snippet, 80)
    )
}

pub fn compact_list(value: &Value, current_tab: Option<&str>) -> String {
    let envelope = envelope(value, true);
    let Some(rows) = envelope["result"].as_array() else {
        return String::new();
    };
    let mut out: String = rows
        .iter()
        .map(|row| compact_row(row, current_tab))
        .collect();
    let total = envelope.pointer("/meta/total").and_then(Value::as_u64);
    let more = envelope
        .pointer("/meta/more")
        .and_then(Value::as_u64)
        .or_else(|| total.map(|total| total.saturating_sub(rows.len() as u64)))
        .unwrap_or(0);
    if more > 0 {
        out.push_str(&format!("{more} more — refine the query or use --limit\n"));
    }
    out
}

#[cfg(test)]
mod tests;
