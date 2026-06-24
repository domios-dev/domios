use crate::args::plan::PlanSectionAction;
use crate::client::SocketClient;
use crate::cmd::plan::{call_plan_rpc, resolve_text};
use serde_json::{json, Value};

pub fn handle(client: &mut SocketClient, action: PlanSectionAction) -> Result<Value, String> {
    match action {
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
            call_plan_rpc(client, "plan.section.update", params)
        }
        PlanSectionAction::Set {
            plan_id,
            section_id,
            name,
            body,
            body_file,
        } => {
            let body_text = resolve_text(body, body_file)?;
            let mut params = json!({ "plan_id": plan_id, "section_id": section_id });
            if let Some(name) = name {
                params["name"] = json!(name);
            }
            if let Some(body) = body_text {
                params["body"] = json!(body);
            }
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
