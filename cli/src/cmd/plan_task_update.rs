use crate::client::SocketClient;
use crate::cmd::plan::{call_plan_rpc, resolve_text};
use serde_json::{json, Value};

#[allow(clippy::too_many_arguments)]
pub(crate) fn run(
    client: &mut SocketClient,
    plan_id: String,
    task_id: String,
    name: Option<String>,
    kind: Option<String>,
    model: Option<String>,
    why: Option<String>,
    why_file: Option<String>,
    status: Option<String>,
    status_label: Option<String>,
    body: Option<String>,
    body_file: Option<String>,
    intent: Option<String>,
    intent_file: Option<String>,
    sections: Option<String>,
    criterion: Vec<String>,
    test_plan: Option<String>,
    correctness: Option<String>,
    parent: Option<String>,
    group: Option<String>,
    priority: Option<String>,
    tags: Vec<String>,
    primary: Option<String>,
    handle: Option<String>,
) -> Result<Value, String> {
    let why_text = resolve_text(why, why_file)?;
    let body_text = resolve_text(body, body_file)?;
    let intent_text = resolve_text(intent, intent_file)?;
    let mut params = json!({ "plan_id": plan_id, "task_id": task_id });
    if let Some(n) = name {
        params["name"] = json!(n);
    }
    if let Some(k) = kind {
        params["type"] = json!(k);
    }
    if let Some(m) = model {
        params["model"] = json!(m);
    }
    if let Some(w) = why_text {
        params["why"] = json!(w);
    }
    if let Some(s) = status {
        params["status"] = json!(s);
    }
    if let Some(l) = status_label {
        params["status_label"] = json!(l);
    }
    if let Some(b) = body_text {
        params["body"] = json!(b);
    }
    if let Some(i) = intent_text {
        params["intent"] = json!(i);
    }
    if let Some(s) = sections {
        params["sections"] = json!(s);
    }
    if !criterion.is_empty() {
        params["acceptance_criteria"] = json!(criterion);
    }
    if let Some(t) = test_plan {
        params["test_plan"] = json!(t);
    }
    if let Some(c) = correctness {
        params["correctness"] = json!(c);
    }
    if let Some(p) = parent {
        params["parent_task_id"] = json!(p);
    }
    if let Some(g) = group {
        params["group"] = json!(g);
    }
    if let Some(priority) = priority {
        params["priority"] = json!(priority);
    }
    if !tags.is_empty() {
        params["tags"] = json!(tags);
    }
    if let Some(primary) = primary {
        params["primary"] = json!(primary);
    }
    if let Some(handle) = handle {
        params["handle"] = json!(handle);
    }
    call_plan_rpc(client, "plan.task.update", params)
}
