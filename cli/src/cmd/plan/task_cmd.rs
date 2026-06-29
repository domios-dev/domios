use serde_json::{json, Value};

use crate::args::plan::PlanTaskAction;
use crate::client::SocketClient;
use crate::cmd::hints::{
    maybe_orchestration_reminder, push_hints, task_model_hint, task_quality_guard_hint,
    task_sections_hint,
};
use crate::cmd::plan_linking::render_relink_report;

use super::{call, lane_params, resolve_text};

pub(super) fn run_task(
    client: &mut SocketClient,
    json: bool,
    action: PlanTaskAction,
) -> Result<Option<Value>, String> {
    match action {
        PlanTaskAction::List { plan_id, status } => {
            let mut params = json!({
                "active": status.is_none(),
                "mine": false,
                "unassigned": false,
                "plan_id": plan_id,
            });
            if let Some(project) = std::env::current_dir()
                .ok()
                .map(|path| path.display().to_string())
            {
                params["project"] = json!(project);
            }
            if let Some(status) = status {
                params["status"] = json!(status);
            }
            call(client, "task.list", params).map(Some)
        }
        PlanTaskAction::Add {
            plan_id,
            name,
            kind,
            sections,
            model,
            why,
            why_file,
            criterion,
            test_plan,
            correctness,
            body,
            body_file,
            intent,
            intent_file,
            parent,
            assign_pane,
            workspace_id,
            workspace_name,
            worktree,
            branch,
            base,
            destination,
            cleanup_status,
            cleanup_hints,
            group,
            priority,
            tags,
            primary,
            handle,
            loop_from: _,
        } => {
            let why_text = resolve_text(why, why_file)?;
            let body_text = resolve_text(body, body_file)?;
            let intent_text = resolve_text(intent, intent_file)?;
            let has_sections = sections
                .as_deref()
                .map(|s| s.split(',').any(|item| !item.trim().is_empty()))
                .unwrap_or(false);
            let has_model = model.as_deref().is_some_and(|s| !s.trim().is_empty());
            let has_why = why_text.as_deref().is_some_and(|s| !s.trim().is_empty());
            let has_criteria = criterion.iter().any(|s| !s.trim().is_empty());
            let has_test_plan = test_plan.as_deref().is_some_and(|s| !s.trim().is_empty());
            let has_correctness = correctness.as_deref().is_some_and(|s| !s.trim().is_empty());
            let mut params = json!({ "plan_id": plan_id.clone(), "name": name });
            if let Some(k) = kind {
                params["type"] = json!(k);
            }
            if let Some(s) = sections {
                params["sections"] = json!(s);
            }
            if let Some(m) = model {
                params["model"] = json!(m);
            }
            if let Some(w) = why_text {
                params["why"] = json!(w);
            }
            if !criterion.is_empty() {
                params["acceptance_criteria"] = json!(criterion);
            }
            if let Some(test_plan) = test_plan {
                params["test_plan"] = json!(test_plan);
            }
            if let Some(correctness) = correctness {
                params["correctness"] = json!(correctness);
            }
            if let Some(b) = body_text {
                params["body"] = json!(b);
            }
            if let Some(i) = intent_text {
                params["intent"] = json!(i);
            }
            if let Some(p) = parent {
                params["parent"] = json!(p);
            }
            if let Some(p) = assign_pane {
                params["assign_pane"] = json!(p);
            }
            if let Some(lane) = lane_params(
                params
                    .get("assign_pane")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                workspace_id,
                workspace_name,
                worktree,
                branch,
                base,
                destination,
                cleanup_status,
                cleanup_hints,
            ) {
                params["lane"] = lane;
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
            let mut val = call(client, "plan.task.add", params)?;
            if let Some(task_id) = val
                .get("task_id")
                .and_then(|v| v.as_str())
                .map(str::to_string)
            {
                let mut hints = vec![];
                let linked_has_sections = val
                    .get("section_ids")
                    .and_then(|v| v.as_array())
                    .is_some_and(|items| !items.is_empty());
                if !has_sections && !linked_has_sections {
                    hints.push(task_sections_hint(&plan_id, &task_id));
                }
                if !has_model || !has_why {
                    hints.push(task_model_hint(&plan_id, &task_id));
                }
                if let Some(hint) = task_quality_guard_hint(
                    &plan_id,
                    &task_id,
                    has_criteria,
                    has_test_plan,
                    has_correctness,
                ) {
                    hints.push(hint);
                }
                if let Some(hint) = maybe_orchestration_reminder() {
                    hints.push(hint);
                }
                push_hints(&mut val, hints);
            }
            Ok(Some(val))
        }
        PlanTaskAction::Update {
            plan_id,
            task_id,
            name,
            kind,
            model,
            why,
            why_file,
            status,
            status_label,
            body,
            body_file,
            intent,
            intent_file,
            sections,
            criterion,
            test_plan,
            correctness,
            parent,
            pane,
            workspace_id,
            workspace_name,
            worktree,
            branch,
            base,
            destination,
            cleanup_status,
            cleanup_hints,
            group,
            priority,
            tags,
            primary,
            handle,
        } => crate::cmd::plan_task_update::run(
            client,
            plan_id,
            task_id,
            name,
            kind,
            model,
            why,
            why_file,
            status,
            status_label,
            body,
            body_file,
            intent,
            intent_file,
            sections,
            criterion,
            test_plan,
            correctness,
            parent,
            pane,
            workspace_id,
            workspace_name,
            worktree,
            branch,
            base,
            destination,
            cleanup_status,
            cleanup_hints,
            group,
            priority,
            tags,
            primary,
            handle,
        )
        .map(Some),
        PlanTaskAction::Retag {
            plan_id,
            task_id,
            tags,
            primary,
        } => crate::cmd::plan_task_retag::run(client, json, plan_id, task_id, tags, primary),
        PlanTaskAction::Close {
            plan_id,
            task_id,
            result,
            result_file,
            mesh: _,
        } => {
            let result_text = resolve_text(result, result_file)?;
            let mut params = json!({ "plan_id": plan_id, "task_id": task_id });
            if let Some(r) = result_text {
                params["result"] = json!(r);
            }
            call(client, "plan.task.close", params).map(Some)
        }
        PlanTaskAction::Assign {
            plan_id,
            task_id,
            pane,
            mesh,
        } => {
            let mut params = json!({
                "plan_id": plan_id,
                "task_id": task_id,
                "pane": pane,
            });
            if let Some(p) = mesh {
                params["peer"] = json!(p);
            }
            call(client, "plan.task.assign", params).map(Some)
        }
        PlanTaskAction::Link {
            plan_id,
            task_id,
            sections,
        } => call(
            client,
            "plan.task.link",
            json!({ "plan_id": plan_id, "task_id": task_id, "sections": sections }),
        )
        .map(Some),
        PlanTaskAction::Relink { plan_id, apply } => {
            let val = call(
                client,
                "plan.task.relink",
                json!({ "plan_id": plan_id, "apply": apply }),
            )?;
            if json {
                return Ok(Some(val));
            }
            print!("{}", render_relink_report(&val));
            Ok(None)
        }
        PlanTaskAction::Rm { plan_id, task_id } => call(
            client,
            "plan.task.rm",
            json!({ "plan_id": plan_id, "task_id": task_id }),
        )
        .map(Some),
    }
}
