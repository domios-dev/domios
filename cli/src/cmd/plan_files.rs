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
            let exists = entry
                .get("exists")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            out.push_str(&format!("{title}\t{path}"));
            if !exists {
                match entry.get("read_command").and_then(|v| v.as_str()) {
                    Some(cmd) => out.push_str(&format!("\t(not on disk — read: {cmd})")),
                    None => out.push_str("\t(not on disk — body lives in the plan store)"),
                }
            }
            out.push('\n');
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
            "index_path": ".codeterm/plans/active/pl_1/INDEX.md",
            "entries": [
                {
                    "kind": "section",
                    "title": "Spec",
                    "path": ".codeterm/plans/active/pl_1/sections/sec_1.md"
                },
                {
                    "kind": "task",
                    "title": "Build",
                    "path": ".codeterm/plans/active/pl_1/tasks/task_1.md"
                }
            ]
        }));
        assert!(rendered.contains("INDEX.md\t.codeterm/plans/active/pl_1/INDEX.md"));
        assert!(rendered.contains("Spec\t.codeterm/plans/active/pl_1/sections/sec_1.md"));
        assert!(rendered.contains("Build\t.codeterm/plans/active/pl_1/tasks/task_1.md"));
    }

    #[test]
    fn non_existent_task_path_is_annotated_with_read_command() {
        let rendered = render_file_manifest(&json!({
            "index_path": ".codeterm/plans/active/pl_1/INDEX.md",
            "entries": [
                {
                    "kind": "task",
                    "title": "Build",
                    "path": ".codeterm/plans/active/pl_1/tasks/task_1.md",
                    "exists": false,
                    "read_command": "codeterm plan task show pl_1 task_1"
                }
            ]
        }));
        let line = rendered
            .lines()
            .find(|l| l.starts_with("Build\t"))
            .expect("task line");
        assert!(line.contains("not on disk"), "{line}");
        assert!(
            line.contains("codeterm plan task show pl_1 task_1"),
            "{line}"
        );
    }

    #[test]
    fn existing_path_carries_no_annotation() {
        let rendered = render_file_manifest(&json!({
            "entries": [
                {
                    "kind": "task",
                    "title": "Build",
                    "path": ".codeterm/plans/active/pl_1/tasks/task_1.md",
                    "exists": true,
                    "read_command": "codeterm plan task show pl_1 task_1"
                }
            ]
        }));
        assert_eq!(
            rendered.trim_end(),
            "Build\t.codeterm/plans/active/pl_1/tasks/task_1.md"
        );
    }
}
