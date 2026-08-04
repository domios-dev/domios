use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::client::RpcClient;
use crate::cmd::hints::{maybe_orchestration_reminder, push_hints};
use crate::cmd::plan::call_plan_rpc;

fn split_csv(raw: Option<String>) -> Vec<String> {
    raw.unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}

fn read_step_text(raw: &str) -> Result<(String, String), String> {
    let (kind, text) = raw
        .split_once(':')
        .ok_or_else(|| format!("invalid --step {raw:?}; expected kind:text or kind:@file"))?;
    let text = if let Some(path) = text.strip_prefix('@') {
        std::fs::read_to_string(path).map_err(|e| format!("failed to read {path}: {e}"))?
    } else {
        text.to_string()
    };
    Ok((kind.to_string(), text))
}

fn implicit_key(plan_id: &str, seed: &Value) -> String {
    let mut hasher = DefaultHasher::new();
    plan_id.hash(&mut hasher);
    seed.to_string().hash(&mut hasher);
    std::env::var("CODETERM_TAB_ID")
        .unwrap_or_default()
        .hash(&mut hasher);
    let minute = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() / 60)
        .unwrap_or(0);
    minute.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[allow(clippy::too_many_arguments)]
pub fn record(
    client: &mut dyn RpcClient,
    plan_id: Option<String>,
    task: Option<String>,
    upsert_task: Option<String>,
    name: Option<String>,
    sections: Option<String>,
    status: Option<String>,
    body_file: Option<String>,
    steps: Vec<String>,
    links: Vec<String>,
    key: Option<String>,
) -> Result<Value, String> {
    let task_selector = upsert_task.clone().or(task.clone());
    if task_selector.is_none()
        && (!steps.is_empty() || body_file.is_some() || status.is_some() || sections.is_some())
    {
        return Err("--task or --upsert-task is required for task writes".into());
    }

    let mut ops = Vec::new();
    let section_ids = split_csv(sections.clone());
    if let Some(selector) = task_selector.clone() {
        if upsert_task.is_some() || name.is_some() {
            let mut op = json!({
                "op": "upsert_task",
                "client_id": "task",
                "handle": selector,
            });
            if let Some(name) = name {
                op["name"] = json!(name);
            }
            if !section_ids.is_empty() {
                op["sections"] = json!(section_ids.clone());
            }
            if let Some(status) = status {
                op["status"] = json!(status);
            }
            ops.push(op);
        } else {
            if sections.is_some() {
                ops.push(json!({
                    "op": "set_task_sections",
                    "client_id": "task",
                    "task": selector,
                    "sections": section_ids.clone(),
                }));
            }
            if let Some(status) = status {
                ops.push(json!({
                    "op": "set_task_status",
                    "client_id": "task",
                    "task": selector,
                    "status": status,
                }));
            }
        }
    }

    if let Some(path) = body_file {
        let body = std::fs::read_to_string(&path)
            .map_err(|e| format!("failed to read --body-file {path}: {e}"))?;
        let task_ref = if ops
            .iter()
            .any(|op| op.get("client_id").and_then(|v| v.as_str()) == Some("task"))
        {
            "$task".to_string()
        } else {
            task_selector.clone().unwrap()
        };
        ops.push(json!({
            "op": "set_task_body",
            "task": task_ref,
            "body_md": body,
        }));
    }

    for step in steps {
        let (kind, text) = read_step_text(&step)?;
        let task_ref = if ops
            .iter()
            .any(|op| op.get("client_id").and_then(|v| v.as_str()) == Some("task"))
        {
            "$task".to_string()
        } else {
            task_selector.clone().unwrap()
        };
        ops.push(json!({
            "op": "add_milestone",
            "client_id": format!("step_{}", ops.len()),
            "task": task_ref,
            "kind": kind,
            "text": text,
            "from": {
                "tab_id": std::env::var("CODETERM_TAB_ID").ok(),
                "session_id": std::env::var("CODETERM_SESSION_ID").ok(),
                "title": std::env::var("CODETERM_TAB_TITLE").ok(),
            },
        }));
    }

    for link in links {
        ops.push(json!({ "op": "link", "to": link }));
    }

    if ops.is_empty() {
        return Err("nothing to record".into());
    }

    let ops_value = Value::Array(ops);
    let stable_key =
        key.unwrap_or_else(|| implicit_key(plan_id.as_deref().unwrap_or(""), &ops_value));
    let mut params = json!({
        "key": stable_key,
        "ops": ops_value,
    });
    if let Ok(tab_id) = std::env::var("CODETERM_TAB_ID") {
        params["caller_tab_id"] = json!(tab_id);
    }
    if let Some(plan_id) = plan_id {
        params["plan_id"] = json!(plan_id);
    }
    let mut result = call_plan_rpc(client, "plan.batch", params)?;
    if let Some(hint) = maybe_orchestration_reminder() {
        push_hints(&mut result, vec![hint]);
    }
    Ok(result)
}
