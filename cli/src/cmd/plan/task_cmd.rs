use serde_json::{json, Value};

use crate::args::plan::PlanTaskAction;
use crate::client::RpcClient;
use crate::cmd::hints::{
    maybe_orchestration_reminder, push_hints, task_model_hint, task_quality_guard_hint,
    task_sections_hint,
};
use crate::cmd::plan_linking::render_relink_report;

use super::{call, lane_params, maybe_inject_caller_pane, resolve_text};

/// Parent-free addressing: with one positional the ref is the task and the plan is inferred
/// daemon-side; with two, the first is the plan ref and the second the task ref.
fn split_task_ref(first: Option<String>, second: Option<String>) -> (Option<String>, String) {
    match (first, second) {
        (Some(task), None) => (None, task),
        (Some(plan), Some(task)) => (Some(plan), task),
        (None, Some(task)) => (None, task),
        (None, None) => unreachable!("split_task_ref requires at least one task reference"),
    }
}

fn render_attempts(value: &Value) -> String {
    let attempts = value
        .get("attempts")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if attempts.is_empty() {
        return "No attempts.\n".into();
    }
    let mut out = String::new();
    for attempt in attempts {
        let id = attempt
            .get("attempt_id")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let pane = attempt
            .get("executor_pane_id")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let status = attempt
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let provider = attempt
            .get("provider")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let model = attempt.get("model").and_then(|v| v.as_str()).unwrap_or("-");
        let when = attempt
            .get("reported_at")
            .or_else(|| attempt.get("assigned_at"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        out.push_str(&format!(
            "{:<24} {:<24} {:<10} {:<10} {:<18} at={}\n",
            id, pane, status, provider, model, when
        ));
    }
    out
}

fn render_attempt(value: &Value) -> String {
    let attempt = value.get("attempt").unwrap_or(value);
    let mut out = String::new();
    for key in [
        "attempt_id",
        "executor_pane_id",
        "executor_title",
        "status",
        "provider",
        "agent_type",
        "model",
        "session_id",
        "role",
        "parent_pane_id",
        "orchestrator_pane_id",
        "mesh_peer",
        "branch",
        "worktree_path",
        "base",
        "destination",
        "assigned_at",
        "reported_at",
        "reviewed_at",
        "closed_at",
        "competition_group_id",
        "competition_lane",
        "judge_verdict",
        "judge_reason",
        "judged_at",
        "winner",
        "last_report_status",
    ] {
        if let Some(v) = attempt.get(key).filter(|v| !v.is_null()) {
            let rendered = v
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| v.to_string());
            out.push_str(&format!("{key}: {rendered}\n"));
        }
    }
    if let Some(command) = attempt.get("report_command").and_then(|v| v.as_str()) {
        out.push_str("\nreport_command:\n");
        out.push_str(command);
        out.push('\n');
    }
    if let Some(report) = attempt.get("report").filter(|v| !v.is_null()) {
        out.push_str("\nreport:\n");
        out.push_str(&report.to_string());
        out.push('\n');
    }
    if let Some(report) = attempt.get("last_report").and_then(|v| v.as_str()) {
        out.push_str("\nlast_report:\n");
        out.push_str(report);
        out.push('\n');
    }
    if let Some(prompt) = attempt.get("prompt").and_then(|v| v.as_str()) {
        out.push_str("\nprompt:\n");
        out.push_str(prompt);
        out.push('\n');
    }
    out
}

pub(super) fn run_task(
    client: &mut dyn RpcClient,
    json: bool,
    action: PlanTaskAction,
) -> Result<Option<Value>, String> {
    match action {
        PlanTaskAction::List { plan_id, status } => {
            let mut params = json!({
                "active": status.is_none(),
                "mine": false,
                "unassigned": false,
            });
            maybe_inject_caller_pane(&mut params);
            params["require_plan_id"] = json!(true);
            if let Some(plan_id) = plan_id {
                params["plan_id"] = json!(plan_id);
            }
            if !client.is_remote() {
                if let Some(project) = std::env::current_dir()
                    .ok()
                    .map(|path| path.display().to_string())
                {
                    params["project"] = json!(project);
                }
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
            role_guidance,
            role_guidance_file,
            executor_role,
            personality,
            intent,
            intent_file,
            parent,
            depends_on,
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
            force,
            loop_from: _,
        } => {
            let why_text = resolve_text(why, why_file)?;
            let body_text = resolve_text(body, body_file)?;
            let role_guidance_text = resolve_text(role_guidance, role_guidance_file)?;
            let intent_text = resolve_text(intent, intent_file)?;
            let has_sections = sections
                .as_deref()
                .map(|s| s.split(',').any(|item| !item.trim().is_empty()))
                .unwrap_or(false);
            let has_model = model.as_deref().is_some_and(|s| !s.trim().is_empty());
            let has_why = why_text.as_deref().is_some_and(|s| !s.trim().is_empty());
            let has_body = body_text.as_deref().is_some_and(|s| !s.trim().is_empty());
            let has_criteria = criterion.iter().any(|s| !s.trim().is_empty());
            let has_test_plan = test_plan.as_deref().is_some_and(|s| !s.trim().is_empty());
            let has_correctness = correctness.as_deref().is_some_and(|s| !s.trim().is_empty());
            let mut params = json!({ "plan_id": plan_id.clone(), "name": name });
            if force {
                params["force"] = json!(true);
            }
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
            if let Some(guidance) = role_guidance_text {
                params["role_guidance"] = json!(guidance);
            }
            if let Some(role) = executor_role {
                params["executor_role"] = json!(role);
            }
            if let Some(name) = personality {
                params["personality"] = json!(name);
            }
            if let Some(i) = intent_text {
                params["intent"] = json!(i);
            }
            if let Some(p) = parent {
                params["parent"] = json!(p);
            }
            if !depends_on.is_empty() {
                params["depends_on"] = json!(depends_on);
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
            maybe_inject_caller_pane(&mut params);
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
                    has_body,
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
            role_guidance,
            role_guidance_file,
            executor_role,
            personality,
            intent,
            intent_file,
            sections,
            criterion,
            test_plan,
            correctness,
            parent,
            depends_on,
            clear_dependencies,
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
        } => {
            let (plan_ref, task_ref) = split_task_ref(plan_id, task_id);
            crate::cmd::plan_task_update::run(
                client,
                plan_ref,
                task_ref,
                name,
                kind,
                model,
                why,
                why_file,
                status,
                status_label,
                body,
                body_file,
                role_guidance,
                role_guidance_file,
                executor_role,
                personality,
                intent,
                intent_file,
                sections,
                criterion,
                test_plan,
                correctness,
                parent,
                depends_on,
                clear_dependencies,
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
            .map(Some)
        }
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
            let (plan_ref, task_ref) = split_task_ref(Some(plan_id), task_id);
            let result_text = resolve_text(result, result_file)?;
            let mut params = json!({ "task_id": task_ref });
            if let Some(pid) = plan_ref {
                params["plan_id"] = json!(pid);
            }
            if let Some(r) = result_text {
                params["result"] = json!(r);
            }
            call(client, "plan.task.close", params).map(Some)
        }
        PlanTaskAction::Pause {
            plan_id,
            task_id,
            reason,
        } => {
            let (plan_ref, task_ref) = split_task_ref(Some(plan_id), task_id);
            let mut params = json!({
                "task_id": task_ref,
                "reason": reason,
                "from": crate::cmd::plan::owner_from_env(),
            });
            if let Some(pid) = plan_ref {
                params["plan_id"] = json!(pid);
            }
            call(client, "plan.task.pause", params).map(Some)
        }
        PlanTaskAction::Resume { plan_id, task_id } => {
            let (plan_ref, task_ref) = split_task_ref(Some(plan_id), task_id);
            let mut params = json!({
                "task_id": task_ref,
                "from": crate::cmd::plan::owner_from_env(),
            });
            if let Some(pid) = plan_ref {
                params["plan_id"] = json!(pid);
            }
            call(client, "plan.task.resume", params).map(Some)
        }
        PlanTaskAction::Assign {
            plan_id,
            task_id,
            pane,
            mesh,
        } => {
            let (plan_ref, task_ref) = split_task_ref(Some(plan_id), task_id);
            let mut params = json!({
                "task_id": task_ref,
                "pane": pane,
            });
            if let Some(pid) = plan_ref {
                params["plan_id"] = json!(pid);
            }
            if let Some(p) = mesh {
                params["peer"] = json!(p);
            }
            call(client, "plan.task.assign", params).map(Some)
        }
        PlanTaskAction::Dispatch {
            plan_id,
            task_id,
            agent,
            model,
            reasoning_effort,
            parent,
            workspace,
            cwd,
        } => {
            let (plan_ref, task_ref) = split_task_ref(Some(plan_id), task_id);
            let mut params = json!({ "task_id": task_ref });
            if let Some(pid) = plan_ref {
                params["plan_id"] = json!(pid);
            }
            if let Some(agent) = agent {
                params["agent"] = json!(agent);
            }
            if let Some(model) = model {
                params["model"] = json!(model);
            }
            if let Some(effort) = reasoning_effort {
                params["reasoning_effort"] = json!(effort);
            }
            if let Some(parent) = parent {
                params["parent_pane_id"] = json!(parent);
            }
            if let Some(workspace) = workspace {
                params["workspace_id"] = json!(workspace);
            }
            if let Some(cwd) = cwd {
                params["cwd"] = json!(cwd);
            }
            maybe_inject_caller_pane(&mut params);
            let val = call(client, "plan.task.dispatch", params)?;
            if json {
                return Ok(Some(val));
            }
            if let Some(warnings) = val.get("warnings").and_then(|v| v.as_array()) {
                for w in warnings.iter().filter_map(|v| v.as_str()) {
                    eprintln!("warning: {w}");
                }
            }
            let pane = val.get("pane_id").and_then(|v| v.as_str()).unwrap_or("?");
            let agent = val.get("agent").and_then(|v| v.as_str()).unwrap_or("agent");
            println!("Dispatched {agent} worker Tab {pane}");
            Ok(None)
        }
        PlanTaskAction::Attempts { plan_id, task_id } => {
            let val = call(
                client,
                "plan.task.attempts",
                json!({ "plan_id": plan_id, "task_id": task_id }),
            )?;
            if json {
                return Ok(Some(val));
            }
            print!("{}", render_attempts(&val));
            Ok(None)
        }
        PlanTaskAction::Attempt {
            plan_id,
            task_id,
            attempt_id,
        } => {
            let val = call(
                client,
                "plan.task.attempt.show",
                json!({ "plan_id": plan_id, "task_id": task_id, "attempt_id": attempt_id }),
            )?;
            if json {
                return Ok(Some(val));
            }
            print!("{}", render_attempt(&val));
            Ok(None)
        }
        PlanTaskAction::Attach {
            plan_id,
            task_id,
            file,
        } => call(
            client,
            "plan.task.attach",
            json!({ "plan_id": plan_id, "task_id": task_id, "path": file }),
        )
        .map(Some),
        PlanTaskAction::Respawn {
            plan_id,
            task_id,
            attempt,
            provider,
            model,
            parent,
            workspace,
            title,
        } => {
            let mut params = json!({
                "plan_id": plan_id,
                "task_id": task_id,
                "attempt_id": attempt,
            });
            if let Some(provider) = provider {
                params["provider"] = json!(provider);
            }
            if let Some(model) = model {
                params["model"] = json!(model);
            }
            if let Some(parent) = parent {
                params["parent_pane_id"] = json!(parent);
            } else if let Ok(parent) = std::env::var("CODETERM_PANE_ID") {
                if !parent.trim().is_empty() {
                    params["parent_pane_id"] = json!(parent);
                }
            }
            if let Some(workspace) = workspace {
                params["workspace_id"] = json!(workspace);
            }
            if let Some(title) = title {
                params["title"] = json!(title);
            }
            let val = call(client, "plan.task.respawn", params)?;
            if json {
                return Ok(Some(val));
            }
            let pane = val.get("pane_id").and_then(|v| v.as_str()).unwrap_or("?");
            let attempt_id = val
                .get("attempt")
                .and_then(|v| v.get("attempt_id"))
                .or_else(|| val.get("attempt").and_then(|v| v.get("id")))
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            println!("Respawned worker Tab {pane} (attempt {attempt_id})");
            Ok(None)
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
        PlanTaskAction::Rm {
            plan_id,
            task_id,
            all,
            yes,
        } => {
            if all && !yes {
                return Err(
                    "refusing to remove all tasks without --yes; this resets the plan to planning mode"
                        .into(),
                );
            }
            let mut params = json!({ "all": all });
            if all {
                params["plan_id"] = json!(plan_id);
            } else {
                let (plan_ref, task_ref) = split_task_ref(Some(plan_id), task_id);
                params["task_id"] = json!(task_ref);
                if let Some(pid) = plan_ref {
                    params["plan_id"] = json!(pid);
                }
            }
            call(client, "plan.task.rm", params).map(Some)
        }
    }
}
