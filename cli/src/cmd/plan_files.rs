use serde_json::Value;

pub(crate) fn render_file_manifest(val: &Value) -> String {
    let mut out = String::new();
    if let Some(index_path) = val.get("index_path").and_then(|v| v.as_str()) {
        out.push_str(&format!("INDEX.md\t{index_path}\n"));
    }
    if let Some(entries) = val.get("entries").and_then(|v| v.as_array()) {
        for entry in entries {
            let title = entry.get("title").and_then(|v| v.as_str()).unwrap_or("?");
            let path = entry.get("path").and_then(|v| v.as_str()).unwrap_or("?");
            out.push_str(&format!("{title}\t{path}\n"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn renders_file_manifest() {
        let rendered = render_file_manifest(&json!({
            "index_path": ".codeterm/plans/pl_1/INDEX.md",
            "entries": [
                {
                    "kind": "section",
                    "title": "Spec",
                    "path": ".codeterm/plans/pl_1/sections/sec_1.md"
                },
                {
                    "kind": "task",
                    "title": "Build",
                    "path": ".codeterm/plans/pl_1/tasks/task_1.md"
                }
            ]
        }));
        assert!(rendered.contains("INDEX.md\t.codeterm/plans/pl_1/INDEX.md"));
        assert!(rendered.contains("Spec\t.codeterm/plans/pl_1/sections/sec_1.md"));
        assert!(rendered.contains("Build\t.codeterm/plans/pl_1/tasks/task_1.md"));
    }
}
