use serde_json::{json, Value};

fn range(value: &str) -> Option<(u64, u64)> {
    let (start, count) = value.split_once(',').unwrap_or((value, "1"));
    Some((start.parse().ok()?, count.parse().ok()?))
}

fn empty(path: &str) -> Value {
    json!({"path":path,"additions":0,"deletions":0,"hunks":[]})
}

fn finish(file: &mut Value, files: &mut Vec<Value>) {
    let hunks = file["hunks"].as_array().cloned().unwrap_or_default();
    if file["path"].as_str().is_some_and(|path| !path.is_empty())
        && (!hunks.is_empty() || file["binary"] == true || file.get("kind").is_some())
    {
        file["truncated"] = json!(hunks.iter().any(|hunk| {
            let count = |markers: &[char]| {
                hunk["lines"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .filter(|line| line.starts_with(markers))
                    .count() as u64
            };
            Some(count(&[' ', '-'])) != hunk["oldLines"].as_u64()
                || Some(count(&[' ', '+'])) != hunk["newLines"].as_u64()
        }));
        files.push(file.clone());
    }
    *file = empty("");
}

fn patch(path: &str, text: &str) -> Vec<Value> {
    let mut files = Vec::new();
    let mut file = empty(path);
    let mut before: Option<String> = None;
    for line in text.replace("\r\n", "\n").split('\n') {
        let hunk_complete = file["hunks"]
            .as_array()
            .and_then(|hunks| hunks.last())
            .is_none_or(|hunk| {
                let count = hunk["lines"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .filter(|line| line.starts_with([' ', '-']))
                    .count() as u64;
                count >= hunk["oldLines"].as_u64().unwrap_or(0)
            });
        if let Some(paths) = line.strip_prefix("diff --git a/") {
            finish(&mut file, &mut files);
            if let Some((old, new)) = paths.split_once(" b/") {
                file["path"] = json!(old);
                if old != new {
                    file["movePath"] = json!(new);
                    file["kind"] = json!("rename");
                }
            }
        } else if let Some(old) = line.strip_prefix("--- ").filter(|_| hunk_complete) {
            if file["hunks"]
                .as_array()
                .is_some_and(|hunks| !hunks.is_empty())
            {
                finish(&mut file, &mut files);
            }
            before = Some(old.strip_prefix("a/").unwrap_or(old).into());
        } else if let Some(new) = line.strip_prefix("+++ ").filter(|_| before.is_some()) {
            let old = before.take().unwrap_or_default();
            let new = new.strip_prefix("b/").unwrap_or(new);
            let kind = if old == "/dev/null" {
                "add"
            } else if new == "/dev/null" {
                "delete"
            } else if old != new {
                "rename"
            } else {
                "modify"
            };
            file["path"] = json!(if new == "/dev/null" || kind == "rename" {
                &old
            } else {
                new
            });
            file["kind"] = json!(kind);
            if kind == "rename" {
                file["movePath"] = json!(new);
            }
        } else if line.starts_with("@@ ") {
            let mut parts = line.split_whitespace().skip(1);
            let old = parts
                .next()
                .and_then(|part| part.strip_prefix('-'))
                .and_then(range);
            let new = parts
                .next()
                .and_then(|part| part.strip_prefix('+'))
                .and_then(range);
            if let (Some((old_start, old_lines)), Some((new_start, new_lines))) = (old, new) {
                if let Some(hunks) = file["hunks"].as_array_mut() {
                    hunks.push(json!({"oldStart":old_start,"oldLines":old_lines,
                        "newStart":new_start,"newLines":new_lines,"lines":[]}));
                }
            }
        } else if line.starts_with("Binary files") || line.starts_with("GIT binary patch") {
            file["binary"] = json!(true);
        } else if line.starts_with([' ', '+', '-', '\\']) {
            if let Some(lines) = file["hunks"]
                .as_array_mut()
                .and_then(|hunks| hunks.last_mut())
                .and_then(|hunk| hunk["lines"].as_array_mut())
            {
                lines.push(json!(line));
                if line.starts_with('+') {
                    file["additions"] = json!(file["additions"].as_u64().unwrap_or(0) + 1);
                }
                if line.starts_with('-') {
                    file["deletions"] = json!(file["deletions"].as_u64().unwrap_or(0) + 1);
                }
            }
        }
    }
    finish(&mut file, &mut files);
    if files.len() == 1 && !path.is_empty() && files[0]["kind"] != "rename" {
        files[0]["path"] = json!(path);
    }
    files
}

pub(super) fn from_tool(path: &str, edits: &[Value], metadata: &Value) -> Vec<Value> {
    if let Some(files) = metadata["files"].as_array() {
        return files
            .iter()
            .flat_map(|item| {
                let path = item["filePath"].as_str().unwrap_or_default();
                from_tool(path, &[], &json!({"diff":item["diff"]}))
                    .into_iter()
                    .map(|mut diff| {
                        if matches!(item["type"].as_str(), Some("add" | "delete")) {
                            diff["kind"] = item["type"].clone();
                        }
                        if item["movePath"].is_string() {
                            diff["movePath"] = item["movePath"].clone();
                            diff["kind"] = json!("rename");
                        }
                        diff
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
    }
    let text = metadata["diff"]
        .as_str()
        .or_else(|| metadata["patch"].as_str())
        .unwrap_or_default();
    let diffs = patch(path, text);
    if !diffs.is_empty() {
        return diffs;
    }
    edits.iter().filter_map(|edit| {
        let path = edit["path"].as_str().filter(|path| !path.is_empty())?;
        let old = edit["old"].as_str().unwrap_or_default();
        let new = edit["new"].as_str().unwrap_or_default();
        if old == new { return None; }
        let lines = |text: &str, prefix: char| -> Vec<String> {
            if text.is_empty() { return Vec::new(); }
            let normalized = text.replace("\r\n", "\n");
            normalized.strip_suffix('\n').unwrap_or(&normalized).split('\n')
                .map(|line| format!("{prefix}{line}")).collect()
        };
        let old = lines(old, '-'); let new = lines(new, '+');
        let mut combined = old.clone(); combined.extend(new.clone());
        Some(json!({"path":path,"additions":new.len(),"deletions":old.len(),"hunks":[{
            "oldStart":1,"oldLines":old.len(),"newStart":1,"newLines":new.len(),"lines":combined}]}))
    }).collect()
}
