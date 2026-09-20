use serde_json::{json, Value};

use crate::args::plan::PlanTaskAction;
use crate::client::RpcClient;
use crate::cmd::hints::{
    maybe_orchestration_reminder, push_hints, task_model_hint, task_quality_guard_hint,
    task_sections_hint,
};
use crate::cmd::plan_linking::render_relink_report;

use super::{call, inject_trusted_session, lane_params, maybe_inject_caller_tab, resolve_text};

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

fn parse_criteria_file(content: &str) -> Vec<String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return vec![];
    }
    if trimmed.starts_with('[') {
        if let Ok(items) = serde_json::from_str::<Vec<String>>(trimmed) {
            return items;
        }
    }
    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

fn merge_criteria(
    mut criterion: Vec<String>,
    criterion_file: Option<String>,
) -> Result<Vec<String>, String> {
    if let Some(content) = resolve_text(None, criterion_file)? {
        criterion.extend(parse_criteria_file(&content));
    }
    Ok(criterion)
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
        let tab = attempt
            .get("executor_tab_id")
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
            id, tab, status, provider, model, when
        ));
    }
    out
}

fn render_attempt(value: &Value) -> String {
    let attempt = value.get("attempt").unwrap_or(value);
    let mut out = String::new();
    for key in [
        "attempt_id",
        "executor_tab_id",
        "executor_title",
        "status",
        "provider",
        "agent_type",
        "model",
        "session_id",
        "role",
        "parent_tab_id",
        "orchestrator_tab_id",
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

fn render_dispatch_response(value: &Value) -> String {
    let tab = value.get("tab_id").and_then(Value::as_str).unwrap_or("?");
    let agent = value
        .get("agent")
        .and_then(Value::as_str)
        .unwrap_or("agent");
    let state = value.get("state").and_then(Value::as_str).unwrap_or("");
    let ready = value
        .get("readiness")
        .and_then(|readiness| readiness.get("ready"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if state == "awaiting_dispatch_ready" && !ready {
        let event = value
            .get("readiness")
            .and_then(|readiness| readiness.get("awaiting_event"))
            .and_then(Value::as_str)
            .unwrap_or("dispatch_ready");
        return format!("Accepted {agent} worker Tab {tab}; awaiting {event}\n");
    }
    // A spawned Worker is not an accepted dispatch: binding is async, so the
    // response names the spawn and the event still owed.
    if state == "awaiting_worker_ack" && !ready {
        let event = value
            .get("readiness")
            .and_then(|readiness| readiness.get("awaiting_event"))
            .and_then(Value::as_str)
            .unwrap_or("worker_ack");
        return format!("Spawned {agent} worker Tab {tab}; awaiting {event}\n");
    }
    format!("Dispatched {agent} worker Tab {tab}\n")
}

/// Conspicuous but non-blocking: the dispatch already succeeded by the time this
/// prints, and each line carries the stable id plus the exact fix.
fn print_safety_nudges(val: &Value) {
    let Some(nudges) = val.get("safety_nudges").and_then(Value::as_array) else {
        return;
    };
    if nudges.is_empty() {
        return;
    }
    eprintln!("--- lane safety: {} advisory finding(s) ---", nudges.len());
    for nudge in nudges {
        let field = |key: &str| nudge.get(key).and_then(Value::as_str).unwrap_or("");
        eprintln!("  [{}] {}", field("id"), field("reason"));
        eprintln!("      fix: {}", field("safe_alternative"));
    }
}

fn task_status(item: &Value) -> &str {
    item.get("task")
        .unwrap_or(item)
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("")
}

fn hidden_task_summary(shown: usize, all_tasks: &[Value]) -> Option<String> {
    let mut counts = std::collections::BTreeMap::<&str, usize>::new();
    for task in all_tasks {
        let status = task_status(task);
        if matches!(status, "paused" | "done" | "cancelled") {
            *counts.entry(status).or_default() += 1;
        }
    }
    let hidden: usize = counts.values().sum();
    if hidden == 0 {
        return None;
    }
    let breakdown = counts
        .into_iter()
        .map(|(status, count)| format!("{status}={count}"))
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!(
        "{shown} shown; {hidden} hidden ({breakdown}). Use --all."
    ))
}

const EMPTY_REASON_PLAN_NOT_FOUND: &str = "plan_not_found";
const EMPTY_REASON_PLAN_ARCHIVED: &str = "plan_archived";
const EMPTY_REASON_PLAN_OUTSIDE_PROJECT: &str = "plan_outside_project";
const EMPTY_REASON_NO_PLANS_IN_SCOPE: &str = "no_plans_in_scope";
const EMPTY_REASON_NO_TASKS_IN_SCOPE: &str = "no_tasks_in_scope";
const EMPTY_REASON_ALL_TASKS_INACTIVE: &str = "all_tasks_inactive";
const EMPTY_REASON_NO_MATCH_FOR_FILTERS: &str = "no_match_for_filters";

/// Renders the daemon's verdict verbatim. The CLI cannot resolve project
/// identity — a linked worktree is a distinct directory but the same project.
pub(crate) fn render_empty_reason(reason: &Value) -> Option<String> {
    let field = |key: &str| reason.get(key).and_then(Value::as_str).unwrap_or("?");
    match reason.get("kind").and_then(Value::as_str)? {
        EMPTY_REASON_PLAN_NOT_FOUND => Some(format!("Plan {} was not found.", field("plan_id"))),
        EMPTY_REASON_PLAN_ARCHIVED => Some(format!(
            "Plan {} is archived; archived plans are excluded from task listing.",
            field("plan_id")
        )),
        EMPTY_REASON_PLAN_OUTSIDE_PROJECT => Some(format!(
            "Plan {} belongs to {}, outside the current project {}.",
            field("plan_id"),
            field("plan_project"),
            field("requested_project")
        )),
        EMPTY_REASON_NO_PLANS_IN_SCOPE => Some("No plans in this project.".into()),
        EMPTY_REASON_NO_TASKS_IN_SCOPE => Some("This plan has no tasks yet.".into()),
        EMPTY_REASON_ALL_TASKS_INACTIVE => Some(format!(
            "{} task(s) exist but none is active. Use --all or --status.",
            reason.get("hidden").and_then(Value::as_u64).unwrap_or(0)
        )),
        EMPTY_REASON_NO_MATCH_FOR_FILTERS => Some("No task matched the supplied filters.".into()),
        _ => None,
    }
}

pub(super) fn run_task(
    client: &mut dyn RpcClient,
    json: bool,
    action: PlanTaskAction,
) -> Result<Option<Value>, String> {
    match action {
        PlanTaskAction::List {
            plan_id,
            status,
            all,
        } => {
            let mut params = json!({
                "active": status.is_none() && !all,
                "mine": false,
                "unassigned": false,
            });
            maybe_inject_caller_tab(&mut params);
            params["require_plan_id"] = json!(true);
            if let Some(plan_id) = &plan_id {
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
            if let Some(status) = &status {
                params["status"] = json!(status);
            }
            let mut response = call(client, "task.list", params.clone())?;
            let shown = response
                .get("tasks")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            if status.is_none() && !all {
                let mut all_params = params.clone();
                all_params["active"] = json!(false);
                let all_response = call(client, "task.list", all_params)?;
                if let Some(summary) = all_response
                    .get("tasks")
                    .and_then(Value::as_array)
                    .and_then(|tasks| hidden_task_summary(shown, tasks))
                {
                    response["task_list_summary"] = json!(summary);
                }
            }
            if shown == 0 && !response["task_list_summary"].is_string() {
                if let Some(reason) = response.get("empty_reason").cloned() {
                    let kind = reason
                        .get("kind")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    if matches!(
                        kind.as_str(),
                        EMPTY_REASON_PLAN_NOT_FOUND
                            | EMPTY_REASON_PLAN_ARCHIVED
                            | EMPTY_REASON_PLAN_OUTSIDE_PROJECT
                    ) {
                        // Explicitly requested plan names are refusals when unavailable, not empty results, so a mis-rooted plan cannot look healthy.
                        let detail = render_empty_reason(&reason)
                            .unwrap_or_else(|| "task list refused".to_string());
                        return Err(format!("{kind}: {detail}"));
                    }
                    if let Some(message) = render_empty_reason(&reason) {
                        response["task_list_message"] = json!(message);
                    }
                }
            }
            Ok(Some(response))
        }
        PlanTaskAction::Show {
            plan_id,
            task_id,
            siblings,
            all,
        } => {
            let val = call(
                client,
                "task.show",
                json!({ "plan_id": plan_id, "task_id": task_id }),
            )?;
            if json {
                return Ok(Some(val));
            }
            crate::cmd::task::print_task_result(
                false,
                &val,
                Some(crate::cmd::task::TaskShowOpts {
                    list_siblings: siblings,
                    include_done_siblings: all,
                }),
            );
            Ok(None)
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
            criterion_file,
            external_ref,
            test_plan,
            test_plan_file,
            qa_plan,
            qa_plan_file,
            correctness,
            correctness_file,
            body,
            body_file,
            role_guidance,
            role_guidance_file,
            executor_role,
            personality,
            intent,
            intent_file,
            execution_intent,
            parent,
            depends_on,
            assign_tab,
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
            if executor_role.trim().is_empty() {
                return Err(
                    "--executor-role names who executes this task and cannot be blank".to_string(),
                );
            }
            let personality = codeterm_types::plan::validate_executor_name(&personality)
                .map_err(|err| format!("--personality: {err}"))?;
            let role_guidance_text =
                super::resolve_role_guidance(role_guidance, role_guidance_file)?;
            let intent_text = resolve_text(intent, intent_file)?;
            let qa_plan_text = resolve_text(qa_plan, qa_plan_file)?;
            let test_plan = resolve_text(test_plan, test_plan_file)?;
            let correctness = resolve_text(correctness, correctness_file)?;
            let criterion = merge_criteria(criterion, criterion_file)?;
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
            if !external_ref.is_empty() {
                params["external_refs"] = json!(external_ref);
            }
            if let Some(test_plan) = test_plan {
                params["test_plan"] = json!(test_plan);
            }
            if let Some(qa_plan) = qa_plan_text {
                params["qa_plan"] = json!(qa_plan);
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
            params["executor_role"] = json!(executor_role);
            params["personality"] = json!(personality);
            if let Some(i) = intent_text {
                params["intent"] = json!(i);
            }
            if let Some(execution_intent) = execution_intent {
                params["execution_intent"] = json!(execution_intent);
            }
            if let Some(p) = parent {
                params["parent"] = json!(p);
            }
            if !depends_on.is_empty() {
                params["depends_on"] = json!(depends_on);
            }
            if let Some(p) = assign_tab {
                params["assign_tab"] = json!(p);
            }
            if let Some(lane) = lane_params(
                params
                    .get("assign_tab")
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
            inject_trusted_session(&mut params);
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
            criterion_file,
            replace_criteria,
            external_ref,
            test_plan,
            test_plan_file,
            qa_plan,
            qa_plan_file,
            correctness,
            correctness_file,
            parent,
            depends_on,
            remove_dependency,
            replace_dependencies,
            clear_dependencies,
            tab,
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
            let test_plan = resolve_text(test_plan, test_plan_file)?;
            let correctness = resolve_text(correctness, correctness_file)?;
            let criterion = merge_criteria(criterion, criterion_file)?;
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
                replace_criteria,
                test_plan,
                qa_plan,
                qa_plan_file,
                correctness,
                parent,
                depends_on,
                remove_dependency,
                replace_dependencies,
                clear_dependencies,
                tab,
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
                external_ref,
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
        PlanTaskAction::Assign {
            plan_id,
            task_id,
            tab,
            mesh,
        } => {
            let (plan_ref, task_ref) = split_task_ref(Some(plan_id), task_id);
            let mut params = json!({
                "task_id": task_ref,
                "tab": tab,
            });
            if let Some(pid) = plan_ref {
                params["plan_id"] = json!(pid);
            }
            if let Some(p) = mesh {
                params["peer"] = json!(p);
            }
            inject_trusted_session(&mut params);
            call(client, "plan.task.assign", params).map(Some)
        }
        PlanTaskAction::Dispatch {
            plan_id,
            task_id,
            agent,
            authorized_gates,
            canonical_writer,
            model,
            reasoning_effort,
            parent,
            workspace,
            cwd,
            execution_intent,
            provider_reserve_pct,
        } => {
            let (plan_ref, task_ref) = split_task_ref(Some(plan_id), task_id);
            let mut params = json!({
                "task_id": task_ref,
                "execution_intent": execution_intent,
                "provider_reserve_pct": provider_reserve_pct,
                "authorized_gates": authorized_gates,
            });
            if let Some(pid) = plan_ref {
                params["plan_id"] = json!(pid);
            }
            if canonical_writer {
                params["placement"] = json!("canonical_writer");
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
                params["parent_tab_id"] = json!(parent);
            }
            if let Some(workspace) = workspace {
                params["workspace_id"] = json!(workspace);
            }
            if let Some(cwd) = cwd {
                params["cwd"] = json!(cwd);
            }
            inject_trusted_session(&mut params);
            let val = call(client, "plan.task.dispatch", params)?;
            if json {
                return Ok(Some(val));
            }
            if let Some(warnings) = val.get("warnings").and_then(|v| v.as_array()) {
                for w in warnings.iter().filter_map(|v| v.as_str()) {
                    eprintln!("warning: {w}");
                }
            }
            print_safety_nudges(&val);
            print!("{}", render_dispatch_response(&val));
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
                params["parent_tab_id"] = json!(parent);
            } else if let Ok(parent) = std::env::var("CODETERM_TAB_ID") {
                if !parent.trim().is_empty() {
                    params["parent_tab_id"] = json!(parent);
                }
            }
            if let Some(workspace) = workspace {
                params["workspace_id"] = json!(workspace);
            }
            if let Some(title) = title {
                params["title"] = json!(title);
            }
            inject_trusted_session(&mut params);
            let val = call(client, "plan.task.respawn", params)?;
            if json {
                return Ok(Some(val));
            }
            let tab = val.get("tab_id").and_then(|v| v.as_str()).unwrap_or("?");
            let attempt_id = val
                .get("attempt")
                .and_then(|v| v.get("attempt_id"))
                .or_else(|| val.get("attempt").and_then(|v| v.get("id")))
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            println!("Respawned worker Tab {tab} (attempt {attempt_id})");
            Ok(None)
        }
        PlanTaskAction::AdoptCheckpoint { plan_id, task_id } => {
            let mut params = json!({ "plan_id": plan_id, "task_id": task_id });
            maybe_inject_caller_tab(&mut params);
            let val = call(client, "plan.task.adopt_checkpoint", params)?;
            if json {
                return Ok(Some(val));
            }
            println!(
                "Adopted checkpoint as owner epoch {} (attempt {}). Run codeterm agent ack.",
                val.get("owner_epoch")
                    .and_then(|value| value.as_u64())
                    .unwrap_or(0),
                val.get("attempt_id")
                    .and_then(|value| value.as_str())
                    .unwrap_or("?")
            );
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
            inject_trusted_session(&mut params);
            call(client, "plan.task.rm", params).map(Some)
        }
    }
}

#[cfg(test)]
mod tests;
