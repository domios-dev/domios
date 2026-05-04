pub fn print_self_update_outcome(body: &serde_json::Value) {
    match body.get("status").and_then(|v| v.as_str()) {
        Some("up_to_date") => {
            let v = body.get("version").and_then(|v| v.as_str()).unwrap_or("?");
            println!("Already up to date (v{}).", v);
        }
        Some("scheduled") => {
            let old = body.get("old_version").and_then(|v| v.as_str()).unwrap_or("?");
            let new = body.get("new_version").and_then(|v| v.as_str()).unwrap_or("?");
            let log = body.get("log_path").and_then(|v| v.as_str()).unwrap_or("?");
            let script = body.get("script_path").and_then(|v| v.as_str()).unwrap_or("?");
            println!("self-update scheduled: v{} -> v{}", old, new);
            println!("daemon will exit shortly; orchestrator runs in the background.");
            println!("script: {}", script);
            println!("log:    {}", log);
        }
        Some("unsupported") => {
            let r = body.get("reason").and_then(|v| v.as_str()).unwrap_or("");
            println!("self-update unsupported: {}", r);
        }
        _ => println!("{}", body),
    }
}

pub fn handle_result(json: bool, result: Result<serde_json::Value, String>) {
    match result {
        Ok(value) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&value).unwrap());
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
        let changed_at = row.get("changed_at").and_then(|v| v.as_str()).unwrap_or("?");
        // Drop sub-second precision + tz for compact display.
        let ts = changed_at
            .split('.')
            .next()
            .unwrap_or(changed_at)
            .replace('T', " ");
        let changed_by = row.get("changed_by").and_then(|v| v.as_str()).unwrap_or("?");
        let change_type = row
            .get("change_type")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        println!("* {}  {:<15} {}", ts, changed_by, change_type);
        let diff_line = |label: &str, old: Option<&serde_json::Value>, new: Option<&serde_json::Value>| {
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
                Some(format!("    {:<11} {} -> {}", format!("{}:", label), fmt(old), fmt(new)))
            } else if has_new {
                Some(format!("    {:<11} {}", format!("{}:", label), fmt(new)))
            } else {
                Some(format!("    {:<11} (deleted: {})", format!("{}:", label), fmt(old)))
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
