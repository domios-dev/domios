use codeterm_blueprint_types::blueprint_document::Document;
use serde_json::Value;

pub(super) fn summary(result: &Value) -> String {
    let mut lines = vec![format!(
        "{} {} {} [{}]",
        result["action"].as_str().unwrap_or("?"),
        result["name"].as_str().unwrap_or("?"),
        result["id"].as_str().unwrap_or("new"),
        result["state"].as_str().unwrap_or("proposed")
    )];
    let document = |value: &Value| {
        Document::from_definition(value)
            .ok()
            .and_then(|document| serde_json::to_value(document).ok())
            .unwrap_or(Value::Null)
    };
    let before = document(&result["before"]);
    let after = document(&result["after"]);
    diff("", &before, &after, &mut lines);
    lines.join("\n")
}
fn diff(path: &str, before: &Value, after: &Value, lines: &mut Vec<String>) {
    if before == after {
        return;
    }
    if before.is_object() || after.is_object() {
        let keys = before
            .as_object()
            .into_iter()
            .flat_map(|o| o.keys())
            .chain(after.as_object().into_iter().flat_map(|o| o.keys()))
            .collect::<std::collections::BTreeSet<_>>();
        for key in keys {
            let field = if path.is_empty() {
                key.clone()
            } else {
                format!("{path}.{key}")
            };
            diff(&field, &before[key], &after[key], lines)
        }
    } else if before.is_array() || after.is_array() {
        let size = before
            .as_array()
            .map_or(0, Vec::len)
            .max(after.as_array().map_or(0, Vec::len));
        for index in 0..size {
            diff(
                &format!("{path}[{index}]"),
                &before[index],
                &after[index],
                lines,
            )
        }
    } else {
        lines.push(format!("  {path}: {} -> {}", short(before), short(after)))
    }
}
fn short(value: &Value) -> String {
    let text = value.to_string();
    let mut text = text.chars().take(160).collect::<String>();
    if value.to_string().chars().count() > 160 {
        text.push_str("...")
    }
    text
}
