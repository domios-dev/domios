use serde_json::Value;
use std::collections::{HashMap, HashSet};

pub(crate) fn task_value(item: &Value) -> &Value {
    item.get("task").unwrap_or(item)
}

pub(crate) fn task_str<'a>(item: &'a Value, field: &str) -> Option<&'a str> {
    task_value(item).get(field).and_then(|v| v.as_str())
}

fn task_sort_key(item: &Value) -> (&str, &str) {
    (
        task_str(item, "name").unwrap_or(""),
        task_str(item, "id").unwrap_or(""),
    )
}

pub(crate) struct TaskTree<'a> {
    pub roots: Vec<&'a Value>,
    pub children: HashMap<&'a str, Vec<&'a Value>>,
}

pub(crate) fn build_task_tree<'a>(tasks: &[&'a Value]) -> TaskTree<'a> {
    let task_ids: HashSet<&str> = tasks
        .iter()
        .filter_map(|item| task_str(item, "id"))
        .collect();
    let mut children: HashMap<&str, Vec<&Value>> = HashMap::new();
    let mut roots = vec![];
    for item in tasks {
        let parent = task_str(item, "parent_task_id");
        if let Some(parent_id) = parent.filter(|id| task_ids.contains(id)) {
            children.entry(parent_id).or_default().push(*item);
        } else {
            roots.push(*item);
        }
    }
    roots.sort_by_key(|item| task_sort_key(item));
    for items in children.values_mut() {
        items.sort_by_key(|item| task_sort_key(item));
    }
    TaskTree { roots, children }
}

pub(crate) fn render_simple_task_list(tasks: &[Value]) -> String {
    let refs: Vec<&Value> = tasks.iter().collect();
    if refs.is_empty() {
        return "(no tasks)\n".into();
    }
    let tree = build_task_tree(&refs);
    let mut out = String::new();
    for item in tree.roots {
        append_simple_task(&mut out, item, 0, &tree.children);
    }
    out
}

fn append_simple_task(
    out: &mut String,
    item: &Value,
    depth: usize,
    children: &HashMap<&str, Vec<&Value>>,
) {
    let id = task_str(item, "id").unwrap_or("?");
    let name = task_str(item, "name").unwrap_or("?");
    let status = task_str(item, "status").unwrap_or("?");
    let plan = item.get("plan_id").and_then(|v| v.as_str()).unwrap_or("?");
    let external_refs = task_value(item)
        .get("external_refs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("canonical_url").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join(",");
    let suffix = if external_refs.is_empty() {
        String::new()
    } else {
        format!("\t{external_refs}")
    };
    out.push_str(&format!(
        "{}{id}\t{status}\t{plan}\t{name}{suffix}\n",
        "  ".repeat(depth)
    ));
    if let Some(id) = task_str(item, "id") {
        if let Some(kids) = children.get(id) {
            for child in kids {
                append_simple_task(out, child, depth + 1, children);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn simple_task_list_indents_subtasks_under_parent() {
        let rendered = render_simple_task_list(&[
            json!({
                "plan_id": "pl_1",
                "task": { "id": "task_parent", "name": "Parent", "status": "in_progress" }
            }),
            json!({
                "plan_id": "pl_1",
                "task": {
                    "id": "task_child",
                    "name": "Child",
                    "status": "pending",
                    "parent_task_id": "task_parent"
                }
            }),
        ]);
        assert!(rendered.contains("task_parent\tin_progress\tpl_1\tParent\n"));
        assert!(rendered.contains("  task_child\tpending\tpl_1\tChild\n"));
    }

    #[test]
    fn simple_task_list_renders_external_references() {
        let rendered = render_simple_task_list(&[json!({
            "plan_id": "pl_1",
            "task": {
                "id": "task_1",
                "name": "Linked",
                "status": "pending",
                "external_refs": [{
                    "kind": "github_issue",
                    "repository": "rollacode/codeterm-releases",
                    "number": 17,
                    "canonical_url": "https://github.com/rollacode/codeterm-releases/issues/17"
                }]
            }
        })]);
        assert!(rendered.contains("https://github.com/rollacode/codeterm-releases/issues/17"));
    }
}
