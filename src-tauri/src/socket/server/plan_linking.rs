use serde_json::{json, Value};

use crate::plan::{OrchestratorRef, Plan};

pub(super) fn csv_string_field(params: Option<&Value>, key: &str) -> Vec<String> {
    params
        .and_then(|x| x.get(key))
        .and_then(|v| v.as_str())
        .map(|s| {
            s.split(',')
                .map(str::trim)
                .filter(|x| !x.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn owner_from_params(p: &Value) -> OrchestratorRef {
    p.get("owner")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .or_else(|| {
            p.get("tab_id")
                .and_then(|v| v.as_str())
                .map(OrchestratorRef::from_tab)
        })
        .or_else(|| {
            p.get("caller_tab_id")
                .and_then(|v| v.as_str())
                .map(OrchestratorRef::from_tab)
        })
        .unwrap_or_else(|| OrchestratorRef::from_tab("unknown"))
}

pub(super) fn tags_field(params: Option<&Value>) -> Vec<String> {
    params
        .and_then(|x| x.get("tags"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

pub(super) fn plan_with_linking(plan: &Plan) -> Value {
    let mut value = serde_json::to_value(plan).unwrap_or_default();
    if let Some(obj) = value.as_object_mut() {
        let unlinked_tasks = unlinked_tasks(plan);
        obj.insert(
            "_linking".into(),
            json!({
                "unlinked_count": unlinked_tasks.len(),
                "unlinked_tasks": unlinked_tasks,
            }),
        );
    }
    value
}

fn unlinked_tasks(plan: &Plan) -> Vec<String> {
    plan.tasks
        .iter()
        .filter(|task| task.section_ids.is_empty())
        .map(|task| task.name.clone())
        .collect()
}
