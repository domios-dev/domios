use crate::args::plan::PlanSectionAction;
use crate::client::RpcClient;
use crate::cmd::plan::{call_plan_rpc, maybe_inject_caller_tab, resolve_text};
use serde_json::{json, Value};

pub fn handle(
    client: &mut dyn RpcClient,
    json_output: bool,
    action: PlanSectionAction,
) -> Result<Option<Value>, String> {
    if let PlanSectionAction::Show {
        plan_id,
        section_id,
    } = &action
    {
        let val = call_plan_rpc(
            client,
            "plan.section.show",
            json!({ "plan_id": plan_id, "section_id": section_id }),
        )?;
        if json_output {
            return Ok(Some(val));
        }
        print!("{}", render_section_show(&val));
        return Ok(None);
    }
    section_mutation(client, action).map(Some)
}

pub(crate) fn render_section_show(val: &Value) -> String {
    let section = val.get("section").unwrap_or(&Value::Null);
    let id = section.get("id").and_then(|v| v.as_str()).unwrap_or("?");
    let title = val
        .get("title")
        .and_then(|v| v.as_str())
        .or_else(|| section.get("title").and_then(|v| v.as_str()))
        .unwrap_or(id);
    let handle = section.get("handle").and_then(|v| v.as_str());
    let tags = section
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    let markdown = val
        .get("markdown")
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    let mut out = format!("# {title}\n\n");
    out.push_str(&format!("id: {id}\n"));
    if let Some(handle) = handle.filter(|h| !h.is_empty()) {
        out.push_str(&format!("handle: {handle}\n"));
    }
    if !tags.is_empty() {
        out.push_str(&format!("tags: {tags}\n"));
    }
    if !markdown.trim().is_empty() {
        out.push('\n');
        out.push_str(markdown.trim_end());
        out.push('\n');
    }
    out
}

fn section_mutation(
    client: &mut dyn RpcClient,
    action: PlanSectionAction,
) -> Result<Value, String> {
    match action {
        PlanSectionAction::Show { .. } => unreachable!("Show handled before mutation dispatch"),
        PlanSectionAction::Attach {
            plan_id,
            section_id,
            file,
        } => call_plan_rpc(
            client,
            "plan.section.attach",
            json!({ "plan_id": plan_id, "section_id": section_id, "path": file }),
        ),
        PlanSectionAction::Add {
            plan_id,
            title,
            body,
            body_file,
            parent,
            tags,
            primary,
            handle,
        } => {
            let body_text = resolve_text(body, body_file)?.unwrap_or_default();
            let mut params = json!({
                "plan_id": plan_id,
                "title": title,
                "body": body_text,
            });
            if let Some(p) = parent {
                params["parent"] = json!(p);
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
            maybe_inject_caller_tab(&mut params);
            call_plan_rpc(client, "plan.section.add", params)
        }
        PlanSectionAction::Update {
            plan_id,
            section_id,
            title,
            body,
            body_file,
            tags,
            primary,
            handle,
        } => {
            let body_text = resolve_text(body, body_file)?;
            let mut params = json!({ "plan_id": plan_id, "section_id": section_id });
            if let Some(t) = title {
                params["title"] = json!(t);
            }
            if let Some(b) = body_text {
                params["body"] = json!(b);
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
            maybe_inject_caller_tab(&mut params);
            call_plan_rpc(client, "plan.section.update", params)
        }
        PlanSectionAction::Rm {
            plan_id,
            section_id,
        } => call_plan_rpc(
            client,
            "plan.section.rm",
            json!({ "plan_id": plan_id, "section_id": section_id }),
        ),
    }
}
