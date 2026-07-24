const ENTITY_ID_FIELDS: &[&str] = &[
    "id",
    "task_id",
    "plan_id",
    "spec_id",
    "section_id",
    "spec_section_id",
    "plan_section_id",
    "element_id",
    "milestone_id",
    "stride_id",
    "loop_id",
    "pane_id",
    "workspace_id",
    "service_id",
    "run_id",
    "attachment_id",
];

pub fn uniform_id_value(value: &serde_json::Value) -> serde_json::Value {
    let mut out = value.clone();
    add_uniform_id_alias(&mut out);
    rename_pane_json_keys(&mut out);
    out
}

pub fn print_json(value: &serde_json::Value) {
    println!(
        "{}",
        serde_json::to_string_pretty(&uniform_id_value(value)).unwrap()
    );
}

fn add_uniform_id_alias(value: &mut serde_json::Value) {
    let Some(obj) = value.as_object_mut() else {
        return;
    };
    if obj.get("id").and_then(|v| v.as_str()).is_some() {
        return;
    }
    let Some(id) = ENTITY_ID_FIELDS.iter().find_map(|field| {
        obj.get(*field)
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    }) else {
        return;
    };
    obj.insert("id".into(), serde_json::Value::String(id));
}

fn rename_pane_json_keys(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Array(items) => {
            for item in items {
                rename_pane_json_keys(item);
            }
        }
        serde_json::Value::Object(object) => {
            for item in object.values_mut() {
                rename_pane_json_keys(item);
            }
            let keys: Vec<String> = object.keys().cloned().collect();
            for key in keys {
                let tab_key = key
                    .replace("pane_ids", "tab_ids")
                    .replace("pane_id", "tab_id")
                    .replace("panes", "tabs")
                    .replace("pane", "tab")
                    .replace("Pane", "Tab");
                if tab_key != key {
                    if let Some(value) = object.remove(&key) {
                        object.insert(tab_key, value);
                    }
                }
            }
        }
        serde_json::Value::String(text) if text == "pane" => *text = "tab".into(),
        serde_json::Value::String(text) if text == "panes" => *text = "tabs".into(),
        _ => {}
    }
}

