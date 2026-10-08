use serde_json::{json, Value};

pub(super) fn paths(changes: Option<&Value>) -> String {
    changes
        .and_then(Value::as_array)
        .map(|changes| {
            changes
                .iter()
                .filter_map(|change| change["path"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

fn range(value: &str) -> Option<(u64, u64)> {
    let (start, lines) = value.split_once(',').unwrap_or((value, "1"));
    Some((start.parse().ok()?, lines.parse().ok()?))
}

fn hunks(diff: &str, kind: &str) -> Vec<Value> {
    if matches!(kind, "add" | "delete") {
        let added = kind == "add";
        let lines: Vec<_> = diff
            .lines()
            .map(|line| format!("{}{line}", if added { '+' } else { '-' }))
            .collect();
        if lines.is_empty() {
            return Vec::new();
        }
        return vec![
            json!({"oldStart":if added {0} else {1}, "oldLines":if added {0} else {lines.len()},
            "newStart":if added {1} else {0}, "newLines":if added {lines.len()} else {0}, "lines":lines}),
        ];
    }
    let mut hunks: Vec<Value> = Vec::new();
    for line in diff.lines() {
        if line.starts_with("@@ ") {
            let mut parts = line.split_whitespace();
            parts.next();
            let old = parts
                .next()
                .and_then(|s| s.strip_prefix('-'))
                .and_then(range);
            let new = parts
                .next()
                .and_then(|s| s.strip_prefix('+'))
                .and_then(range);
            if let (Some((old_start, old_lines)), Some((new_start, new_lines))) = (old, new) {
                hunks.push(json!({"oldStart":old_start,"oldLines":old_lines,"newStart":new_start,"newLines":new_lines,"lines":[]}));
            }
        } else if let Some(lines) = hunks
            .last_mut()
            .filter(|_| line.starts_with(['+', '-', ' ', '\\']))
            .and_then(|hunk| hunk["lines"].as_array_mut())
        {
            lines.push(json!(line));
        }
    }
    hunks
}

pub(super) fn file_diffs(changes: Option<&Value>) -> Value {
    let diffs: Vec<_> = changes
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|change| {
            let path = change["path"].as_str()?;
            let kind = change["kind"]["type"].as_str()?;
            let diff = change["diff"].as_str()?;
            let binary = diff.lines().any(|line| line.starts_with("Binary files") || line == "GIT binary patch");
            let hunks = if binary { Vec::new() } else { hunks(diff, kind) };
            let count = |marker| {
                hunks
                    .iter()
                    .filter_map(|hunk| hunk["lines"].as_array())
                    .flatten()
                    .filter_map(Value::as_str)
                    .filter(|line| line.starts_with(marker))
                    .count()
            };
            let mut result = json!({"path":path,"kind": if kind == "update" { "modify" } else { kind },
                "additions":count('+'),"deletions":count('-'),"hunks":hunks});
            if let Some(destination) = change["kind"]["move_path"].as_str() {
                result["movePath"] = json!(destination);
                result["kind"] = json!("rename");
            }
            if binary {
                result["binary"] = json!(true);
            }
            Some(result)
        })
        .collect();
    json!(diffs)
}

pub(super) fn edits(diffs: Option<&Value>) -> Value {
    let edits: Vec<_> = diffs
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|diff| {
            diff["hunks"].as_array().into_iter().flatten().map(|hunk| {
                let lines = |marker| {
                    hunk["lines"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .filter_map(|line| line.strip_prefix(marker))
                        .collect::<Vec<_>>()
                        .join("\n")
                };
                json!({"path":diff["path"],"old":lines('-'),"new":lines('+')})
            })
        })
        .collect();
    json!(edits)
}
