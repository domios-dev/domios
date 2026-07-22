use crate::client::RpcClient;
use crate::cmd::plan::call_plan_rpc;
use serde_json::{json, Value};

pub fn run(
    client: &mut dyn RpcClient,
    json: bool,
    plan_id: String,
    task_id: String,
    tags: Vec<String>,
    primary: Option<String>,
) -> Result<Option<Value>, String> {
    if tags.is_empty() && primary.is_none() {
        return Err("retag requires at least one --tag or --primary".into());
    }
    let mut params = json!({ "plan_id": plan_id, "task_id": task_id });
    if !tags.is_empty() {
        params["tags"] = json!(tags);
    }
    if let Some(primary) = primary {
        params["primary"] = json!(primary);
    }
    call_plan_rpc(client, "plan.task.update", params)?;
    let plan = call_plan_rpc(client, "plan.get", json!({ "plan_id": plan_id }))?;
    let task = find_task(&plan, &task_id)
        .ok_or_else(|| format!("task '{task_id}' not found after retag"))?;
    let handle = task
        .get("handle")
        .and_then(|v| v.as_str())
        .unwrap_or(task_id.as_str());
    let tag_list = task
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    let payload =
        json!({ "handle": handle, "tags": task.get("tags").cloned().unwrap_or(json!([])) });
    if json {
        return Ok(Some(payload));
    }
    println!("handle: {handle}");
    if !tag_list.is_empty() {
        println!("tags: {tag_list}");
    }
    Ok(None)
}

fn find_task<'a>(plan: &'a Value, selector: &str) -> Option<&'a Value> {
    plan.get("tasks")?.as_array()?.iter().find(|task| {
        task.get("id").and_then(|v| v.as_str()) == Some(selector)
            || task.get("handle").and_then(|v| v.as_str()) == Some(selector)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_task_matches_id_or_handle() {
        let plan = json!({
            "tasks": [
                { "id": "task_1", "handle": "nav-pan-zoom", "tags": ["area:nav"] }
            ]
        });
        assert!(find_task(&plan, "task_1").is_some());
        assert!(find_task(&plan, "nav-pan-zoom").is_some());
        assert!(find_task(&plan, "missing").is_none());
    }
}