pub fn handle_result(json: bool, result: Result<serde_json::Value, String>) {
    match result {
        Ok(value) => {
            if json {
                print_json(&value);
            } else {
                print_human(&value);
            }
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

pub fn print_human(value: &serde_json::Value) {
    match value {
        serde_json::Value::Array(arr) => {
            if arr.is_empty() {
                println!("(empty)");
                return;
            }
            for item in arr {
                print_object(item);
                println!();
            }
        }
        serde_json::Value::Object(_) => {
            print_object(value);
        }
        serde_json::Value::String(s) => println!("{}", s),
        serde_json::Value::Null => println!("OK"),
        other => println!("{}", other),
    }
}

pub fn print_object(value: &serde_json::Value) {
    if let Some(obj) = value.as_object() {
        let max_key_len = obj.keys().map(|k| k.len()).max().unwrap_or(0);
        for (key, val) in obj {
            let display_val = match val {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Array(a) => {
                    let items: Vec<String> = a
                        .iter()
                        .map(|v| match v {
                            serde_json::Value::String(s) => s.clone(),
                            other => other.to_string(),
                        })
                        .collect();
                    items.join(", ")
                }
                serde_json::Value::Null => "(none)".to_string(),
                other => other.to_string(),
            };
            println!("  {:width$}  {}", key, display_val, width = max_key_len);
        }
    }
}

pub fn print_history_timeline(rows: &[serde_json::Value]) {
    if rows.is_empty() {
        println!("(no history)");
        return;
    }
    for row in rows {
        let changed_at = row
            .get("changed_at")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        // Drop sub-second precision + tz for compact display.
        let ts = changed_at
            .split('.')
            .next()
            .unwrap_or(changed_at)
            .replace('T', " ");
        let changed_by = row
            .get("changed_by")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let change_type = row
            .get("change_type")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let row_id = row.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
        println!("* [{}] {}  {:<15} {}", row_id, ts, changed_by, change_type);
        let diff_line =
            |label: &str, old: Option<&serde_json::Value>, new: Option<&serde_json::Value>| {
                let has_old = old.map(|v| !v.is_null()).unwrap_or(false);
                let has_new = new.map(|v| !v.is_null()).unwrap_or(false);
                if !has_old && !has_new {
                    return None;
                }
                let fmt = |v: Option<&serde_json::Value>| -> String {
                    match v {
                        None | Some(serde_json::Value::Null) => "-".into(),
                        Some(serde_json::Value::String(s)) => {
                            let truncated: String = s.chars().take(60).collect();
                            if s.chars().count() > 60 {
                                format!("{}…", truncated)
                            } else {
                                truncated
                            }
                        }
                        Some(other) => other.to_string(),
                    }
                };
                if has_old && has_new {
                    Some(format!(
                        "    {:<11} {} -> {}",
                        format!("{}:", label),
                        fmt(old),
                        fmt(new)
                    ))
                } else if has_new {
                    Some(format!("    {:<11} {}", format!("{}:", label), fmt(new)))
                } else {
                    Some(format!(
                        "    {:<11} (deleted: {})",
                        format!("{}:", label),
                        fmt(old)
                    ))
                }
            };
        for (label, old_k, new_k) in [
            ("title", "old_title", "new_title"),
            ("kind", "old_kind", "new_kind"),
            ("importance", "old_importance", "new_importance"),
            ("body", "old_body", "new_body"),
        ] {
            if let Some(line) = diff_line(label, row.get(old_k), row.get(new_k)) {
                println!("{}", line);
            }
        }
        if let Some(reason) = row.get("reason").and_then(|v| v.as_str()) {
            println!("    reason:     {}", reason);
        }
    }
}

pub fn print_taxonomy(value: &serde_json::Value) {
    let Some(obj) = value.as_object() else {
        print_human(value);
        return;
    };
    let mut areas: Vec<&String> = obj.keys().collect();
    // Alphabetical, with "(none)" pushed to the bottom.
    areas.sort_by(|a, b| match (a.as_str(), b.as_str()) {
        ("(none)", "(none)") => std::cmp::Ordering::Equal,
        ("(none)", _) => std::cmp::Ordering::Greater,
        (_, "(none)") => std::cmp::Ordering::Less,
        (x, y) => x.cmp(y),
    });
    if areas.is_empty() {
        println!("(empty)");
        return;
    }
    for area in areas {
        println!("{}", area);
        if let Some(kinds) = obj.get(area).and_then(|v| v.as_object()) {
            let mut kind_keys: Vec<&String> = kinds.keys().collect();
            kind_keys.sort();
            for k in kind_keys {
                let n = kinds.get(k).and_then(|v| v.as_u64()).unwrap_or(0);
                println!("  {:<13} {}", k, n);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn uniform_id_alias_preserves_existing_id() {
        let out = uniform_id_value(&json!({ "id": "canonical", "task_id": "task_1" }));
        assert_eq!(out.get("id").and_then(|v| v.as_str()), Some("canonical"));
    }

    #[test]
    fn uniform_id_alias_adds_top_level_id_for_plan_create() {
        let out = uniform_id_value(&json!({ "plan_id": "plan_1", "title": "Plan" }));
        assert_eq!(out.get("id").and_then(|v| v.as_str()), Some("plan_1"));
        assert_eq!(out.get("plan_id").and_then(|v| v.as_str()), Some("plan_1"));
    }

    #[test]
    fn uniform_id_alias_adds_top_level_id_for_task_add() {
        let out = uniform_id_value(&json!({ "plan_id": "plan_1", "task_id": "task_1" }));
        assert_eq!(out.get("id").and_then(|v| v.as_str()), Some("task_1"));
    }

    #[test]
    fn uniform_id_alias_adds_top_level_id_for_agent_spawn() {
        let out = uniform_id_value(&json!({ "pane_id": "pane_1", "agent": "codex" }));
        assert_eq!(out.get("id").and_then(|v| v.as_str()), Some("pane_1"));
    }

    #[test]
    fn json_output_uses_tab_field_names_recursively() {
        let out = uniform_id_value(&json!({
            "pane_id": "tab_1",
            "parent_pane_id": "tab_0",
            "panes": [{ "createdByPane": "tab_2" }]
        }));
        assert_eq!(out.get("tab_id").and_then(|v| v.as_str()), Some("tab_1"));
        assert_eq!(
            out.get("parent_tab_id").and_then(|v| v.as_str()),
            Some("tab_0")
        );
        assert_eq!(
            out.pointer("/tabs/0/createdByTab").and_then(|v| v.as_str()),
            Some("tab_2")
        );
    }
}
