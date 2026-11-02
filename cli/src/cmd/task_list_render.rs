use serde_json::Value;
use std::collections::{HashMap, HashSet};

pub(crate) struct TaskSummaryRow<'a> {
    pub id: &'a str,
    pub key: &'a str,
    pub name: &'a str,
    pub status: &'a str,
    pub plan_id: &'a str,
    pub task: &'a Value,
}

impl<'a> TaskSummaryRow<'a> {
    pub fn new(row: &'a Value) -> Self {
        let task = row.get("task").unwrap_or(row);
        let text = |field: &str| task.get(field).and_then(Value::as_str).unwrap_or("-");
        Self {
            id: text("id"),
            key: row
                .get("display_key")
                .and_then(Value::as_str)
                .or_else(|| task.get("key").and_then(Value::as_str))
                .unwrap_or("-"),
            name: text("name"),
            status: text("status"),
            plan_id: row.get("plan_id").and_then(Value::as_str).unwrap_or("-"),
            task,
        }
    }

    pub fn lane_str(&self, field: &str) -> Option<&'a str> {
        self.task.get("lane")?.get(field)?.as_str()
    }
}

pub(crate) fn task_value(item: &Value) -> &Value {
    TaskSummaryRow::new(item).task
}

pub(crate) fn task_str<'a>(item: &'a Value, field: &str) -> Option<&'a str> {
    task_value(item).get(field).and_then(|v| v.as_str())
}

/// The daemon owns key formatting and the legacy fallback; the CLI only prints it.
pub(crate) fn task_key_label(item: &Value) -> &str {
    TaskSummaryRow::new(item).key
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
    let row = TaskSummaryRow::new(item);
    let TaskSummaryRow {
        id,
        key,
        name,
        status,
        plan_id: plan,
        ..
    } = row;
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
        "{}{id}\t{key}\t{status}\t{plan}\t{name}{suffix}\n",
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
mod tests;
